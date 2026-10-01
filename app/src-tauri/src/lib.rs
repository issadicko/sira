use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::State;
use xc_core::assert::{evaluate, AssertionResult, ResponseView};
use xc_core::pretty::pretty_json;
use xc_core::vars::{Context, Scope, VariableInfo};
use xc_core::{CollectionInfo, EnvVar, Prepared, RequestDoc};
use xc_engine::Timings;
use xc_sync::import::{fetch_spec, OpenApiPreview};
use xc_sync::manage::{self, DropPosition, FolderKind};
use xc_sync::openapi::GroupBy;
use xc_sync::sync::{self, Decisions, OpView, Plan, Report, SyncStatus};

#[derive(Default)]
struct AppState {
    runtime: Mutex<HashMap<String, HashMap<String, String>>>,
    inflight: Mutex<HashMap<String, tokio::task::AbortHandle>>,
    plans: Arc<Plans>,
    writes: tokio::sync::Mutex<()>,
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
    response: ResponseDto,
    assertions: Vec<AssertionResult>,
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
fn open_collection(root: String) -> Reply<CollectionInfo> {
    let mut info = xc_core::open_collection(Path::new(&root)).map_err(err)?;
    xc_core::mark_deprecated(&mut info.items, &xc_sync::store::removed_files(Path::new(&root)));
    Ok(info)
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

#[tauri::command]
fn variables(
    state: State<'_, AppState>,
    root: String,
    path: String,
    doc: RequestDoc,
    env: Option<String>,
) -> Reply<Vec<VariableInfo>> {
    let dir = Path::new(&root);
    let runtime = state.runtime.lock().map_err(err)?.get(&root).cloned().unwrap_or_default();
    let ctx = Context::load(dir, &path).map_err(err)?;
    let scope = Scope::build(dir, &ctx, &path, &doc, env.as_deref(), &runtime).map_err(err)?;
    Ok(scope.infos())
}

#[tauri::command]
async fn send_request(state: State<'_, AppState>, args: SendArgs) -> Reply<SendResult> {
    let runtime = state.runtime.lock().map_err(err)?.get(&args.root).cloned().unwrap_or_default();
    let Prepared { request, unresolved } =
        xc_core::prepare(&root(&args.root), &args.path, &args.doc, args.env.as_deref(), &runtime).map_err(err)?;
    let (method, url) = (request.method.clone(), request.url.clone());
    let task = tokio::spawn(async move { xc_engine::send(request).await });
    state.inflight.lock().map_err(err)?.insert(args.id.clone(), task.abort_handle());
    let outcome = task.await;
    state.inflight.lock().map_err(err)?.remove(&args.id);
    let res = match outcome {
        Ok(result) => result.map_err(err)?,
        Err(e) if e.is_cancelled() => return Err("Requête annulée".into()),
        Err(e) => return Err(err(e)),
    };
    let assertions =
        evaluate(&args.doc.assertions, &ResponseView { status: res.status, headers: &res.headers, body: &res.body });
    let size = res.body.len();
    let body = xc_engine::lossy_text(res.body);
    Ok(SendResult {
        method,
        url,
        unresolved,
        assertions,
        response: ResponseDto {
            status: res.status,
            reason: res.reason,
            http_version: res.http_version,
            remote_addr: res.remote_addr,
            size,
            pretty: pretty_json(&body),
            body,
            headers: res.headers,
            timings: res.timings,
        },
    })
}

#[tauri::command]
fn cancel_request(state: State<'_, AppState>, id: String) -> Reply<bool> {
    Ok(state.inflight.lock().map_err(err)?.remove(&id).map(|h| h.abort()).is_some())
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
            read_request,
            save_request,
            read_environment,
            variables,
            send_request,
            cancel_request,
            parse_curl,
            create_request_from_curl,
            preview_openapi,
            import_openapi,
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
            sync_apply
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
