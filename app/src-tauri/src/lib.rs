use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use xc_core::pretty::pretty_json;
use xc_core::vars::{Context, Scope, VariableInfo};
use xc_core::{CollectionInfo, EnvVar, RequestDoc};
use xc_engine::Timings;
use xc_runner::{AssertionResult, PhaseReport, Request, RunError, Session};
use xc_sync::import::{fetch_spec, OpenApiPreview};
use xc_sync::manage::{self, DropPosition, FolderKind};
use xc_sync::openapi::GroupBy;
use xc_sync::sync::{self, Decisions, OpView, Plan, Report, SyncStatus};

mod oauth;
mod runs;
mod secrets;

#[derive(Default)]
struct AppState {
    sessions: Mutex<HashMap<String, Session>>,
    runs: runs::Runs,
    secrets: secrets::Secrets,
    inflight: Mutex<HashMap<String, Inflight>>,
    plans: Arc<Plans>,
    writes: tokio::sync::Mutex<()>,
    watching: tokio::sync::Mutex<()>,
    watch: Mutex<Option<xc_watch::Watch>>,
}

/// Une requête en cours d'envoi : sa tâche et le drapeau qui interrompt ses scripts.
struct Inflight {
    task: tokio::task::AbortHandle,
    cancel: Arc<AtomicBool>,
}

/// Plans de synchro en cours, sous un identifiant ; un plan par collection au plus.
#[derive(Default)]
struct Plans {
    by_id: Mutex<HashMap<String, (String, Arc<Plan>)>>,
    count: AtomicU64,
}

impl Plans {
    /// Garde `plan` et oublie le plan précédent de la même collection.
    fn insert(&self, root: &str, plan: Plan) -> Reply<String> {
        let id = format!("plan-{}", self.count.fetch_add(1, Ordering::Relaxed) + 1);
        let mut plans = self.by_id.lock().map_err(err)?;
        plans.retain(|_, (plan_root, _)| plan_root != root);
        plans.insert(id.clone(), (root.to_owned(), Arc::new(plan)));
        Ok(id)
    }

    fn get(&self, id: &str) -> Reply<Arc<Plan>> {
        let plans = self.by_id.lock().map_err(err)?;
        let found = plans.get(id).map(|(_, plan)| Arc::clone(plan));
        found.ok_or_else(|| "plan introuvable : relancer la comparaison".to_owned())
    }

    /// Écrit le plan ; il n'est oublié qu'après une application réussie, pour que l'arbitrage puisse continuer quand
    /// l'application est refusée.
    fn apply(&self, id: &str, decisions: &Decisions) -> Reply<Report> {
        let report = self.get(id)?.apply(decisions).map_err(err)?;
        self.by_id.lock().map_err(err)?.remove(id);
        Ok(report)
    }
}

type Reply<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn root(path: &str) -> PathBuf {
    PathBuf::from(path)
}

/// Exécute une action sur le disque hors du fil principal.
async fn blocking<T, E>(action: impl FnOnce() -> Result<T, E> + Send + 'static) -> Reply<T>
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    tokio::task::spawn_blocking(action).await.map_err(err)?.map_err(err)
}

/// Exécute une action qui écrit dans une collection, une seule à la fois : deux écritures concurrentes (un
/// glisser-déposer pendant une duplication, un enregistrement pendant une synchro) liraient les mêmes `seq` ou les
/// mêmes noms libres et se contrediraient.
async fn writing<T, E>(state: &AppState, action: impl FnOnce() -> Result<T, E> + Send + 'static) -> Reply<T>
where
    T: Send + 'static,
    E: std::fmt::Display + Send + 'static,
{
    let _turn = state.writes.lock().await;
    blocking(action).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ResponseDto {
    status: u16,
    reason: String,
    http_version: String,
    remote_addr: String,
    headers: Vec<(String, String)>,
    body: String,
    pretty: Option<String>,
    size: usize,
    timings: Timings,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SendResult {
    method: String,
    url: String,
    unresolved: Vec<String>,
    /// Absente quand la requête n'est pas partie (script pré-requête en erreur ou qui l'ignore) ou n'a pas abouti.
    response: Option<ResponseDto>,
    assertions: Vec<AssertionResult>,
    scripts: ScriptsDto,
    error: Option<RunError>,
    skipped: bool,
}

#[derive(Serialize)]
struct ScriptsDto {
    pre: PhaseReport,
    post: PhaseReport,
    tests: PhaseReport,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendArgs {
    id: String,
    root: String,
    path: String,
    doc: RequestDoc,
    env: Option<String>,
}

#[tauri::command]
async fn open_collection(root: String) -> Reply<CollectionInfo> {
    blocking(move || {
        let mut info = xc_core::open_collection(Path::new(&root))?;
        xc_core::mark_deprecated(&mut info.items, &xc_sync::store::removed_files(Path::new(&root)));
        Ok::<_, xc_core::CoreError>(info)
    })
    .await
}

/// Changements du disque dans la collection `root`, annoncés à l'interface sous l'événement `collection-changed`.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DiskChange {
    root: String,
    paths: Vec<String>,
    truncated: bool,
}

/// Surveille la collection ouverte et remplace la surveillance précédente.
#[tauri::command]
async fn watch_collection<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    root: String,
) -> Reply<()> {
    let _turn = state.watching.lock().await;
    state.watch.lock().map_err(err)?.take();
    let announced = root.clone();
    let watch = blocking(move || {
        xc_watch::Watch::start(Path::new(&root), move |batch| {
            let change = DiskChange { root: announced.clone(), paths: batch.paths, truncated: batch.truncated };
            app.emit("collection-changed", change).ok();
        })
    })
    .await?;
    *state.watch.lock().map_err(err)? = Some(watch);
    Ok(())
}

#[tauri::command]
fn read_request(root: String, path: String) -> Reply<RequestDoc> {
    xc_core::read_request(Path::new(&root), &path).map_err(err)
}

#[tauri::command]
async fn save_request(state: State<'_, AppState>, root: String, path: String, doc: RequestDoc) -> Reply<bool> {
    writing(&state, move || xc_core::save_request(Path::new(&root), &path, &doc)).await
}

#[tauri::command]
fn read_environment(root: String, name: String) -> Reply<Vec<EnvVar>> {
    xc_core::read_environment(Path::new(&root), &name).map_err(err)
}

/// Enregistre les variables de l'environnement ; un environnement absent est refusé, sauf avec `create`. `false` : rien
/// n'a changé.
#[tauri::command]
async fn save_environment(
    state: State<'_, AppState>,
    root: String,
    name: String,
    vars: Vec<EnvVar>,
    create: bool,
) -> Reply<bool> {
    if !create && xc_core::read_environment(Path::new(&root), &name).is_err() {
        return writing(&state, move || xc_core::save_environment(Path::new(&root), &name, &vars, create)).await;
    }
    let store = Arc::clone(&state.secrets.0);
    let (dir, env) = (root.clone(), name.clone());
    let (vars, stored) = blocking(move || secrets::apply(&*store, &dir, &env, &vars)).await?;
    let wrote = writing(&state, move || xc_core::save_environment(Path::new(&root), &name, &vars, create)).await?;
    Ok(wrote || stored)
}

#[tauri::command]
async fn create_environment(state: State<'_, AppState>, root: String, name: String) -> Reply<String> {
    writing(&state, move || manage::create_environment(Path::new(&root), &name)).await
}

/// Les valeurs des secrets de `env`, lues avant qu'un renommage ou une duplication ne change le fichier. Un trousseau
/// indisponible n'en garde aucune : l'opération sur le fichier n'a alors rien à perdre.
async fn secrets_of(state: &AppState, root: &str, env: &str) -> Vec<(String, String)> {
    let (store, dir, env) = (Arc::clone(&state.secrets.0), root.to_owned(), env.to_owned());
    blocking(move || Ok::<_, String>(secrets::take(&*store, &dir, &env).unwrap_or_default())).await.unwrap_or_default()
}

/// Range les secrets de l'environnement `from` sous `to` ; les oublie sous `from` quand l'environnement a été renommé.
async fn carry_secrets(
    state: &AppState,
    root: &str,
    from: &str,
    to: &str,
    values: Vec<(String, String)>,
    forget: bool,
) -> Reply<()> {
    if values.is_empty() {
        return Ok(());
    }
    let (store, dir, from, to) = (Arc::clone(&state.secrets.0), root.to_owned(), from.to_owned(), to.to_owned());
    let label = to.clone();
    blocking(move || {
        secrets::put(&*store, &dir, &to, &values)?;
        if forget {
            let names: Vec<String> = values.into_iter().map(|(name, _)| name).collect();
            secrets::forget(&*store, &dir, &from, &names);
        }
        Ok::<_, String>(())
    })
    .await
    .map_err(|e| {
        format!(
            "L'environnement « {label} » est prêt, mais ses secrets n'ont pas pu être copiés ({e}) : ressaisis-les."
        )
    })
}

#[tauri::command]
async fn rename_environment(state: State<'_, AppState>, root: String, from: String, name: String) -> Reply<String> {
    let values = secrets_of(&state, &root, &from).await;
    let (dir, old) = (root.clone(), from.clone());
    let renamed = writing(&state, move || manage::rename_environment(Path::new(&dir), &old, &name)).await?;
    carry_secrets(&state, &root, &from, &renamed, values, true).await?;
    Ok(renamed)
}

#[tauri::command]
async fn clone_environment(state: State<'_, AppState>, root: String, from: String, name: String) -> Reply<String> {
    let values = secrets_of(&state, &root, &from).await;
    let (dir, old) = (root.clone(), from.clone());
    let cloned = writing(&state, move || manage::clone_environment(Path::new(&dir), &old, &name)).await?;
    carry_secrets(&state, &root, &from, &cloned, values, false).await?;
    Ok(cloned)
}

/// Envoie l'environnement à la corbeille du système et oublie ses secrets.
#[tauri::command]
async fn delete_environment(state: State<'_, AppState>, root: String, name: String) -> Reply<()> {
    let names = secrets::declared(&root, &name);
    let (dir, env) = (root.clone(), name.clone());
    writing(&state, move || {
        manage::delete_environment(Path::new(&dir), &env, |file| trash_context().delete(file).map_err(trash_message))
    })
    .await?;
    let (store, dir) = (Arc::clone(&state.secrets.0), root);
    blocking(move || {
        secrets::forget(&*store, &dir, &name, &names);
        Ok::<_, String>(())
    })
    .await
}

/// Environnement que la collection ouvre par défaut ; `None` pour n'en choisir aucun.
#[tauri::command]
async fn set_default_environment(state: State<'_, AppState>, root: String, name: Option<String>) -> Reply<()> {
    writing(&state, move || xc_core::set_default_environment(Path::new(&root), name.as_deref())).await
}

#[tauri::command]
async fn variables(
    state: State<'_, AppState>,
    root: String,
    path: String,
    doc: RequestDoc,
    env: Option<String>,
) -> Reply<Vec<VariableInfo>> {
    let dir = Path::new(&root);
    let mut session = state.sessions.lock().map_err(err)?.get(&root).cloned().unwrap_or_default();
    let (store, folder, picked) = (Arc::clone(&state.secrets.0), root.clone(), env.clone());
    session.secrets = blocking(move || Ok::<_, String>(secrets::load(&*store, &folder, picked.as_deref()))).await?;
    let ctx = Context::load(dir, &path).map_err(err)?;
    let scope = Scope::build(dir, &ctx, &path, &doc, env.as_deref(), &session.runtime_strings()).map_err(err)?;
    Ok(scope.with_overrides(session.overrides(env.as_deref())).infos())
}

#[tauri::command]
async fn send_request<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    args: SendArgs,
) -> Reply<SendResult> {
    let SendArgs { id, root: root_path, path, doc, env } = args;
    let mut session = state.sessions.lock().map_err(err)?.get(&root_path).cloned().unwrap_or_default();
    session.authorizer = Some(oauth::authorizer(&app));
    let (store, dir, picked) = (Arc::clone(&state.secrets.0), root_path.clone(), env.clone());
    session.secrets = blocking(move || Ok::<_, String>(secrets::load(&*store, &dir, picked.as_deref()))).await?;
    let cancel = Arc::new(AtomicBool::new(false));
    let (dir, flag) = (root(&root_path), Arc::clone(&cancel));
    let task = tokio::spawn(async move {
        let collection_name = xc_core::collection_name(&dir).unwrap_or_default();
        let request = Request {
            root: &dir,
            path: &path,
            doc: &doc,
            env: env.as_deref(),
            collection_name: &collection_name,
            execution_mode: "standalone",
            cancel: flag,
        };
        let outcome = xc_runner::run_request(request, &mut session).await;
        (outcome, session)
    });
    state.inflight.lock().map_err(err)?.insert(id.clone(), Inflight { task: task.abort_handle(), cancel });
    let joined = task.await;
    state.inflight.lock().map_err(err)?.remove(&id);
    let (outcome, session) = match joined {
        Ok(done) => done,
        Err(e) if e.is_cancelled() => return Err("Requête annulée".into()),
        Err(e) => return Err(err(e)),
    };
    state.sessions.lock().map_err(err)?.insert(root_path, session);

    let response = outcome.response.map(|res| {
        let size = res.body.len();
        let body = xc_engine::lossy_text(res.body);
        ResponseDto {
            status: res.status,
            reason: res.reason,
            http_version: res.http_version,
            remote_addr: res.remote_addr,
            size,
            pretty: pretty_json(&body),
            body,
            headers: res.headers,
            timings: res.timings,
        }
    });
    Ok(SendResult {
        method: outcome.method,
        url: outcome.url,
        unresolved: outcome.unresolved,
        response,
        assertions: outcome.assertions,
        scripts: ScriptsDto { pre: outcome.pre, post: outcome.post, tests: outcome.tests },
        error: outcome.error,
        skipped: outcome.skipped,
    })
}

#[tauri::command]
fn cancel_request(state: State<'_, AppState>, id: String) -> Reply<bool> {
    let inflight = state.inflight.lock().map_err(err)?.remove(&id);
    Ok(inflight
        .map(|running| {
            running.cancel.store(true, Ordering::Relaxed);
            running.task.abort();
        })
        .is_some())
}

#[tauri::command]
async fn parse_curl(command: String) -> Option<RequestDoc> {
    tokio::task::spawn_blocking(move || xc_sync::import::request_doc_from_curl(&command)).await.ok().flatten()
}

#[tauri::command]
async fn create_request_from_curl(
    state: State<'_, AppState>,
    root: String,
    folder: String,
    name: String,
    command: String,
) -> Reply<String> {
    writing(&state, move || xc_sync::import::create_request_from_curl(Path::new(&root), &folder, &name, &command)).await
}

#[tauri::command]
async fn preview_openapi(source: String) -> Reply<OpenApiPreview> {
    let text = fetch_spec(&source).await.map_err(err)?;
    tokio::task::spawn_blocking(move || xc_sync::import::preview(&text)).await.map_err(err)?.map_err(err)
}

#[tauri::command]
async fn import_openapi(
    state: State<'_, AppState>,
    source: String,
    location: String,
    group_by: String,
) -> Reply<String> {
    let group_by: GroupBy = group_by.parse().map_err(err)?;
    let text = fetch_spec(&source).await.map_err(err)?;
    let created =
        writing(&state, move || xc_sync::import::import_spec(&text, &source, Path::new(&location), group_by)).await?;
    Ok(created.display().to_string())
}

/// Ce qu'a donné l'import d'une collection Postman : sa racine, et ce qui n'a pas pu être converti.
#[derive(Debug, Serialize)]
struct PostmanImport {
    root: String,
    issues: Vec<xc_sync::postman::Issue>,
}

fn read_source(path: &str) -> Reply<String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path} illisible : {e}"))
}

/// Importe une collection Postman (v2.0 ou v2.1, fichier JSON) dans un nouveau dossier de `location`.
#[tauri::command]
async fn import_postman(state: State<'_, AppState>, source: String, location: String) -> Reply<PostmanImport> {
    let text = blocking(move || read_source(&source)).await?;
    let (root, issues) = writing(&state, move || xc_sync::import::import_postman(&text, Path::new(&location))).await?;
    Ok(PostmanImport { root: root.display().to_string(), issues })
}

/// Ajoute un environnement Postman à la collection `root` ; renvoie son nom.
#[tauri::command]
async fn import_postman_environment(state: State<'_, AppState>, root: String, source: String) -> Reply<String> {
    let text = blocking(move || read_source(&source)).await?;
    writing(&state, move || xc_sync::import::import_postman_environment(&text, Path::new(&root))).await
}

#[tauri::command]
async fn inspect_folder(path: String) -> Reply<FolderKind> {
    blocking(move || manage::inspect_folder(Path::new(&path))).await
}

#[tauri::command]
async fn create_collection(state: State<'_, AppState>, parent: String, name: String) -> Reply<String> {
    writing(&state, move || manage::create_collection(Path::new(&parent), &name).map(|root| root.display().to_string()))
        .await
}

#[tauri::command]
async fn init_collection(state: State<'_, AppState>, dir: String, name: String) -> Reply<String> {
    writing(&state, move || manage::init_collection(Path::new(&dir), &name).map(|root| root.display().to_string()))
        .await
}

#[tauri::command]
async fn create_request(state: State<'_, AppState>, root: String, folder: String, name: String) -> Reply<String> {
    writing(&state, move || manage::create_request(Path::new(&root), &folder, &name)).await
}

#[tauri::command]
async fn create_folder(state: State<'_, AppState>, root: String, parent: String, name: String) -> Reply<String> {
    writing(&state, move || manage::create_folder(Path::new(&root), &parent, &name)).await
}

#[tauri::command]
async fn rename_item(state: State<'_, AppState>, root: String, path: String, name: String) -> Reply<String> {
    writing(&state, move || manage::rename_item(Path::new(&root), &path, &name)).await
}

#[tauri::command]
async fn clone_item(state: State<'_, AppState>, root: String, path: String, name: String) -> Reply<String> {
    writing(&state, move || manage::clone_item(Path::new(&root), &path, &name)).await
}

/// Envoie la requête ou le dossier à la corbeille du système.
#[tauri::command]
async fn delete_item(state: State<'_, AppState>, root: String, path: String) -> Reply<()> {
    writing(&state, move || {
        manage::delete_item(Path::new(&root), &path, |item| trash_context().delete(item).map_err(trash_message))
    })
    .await
}

/// Sur macOS, `NSFileManager` plutôt que le Finder : aucune autorisation « contrôler le Finder » n'est demandée.
fn trash_context() -> trash::TrashContext {
    #[allow(unused_mut)]
    let mut context = trash::TrashContext::default();
    #[cfg(target_os = "macos")]
    trash::macos::TrashContextExtMacos::set_delete_method(&mut context, trash::macos::DeleteMethod::NsFileManager);
    context
}

/// Raison en français d'un échec de la corbeille.
fn trash_message(e: trash::Error) -> String {
    match e {
        trash::Error::TargetedRoot => "la racine d'un volume ne peut pas être mise à la corbeille".into(),
        trash::Error::CouldNotAccess { .. } => "élément introuvable ou inaccessible".into(),
        trash::Error::CanonicalizePath { .. } | trash::Error::ConvertOsString { .. } => "chemin illisible".into(),
        trash::Error::Os { code, .. } => format!("la corbeille a refusé l'élément (code {code})"),
        #[cfg(all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android")))]
        trash::Error::FileSystem { source, .. } => xc_core::io_message(&source),
        _ => "la corbeille a refusé l'élément".into(),
    }
}

#[tauri::command]
async fn move_item(
    state: State<'_, AppState>,
    root: String,
    path: String,
    target: String,
    position: DropPosition,
) -> Reply<String> {
    writing(&state, move || manage::move_item(Path::new(&root), &path, &target, position)).await
}

#[tauri::command]
fn sync_status(root: String) -> Reply<SyncStatus> {
    sync::status(Path::new(&root)).map_err(err)
}

/// Compare la spec (`source`, ou la source enregistrée) à la collection et garde le plan sous un identifiant, qui
/// remplace le plan précédent de la même collection.
#[tauri::command]
async fn sync_plan(
    state: State<'_, AppState>,
    root: String,
    source: Option<String>,
    pairings: Vec<(String, String)>,
) -> Reply<serde_json::Value> {
    let (text, recorded) = sync::read_source(Path::new(&root), source.as_deref()).await.map_err(err)?;
    let dir = root.clone();
    let computed = tokio::task::spawn_blocking(move || {
        let plan = sync::plan(Path::new(&dir), &text, recorded, &pairings).map_err(err)?;
        let reply = serde_json::to_value(&plan).map_err(err)?;
        Ok::<_, String>((plan, reply))
    })
    .await
    .map_err(err)?;
    let (plan, mut reply) = computed?;
    reply["id"] = serde_json::Value::String(state.plans.insert(&root, plan)?);
    Ok(reply)
}

#[tauri::command]
async fn sync_op_view(state: State<'_, AppState>, plan_id: String, key: String, decisions: Decisions) -> Reply<OpView> {
    let plan = state.plans.get(&plan_id)?;
    tokio::task::spawn_blocking(move || plan.op_view(&key, &decisions)).await.map_err(err)?.map_err(err)
}

#[tauri::command]
async fn sync_apply(state: State<'_, AppState>, plan_id: String, decisions: Decisions) -> Reply<Report> {
    let plans = Arc::clone(&state.plans);
    writing(&state, move || plans.apply(&plan_id, &decisions)).await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            open_collection,
            watch_collection,
            read_request,
            save_request,
            read_environment,
            save_environment,
            create_environment,
            rename_environment,
            clone_environment,
            delete_environment,
            set_default_environment,
            variables,
            send_request,
            cancel_request,
            parse_curl,
            create_request_from_curl,
            preview_openapi,
            import_openapi,
            import_postman,
            import_postman_environment,
            inspect_folder,
            create_collection,
            init_collection,
            create_request,
            create_folder,
            rename_item,
            clone_item,
            delete_item,
            move_item,
            sync_status,
            sync_plan,
            sync_op_view,
            sync_apply,
            runs::start_run,
            runs::cancel_run,
            runs::inspect_run_data,
            runs::export_run,
            secrets::secret_names,
            oauth::oauth_status,
            oauth::oauth_fetch,
            oauth::oauth_clear
        ])
        .run(tauri::generate_context!())
        .expect("impossible de démarrer l'application");
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tauri::Manager;
    use xc_sync::import::import_spec;
    use xc_sync::merge::Choice;

    use super::*;

    fn app() -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        app
    }

    const V1: &str = "openapi: 3.0.0
info: {title: T, version: '1'}
paths:
  /pets:
    get:
      operationId: listPets
      summary: List
      parameters:
        - {name: limit, in: query, required: true, description: max, schema: {type: integer}}
      responses: {'200': {description: ok}}
";

    fn collection(dir: &Path) -> String {
        let root = import_spec(V1, "api.yaml", dir, GroupBy::Tags).unwrap();
        root.display().to_string()
    }

    fn plan_of(root: &str, spec: &str) -> Plan {
        sync::plan(Path::new(root), spec, "api.yaml".into(), &[]).unwrap()
    }

    #[test]
    fn ef_syn_05_a_new_plan_replaces_the_previous_plan_of_the_same_collection_only() {
        let (dir, other) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (root, other_root) = (collection(dir.path()), collection(other.path()));
        let plans = Plans::default();
        let first = plans.insert(&root, plan_of(&root, V1)).unwrap();
        let elsewhere = plans.insert(&other_root, plan_of(&other_root, V1)).unwrap();
        let second = plans.insert(&root, plan_of(&root, V1)).unwrap();
        assert_ne!(first, second);
        assert!(plans.get(&first).unwrap_err().contains("plan introuvable"));
        assert!(plans.get(&second).is_ok() && plans.get(&elsewhere).is_ok());
    }

    #[test]
    fn ef_syn_06_a_plan_is_forgotten_after_a_successful_apply_and_kept_after_a_refusal() {
        let dir = tempfile::tempdir().unwrap();
        let root = collection(dir.path());
        let file = Path::new(&root).join("List.yml");
        let text = fs::read_to_string(&file).unwrap();
        fs::write(&file, text.replace("method: GET", "method: PUT")).unwrap();
        let spec = V1.replace("get:", "post:");
        let plans = Plans::default();
        let plan = plan_of(&root, &spec);
        let decisions = Decisions::uniform(&plan, Choice::Spec);
        let id = plans.insert(&root, plan).unwrap();

        let refused = plans.apply(&id, &Decisions::default()).unwrap_err();
        assert!(refused.contains("conflit(s) sans choix"), "{refused}");
        assert!(plans.get(&id).is_ok(), "l'arbitrage peut continuer");
        assert!(fs::read_to_string(&file).unwrap().contains("method: PUT"));

        let report = plans.apply(&id, &decisions).unwrap();
        assert_eq!(report.written, ["List.yml"]);
        assert!(fs::read_to_string(&file).unwrap().contains("method: POST"));
        assert!(plans.get(&id).unwrap_err().contains("plan introuvable"), "oublié après l'application");
        assert!(plans.apply(&id, &decisions).is_err());
    }

    #[tokio::test]
    async fn ef_col_04_commands_create_a_collection_then_manage_its_items() {
        let app = app();
        let state = || app.state::<AppState>();
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().display().to_string();
        assert_eq!(inspect_folder(parent.clone()).await.unwrap(), FolderKind::Empty);
        let root = create_collection(state(), parent, "Ma Collection".into()).await.unwrap();
        assert_eq!(Path::new(&root), dir.path().join("Ma Collection"));
        assert_eq!(inspect_folder(root.clone()).await.unwrap(), FolderKind::Collection);

        let folder = create_folder(state(), root.clone(), String::new(), "Commandes".into()).await.unwrap();
        let request = create_request(state(), root.clone(), folder, "Lister".into()).await.unwrap();
        assert_eq!(request, "Commandes/Lister.yml");
        let renamed = rename_item(state(), root.clone(), request, "Détails".into()).await.unwrap();
        assert_eq!(renamed, "Commandes/Détails.yml");
        let copy = clone_item(state(), root.clone(), renamed, "Détails copie".into()).await.unwrap();
        let moved = move_item(state(), root.clone(), copy, String::new(), DropPosition::Inside).await.unwrap();
        assert_eq!(moved, "Détails copie.yml");
        let info = xc_core::open_collection(Path::new(&root)).unwrap();
        assert_eq!(info.request_count, 2);
    }

    #[tokio::test]
    async fn ef_var_01_commands_create_edit_rename_and_duplicate_an_environment() {
        let app = app();
        let state = || app.state::<AppState>();
        let dir = tempfile::tempdir().unwrap();
        let root = create_collection(state(), dir.path().display().to_string(), "Démo".into()).await.unwrap();
        assert_eq!(create_environment(state(), root.clone(), "Dev".into()).await.unwrap(), "Dev");

        let vars: Vec<EnvVar> = serde_json::from_str(
            r#"[{"name":"host","value":"http://d","secret":false,"enabled":true,"description":null,"dataType":null},
                {"name":"vide","enabled":false}]"#,
        )
        .unwrap();
        assert!(save_environment(state(), root.clone(), "Dev".into(), vars.clone(), false).await.unwrap());
        assert!(!save_environment(state(), root.clone(), "Dev".into(), vars, false).await.unwrap());
        let read = read_environment(root.clone(), "Dev".into()).unwrap();
        assert_eq!((read.len(), read[1].enabled), (2, false));
        assert_eq!(serde_json::to_value(&read[0]).unwrap()["dataType"], serde_json::Value::Null);

        let renamed = rename_environment(state(), root.clone(), "Dev".into(), "Local".into()).await.unwrap();
        let copy = clone_environment(state(), root.clone(), renamed, "Local copie".into()).await.unwrap();
        set_default_environment(state(), root.clone(), Some(copy)).await.unwrap();
        let info = xc_core::open_collection(Path::new(&root)).unwrap();
        assert_eq!(info.environments, ["Local", "Local copie"]);
        assert_eq!(info.default_environment.as_deref(), Some("Local copie"));
        set_default_environment(state(), root, None).await.unwrap();
    }

    fn serve_json() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let mut stream = stream.unwrap();
                let _ = stream.read(&mut [0u8; 4096]).unwrap();
                let body = r#"{"id":7}"#;
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        base
    }

    fn send_args(root: &str, path: &str, id: &str) -> SendArgs {
        let doc = xc_core::read_request(Path::new(root), path).unwrap();
        SendArgs { id: id.into(), root: root.into(), path: path.into(), doc, env: None }
    }

    #[tokio::test]
    async fn ef_scr_01_send_request_runs_scripts_and_keeps_their_variables_between_sends() {
        let app = app();
        let state = || app.state::<AppState>();
        let base = serve_json();
        let dir = tempfile::tempdir().unwrap();
        let root = create_collection(state(), dir.path().display().to_string(), "Démo".into()).await.unwrap();
        let config = Path::new(&root).join("opencollection.yml");
        let text = fs::read_to_string(&config).unwrap();
        fs::write(
            &config,
            text.replacen("\n", &format!("\n\nrequest:\n  variables:\n    - name: base\n      value: {base}\n\n"), 1),
        )
        .unwrap();
        let script = |name: &str, code: &str| {
            format!("info:\n  name: {name}\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{base}}}}/x\"\n\nruntime:\n  scripts:\n{code}")
        };
        fs::write(
            Path::new(&root).join("login.yml"),
            script("login", "    - type: after-response\n      code: bru.setVar('token', res.body.id);\n    - type: tests\n      code: |-\n        console.log('seen', res.status);\n        test('ok', () => expect(res.status).to.equal(200));\n"),
        )
        .unwrap();
        fs::write(
            Path::new(&root).join("next.yml"),
            script(
                "next",
                "    - type: tests\n      code: test('token kept', () => expect(bru.getVar('token')).to.equal(7));\n",
            ),
        )
        .unwrap();

        let first = send_request(app.handle().clone(), state(), send_args(&root, "login.yml", "a")).await.unwrap();
        assert_eq!(first.response.as_ref().map(|r| r.status), Some(200));
        assert_eq!(first.scripts.tests.results.len(), 1);
        assert_eq!(first.scripts.tests.logs[0].args, serde_json::json!(["seen", 200]));
        let second = send_request(app.handle().clone(), state(), send_args(&root, "next.yml", "b")).await.unwrap();
        assert!(second.error.is_none(), "{:?}", second.error.map(|e| e.message));
        assert_eq!(second.scripts.tests.results[0].status, "pass");
    }

    #[tokio::test]
    async fn ef_imp_01_commands_import_a_postman_collection_then_an_environment() {
        let app = app();
        let state = || app.state::<AppState>();
        let dir = tempfile::tempdir().unwrap();
        let collection = dir.path().join("shop.postman_collection.json");
        fs::write(
            &collection,
            r#"{"info":{"name":"Shop","schema":"https://schema.getpostman.com/json/collection/v2.1.0/collection.json"},"item":[{"name":"List","request":{"method":"GET","url":"https://shop.test/items"}},{"name":"Broken","request":{"url":"x"}}]}"#,
        )
        .unwrap();
        let parent = dir.path().join("out");
        fs::create_dir(&parent).unwrap();

        let imported =
            import_postman(state(), collection.display().to_string(), parent.display().to_string()).await.unwrap();
        assert!(Path::new(&imported.root).join("List.yml").is_file());
        assert_eq!(imported.issues.len(), 1);
        assert_eq!(imported.issues[0].path, "Broken");

        let env = dir.path().join("env.json");
        fs::write(&env, r#"{"name":"Prod","values":[{"key":"base","value":"https://shop.test","enabled":true}]}"#)
            .unwrap();
        let name = import_postman_environment(state(), imported.root.clone(), env.display().to_string()).await.unwrap();
        assert_eq!(name, "Prod");
        assert_eq!(xc_core::read_environment(Path::new(&imported.root), "Prod").unwrap().len(), 1);

        let missing =
            import_postman(state(), dir.path().join("none.json").display().to_string(), parent.display().to_string())
                .await
                .unwrap_err();
        assert!(missing.contains("illisible"), "{missing}");
        let unsupported = dir.path().join("bad.json");
        fs::write(&unsupported, "{}").unwrap();
        let refused =
            import_postman(state(), unsupported.display().to_string(), parent.display().to_string()).await.unwrap_err();
        assert!(refused.contains("v2.0 et v2.1"), "{refused}");
    }

    #[tokio::test]
    async fn ef_col_04_commands_report_errors_as_french_strings() {
        let app = app();
        let state = || app.state::<AppState>();
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().display().to_string();
        let root = create_collection(state(), parent, "Ma Collection".into()).await.unwrap();
        let outside = rename_item(state(), root.clone(), "../x.yml".into(), "a".into()).await.unwrap_err();
        assert!(outside.contains("hors de la collection"), "{outside}");
        let again = init_collection(state(), root.clone(), "Autre".into()).await.unwrap_err();
        assert!(again.contains("contient déjà une collection"), "{again}");
        let reserved = create_folder(state(), root.clone(), String::new(), "environments".into()).await.unwrap_err();
        assert!(reserved.starts_with("nom invalide"), "{reserved}");
        let missing = inspect_folder(dir.path().join("absent").display().to_string()).await.unwrap_err();
        assert!(missing.starts_with("dossier introuvable"), "{missing}");
        let absent = rename_item(state(), root.clone(), "absent.yml".into(), "a".into()).await.unwrap_err();
        assert!(absent.contains("introuvable"), "{absent}");
        let not_a_collection = dir.path().display().to_string();
        let refused = create_request(state(), not_a_collection, String::new(), "x".into()).await.unwrap_err();
        assert!(refused.contains("n'est pas une collection"), "{refused}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn ef_col_01_concurrent_writes_are_serialized_and_keep_the_seq_unique() {
        let app = app();
        let state = || app.state::<AppState>();
        for _ in 0..10 {
            let dir = tempfile::tempdir().unwrap();
            let root = create_collection(state(), dir.path().display().to_string(), "C".into()).await.unwrap();
            for name in ["A", "B", "C", "D", "E", "F"] {
                create_request(state(), root.clone(), String::new(), name.into()).await.unwrap();
            }
            let (first, second, third, fourth) = tokio::join!(
                move_item(state(), root.clone(), "F.yml".into(), "A.yml".into(), DropPosition::Before),
                move_item(state(), root.clone(), "E.yml".into(), "A.yml".into(), DropPosition::Before),
                create_request(state(), root.clone(), String::new(), "G".into()),
                clone_item(state(), root.clone(), "B.yml".into(), "B bis".into()),
            );
            [first, second, third, fourth].into_iter().for_each(|reply| drop(reply.unwrap()));
            let items = xc_core::collection::list_folder(Path::new(&root), "").unwrap().unwrap();
            let mut seqs: Vec<i64> = items.iter().map(|item| item.seq().unwrap()).collect();
            seqs.sort_unstable();
            seqs.dedup();
            assert_eq!(seqs.len(), items.len(), "aucun seq en double : {items:?}");
        }
    }

    #[tokio::test]
    async fn ef_col_03_watch_collection_announces_disk_changes_of_the_open_collection_only() {
        use tauri::Listener;

        let app = app();
        let state = || app.state::<AppState>();
        let dir = tempfile::tempdir().unwrap();
        let root = create_collection(state(), dir.path().display().to_string(), "C".into()).await.unwrap();
        let other = create_collection(state(), dir.path().display().to_string(), "Autre".into()).await.unwrap();
        let (sender, announced) = std::sync::mpsc::channel();
        app.listen("collection-changed", move |event| drop(sender.send(event.payload().to_owned())));

        watch_collection(app.handle().clone(), state(), root.clone()).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        fs::write(Path::new(&other).join("ailleurs.yml"), "x").unwrap();
        fs::write(Path::new(&root).join("Nouvelle.yml"), "x").unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut seen = Vec::new();
        while std::time::Instant::now() < deadline
            && !seen.iter().any(|p: &serde_json::Value| p["paths"].to_string().contains("Nouvelle.yml"))
        {
            if let Ok(payload) = announced.recv_timeout(std::time::Duration::from_millis(250)) {
                seen.push(serde_json::from_str::<serde_json::Value>(&payload).unwrap());
            }
        }
        let change =
            seen.iter().find(|p| p["paths"].to_string().contains("Nouvelle.yml")).unwrap_or_else(|| panic!("{seen:?}"));
        assert_eq!(change["root"], root);
        assert!(seen.iter().all(|p| !p["paths"].to_string().contains("ailleurs.yml")), "{seen:?}");

        let missing = watch_collection(app.handle().clone(), state(), dir.path().join("absent").display().to_string())
            .await
            .unwrap_err();
        assert!(missing.starts_with("dossier introuvable"), "{missing}");
    }

    #[test]
    fn ef_col_04_trash_failures_are_told_in_french() {
        let os = trash::Error::Os { code: 513, description: "The file couldn't be saved: no permission".into() };
        let unknown = trash::Error::Unknown { description: "unknown failure".into() };
        let missing = trash::Error::CouldNotAccess { target: "/x".into() };
        let messages = [
            trash_message(trash::Error::TargetedRoot),
            trash_message(missing),
            trash_message(os),
            trash_message(unknown),
        ];
        assert_eq!(messages[1], "élément introuvable ou inaccessible");
        assert_eq!(messages[2], "la corbeille a refusé l'élément (code 513)");
        assert_eq!(messages[3], "la corbeille a refusé l'élément");
        assert!(messages.iter().all(|message| !message.contains("permission") && !message.contains("unknown")));
    }

    #[test]
    fn ef_col_01_drop_positions_and_folder_kinds_use_the_names_of_the_interface() {
        let positions: Vec<DropPosition> = serde_json::from_str(r#"["before","after","inside"]"#).unwrap();
        assert_eq!(positions, [DropPosition::Before, DropPosition::After, DropPosition::Inside]);
        assert!(serde_json::from_str::<DropPosition>(r#""Before""#).is_err());
        assert_eq!(serde_json::to_string(&FolderKind::Bru).unwrap(), r#""bru""#);
    }
}
