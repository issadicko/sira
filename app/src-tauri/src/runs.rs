//! Commandes du runner : lancer une collection ou un dossier, suivre le run en direct, l'annuler et exporter son rapport.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, State};
use xc_runner::{
    html_page, json, junit, now_iso, read_rows, run_collection, select, AssertionResult, Event, Halt, Job, Meta,
    Redact, RequestResult, Row, RunError, RunReport, Skip, Stage, Summary, MAX_JUMPS,
};

use crate::{blocking, err, AppState, Reply, ScriptsDto};

/// Les runs en cours (leur drapeau d'annulation) et le rapport du dernier, gardé pour l'export.
#[derive(Default)]
pub struct Runs {
    live: Mutex<std::collections::HashMap<String, Arc<AtomicBool>>>,
    last: Mutex<Option<(String, Arc<RunReport>)>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArgs {
    run_id: String,
    root: String,
    /// Chemins relatifs de requêtes ou de dossiers ; toute la collection si vide.
    targets: Vec<String>,
    env: Option<String>,
    bail: bool,
    delay_ms: u64,
    /// Fichier CSV ou JSON : une itération par ligne.
    data: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HttpDto {
    status: u16,
    reason: String,
    size: usize,
    time_ms: f64,
}

/// Une requête du run, comme l'interface l'affiche : statut, réponse, résultats des scripts et des assertions.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunResultDto {
    iteration: usize,
    name: String,
    path: String,
    method: String,
    url: String,
    /// `pass`, `fail` (un test, une assertion ou un script a échoué), `error` (pas de réponse) ou `skipped`.
    status: &'static str,
    /// Pourquoi elle n'a pas tourné : `script`, `bail`, `stopExecution`, `prompts` (variables à saisir, que le run ne sait pas demander) ou `unreadable` (le motif est dans `error`).
    skipped: Option<String>,
    http: Option<HttpDto>,
    error: Option<RunError>,
    assertions: Vec<AssertionResult>,
    scripts: ScriptsDto,
    duration_ms: f64,
}

impl RunResultDto {
    fn of(result: &RequestResult) -> Self {
        let skipped = result.skip.as_ref().map(|skip| match skip {
            Skip::Script => "script",
            Skip::Bail => "bail",
            Skip::StopExecution => "stopExecution",
            Skip::Unreadable(_) => "unreadable",
            Skip::Prompts(_) => "prompts",
        });
        let o = &result.outcome;
        let error = match &result.skip {
            Some(Skip::Unreadable(why)) => Some(RunError { stage: Stage::Prepare, message: why.clone() }),
            _ => o.error.clone(),
        };
        let status = if skipped.is_some() {
            "skipped"
        } else if o.error.is_some() {
            "error"
        } else if o.passed() {
            "pass"
        } else {
            "fail"
        };
        Self {
            iteration: result.iteration,
            name: result.name.clone(),
            path: result.path.clone(),
            method: o.method.clone(),
            url: o.url.clone(),
            status,
            skipped: skipped.map(str::to_owned),
            http: o.response.as_ref().map(|r| HttpDto {
                status: r.status,
                reason: r.reason.clone(),
                size: r.body.len(),
                time_ms: r.timings.total_ms,
            }),
            error,
            assertions: o.assertions.clone(),
            scripts: ScriptsDto { pre: o.pre.clone(), post: o.post.clone(), tests: o.tests.clone() },
            duration_ms: result.duration.as_secs_f64() * 1000.0,
        }
    }
}

/// Ce que le run annonce à l'interface, sous l'événement `run-event`.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
enum RunEvent {
    Begin { run_id: String, requests: usize, iterations: usize },
    Iteration { run_id: String, index: usize, total: usize, row: Option<Row> },
    Started { run_id: String, iteration: usize, path: String, name: String, method: String },
    Finished { run_id: String, result: Box<RunResultDto> },
    Waiting { run_id: String, ms: u64 },
    Warning { run_id: String, message: String },
}

/// La fin d'un run : de quoi afficher le résumé, avec le motif d'un arrêt éventuel.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunDone {
    run_id: String,
    summary: Summary,
    halt: Option<Halt>,
    failed: bool,
    elapsed_ms: f64,
}

impl Runs {
    fn register(&self, id: &str) -> Reply<Arc<AtomicBool>> {
        let mut live = self.live.lock().map_err(err)?;
        if live.contains_key(id) {
            return Err("un run porte déjà cet identifiant".into());
        }
        let flag = Arc::new(AtomicBool::new(false));
        live.insert(id.to_owned(), Arc::clone(&flag));
        Ok(flag)
    }

    fn finish(&self, id: &str, report: RunReport) -> Reply<RunDone> {
        self.live.lock().map_err(err)?.remove(id);
        let done = RunDone {
            run_id: id.to_owned(),
            summary: report.summary(),
            halt: report.halt.clone(),
            failed: report.failed(),
            elapsed_ms: report.elapsed.as_secs_f64() * 1000.0,
        };
        *self.last.lock().map_err(err)? = Some((id.to_owned(), Arc::new(report)));
        Ok(done)
    }

    fn report(&self, id: &str) -> Reply<Arc<RunReport>> {
        let last = self.last.lock().map_err(err)?;
        match last.as_ref() {
            Some((kept, report)) if kept == id => Ok(Arc::clone(report)),
            _ => Err("rapport introuvable : seul le dernier run est conservé".into()),
        }
    }
}

fn emit<R: tauri::Runtime>(app: &tauri::AppHandle<R>, event: RunEvent) {
    if let Ok(payload) = serde_json::to_value(event) {
        app.emit("run-event", payload).ok();
    }
}

/// Exécute la collection ou les dossiers demandés en annonçant chaque requête ; rend le résumé à la fin. Les variables que
/// les scripts écrivent rejoignent celles de la collection, comme après un envoi.
#[tauri::command]
pub async fn start_run<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    args: RunArgs,
) -> Reply<RunDone> {
    let RunArgs { run_id, root, targets, env, bail, delay_ms, data } = args;
    let (dir, picked) = (Path::new(&root).to_path_buf(), data.clone());
    let prepared = blocking(move || {
        let collection = xc_core::open_collection(&dir).map_err(err)?;
        let items = select(&collection.items, &targets).map_err(err)?;
        let rows = match picked {
            Some(file) => read_rows(Path::new(&file)).map_err(err)?,
            None => Vec::new(),
        };
        Ok::<_, String>((collection.name, items, rows))
    })
    .await?;
    let (name, items, rows) = prepared;
    let cancel = state.runs.register(&run_id)?;
    let mut session = state.sessions.lock().map_err(err)?.get(&root).cloned().unwrap_or_default();
    session.authorizer = Some(crate::oauth::authorizer(&app));
    session.network = crate::network::current(&state)?;
    session.cookies = state.cookies.clone();
    let (store, dir, picked) = (Arc::clone(&state.secrets.0), root.clone(), env.clone());
    session.secrets = blocking(move || Ok::<_, String>(crate::secrets::load(&*store, &dir, picked.as_deref()))).await?;
    emit(&app, RunEvent::Begin { run_id: run_id.clone(), requests: items.len(), iterations: rows.len().max(1) });

    let dir = Path::new(&root);
    let job = Job {
        root: dir,
        collection_name: &name,
        env: env.as_deref(),
        items: &items,
        rows: &rows,
        bail,
        delay: Duration::from_millis(delay_ms),
        max_jumps: MAX_JUMPS,
        execution_mode: "runner",
        cancel,
    };
    let (events, id) = (app.clone(), run_id.clone());
    let report = run_collection(job, &mut session, &mut move |event| match event {
        Event::Iteration { index, total, row } => {
            emit(&events, RunEvent::Iteration { run_id: id.clone(), index, total, row: row.cloned() });
        }
        Event::Started { iteration, item, .. } => emit(
            &events,
            RunEvent::Started {
                run_id: id.clone(),
                iteration,
                path: item.path.clone(),
                name: item.name.clone(),
                method: item.method.clone(),
            },
        ),
        Event::Finished(result) => {
            let result = Box::new(RunResultDto::of(result));
            emit(&events, RunEvent::Finished { run_id: id.clone(), result });
        }
        Event::Waiting(delay) => emit(&events, RunEvent::Waiting { run_id: id.clone(), ms: delay.as_millis() as u64 }),
        Event::Warning(message) => emit(&events, RunEvent::Warning { run_id: id.clone(), message }),
    })
    .await;

    state.sessions.lock().map_err(err)?.insert(root, session);
    state.runs.finish(&run_id, report)
}

#[tauri::command]
pub fn cancel_run(state: State<'_, AppState>, run_id: String) -> Reply<bool> {
    let live = state.runs.live.lock().map_err(err)?;
    Ok(live.get(&run_id).map(|flag| flag.store(true, Ordering::Relaxed)).is_some())
}

/// Nombre de lignes et colonnes d'un fichier de données, pour que l'interface annonce ce qu'il va piloter.
#[derive(Serialize)]
pub struct DataInfo {
    rows: usize,
    columns: Vec<String>,
}

#[tauri::command]
pub async fn inspect_run_data(path: String) -> Reply<DataInfo> {
    blocking(move || {
        let rows = read_rows(Path::new(&path)).map_err(err)?;
        let columns = rows.first().map(|row| row.keys().cloned().collect()).unwrap_or_default();
        Ok::<_, String>(DataInfo { rows: rows.len(), columns })
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportArgs {
    run_id: String,
    root: String,
    format: String,
    path: String,
    /// Retire les en-têtes et les corps du rapport.
    skip_headers: bool,
    skip_bodies: bool,
}

/// Écrit le rapport du run `run_id` (JSON, JUnit ou HTML) dans `path`.
#[tauri::command]
pub async fn export_run(state: State<'_, AppState>, args: ExportArgs) -> Reply<()> {
    let report = state.runs.report(&args.run_id)?;
    blocking(move || {
        let redact = Redact {
            all_headers: args.skip_headers,
            headers: Vec::new(),
            request_body: args.skip_bodies,
            response_body: args.skip_bodies,
        };
        let text = match args.format.as_str() {
            "json" => json(&report, &redact),
            "junit" => junit(&report, &redact),
            "html" => {
                let collection = xc_core::collection_name(Path::new(&args.root)).unwrap_or_default();
                html_page(&report, &redact, &Meta { collection, completed_at: now_iso() })
            }
            other => return Err(format!("format de rapport inconnu : {other}")),
        };
        std::fs::write(&args.path, text).map_err(|e| format!("écriture de {} impossible : {e}", args.path))
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread;

    use serde_json::Value;
    use tauri::{Listener, Manager};

    use super::*;

    fn serve() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let body = r#"{"ok":true}"#;
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        base
    }

    fn collection(base: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let write = |file: &str, text: String| fs::write(dir.path().join(file), text).unwrap();
        write(
            "opencollection.yml",
            format!("opencollection: 1.0.0\n\ninfo:\n  name: Shop\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n"),
        );
        let request = |name: &str, seq: u32, test: &str| {
            format!("info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/{{{{id}}}}/{name}\"\n\nruntime:\n  scripts:\n    - type: after-response\n      code: bru.setVar('last', '{name}');\n    - type: tests\n      code: {test}\n")
        };
        write("a.yml", request("a", 1, "test('ok', () => expect(res.status).to.equal(200));"));
        write("b.yml", request("b", 2, "test('boom', () => expect(1).to.equal(2));"));
        dir
    }

    fn args(root: &Path, run_id: &str) -> RunArgs {
        RunArgs {
            run_id: run_id.into(),
            root: root.display().to_string(),
            targets: Vec::new(),
            env: None,
            bail: false,
            delay_ms: 0,
            data: None,
        }
    }

    fn app() -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        app
    }

    fn events(app: &tauri::App<tauri::test::MockRuntime>) -> Arc<Mutex<Vec<Value>>> {
        let seen: Arc<Mutex<Vec<Value>>> = Arc::default();
        let log = Arc::clone(&seen);
        app.listen("run-event", move |event| {
            log.lock().unwrap().push(serde_json::from_str(event.payload()).unwrap());
        });
        seen
    }

    #[tokio::test]
    async fn ef_run_01_start_run_announces_each_request_then_keeps_the_report_for_export() {
        let dir = collection(&serve());
        let app = app();
        let seen = events(&app);
        let mut run = args(dir.path(), "run-1");
        let rows = dir.path().join("rows.csv");
        fs::write(&rows, "id\n7\n8\n").unwrap();
        run.data = Some(rows.display().to_string());
        let done = start_run(app.handle().clone(), app.state::<AppState>(), run).await.unwrap();

        assert_eq!(
            (done.summary.total_requests, done.summary.passed_requests, done.summary.failed_requests),
            (4, 2, 2)
        );
        assert!(done.failed && done.halt.is_none());
        {
            let seen = seen.lock().unwrap();
            let kinds: Vec<&str> = seen.iter().map(|e| e["kind"].as_str().unwrap()).collect();
            assert_eq!(kinds[0], "begin");
            assert_eq!(kinds.iter().filter(|k| **k == "iteration").count(), 2);
            assert_eq!(kinds.iter().filter(|k| **k == "started").count(), 4);
            let results: Vec<&Value> = seen.iter().filter(|e| e["kind"] == "finished").map(|e| &e["result"]).collect();
            assert_eq!(
                results.iter().map(|r| r["status"].as_str().unwrap()).collect::<Vec<_>>(),
                ["pass", "fail", "pass", "fail"]
            );
            assert_eq!(results[1]["scripts"]["tests"]["results"][0]["description"], "boom");
            assert_eq!(results[2]["iteration"], 1);
            assert_eq!(results[0]["http"]["status"], 200);
            assert!(results[2]["url"].as_str().unwrap().ends_with("/8/a"), "{}", results[2]["url"]);
        }

        let state = app.state::<AppState>();
        let session = state.sessions.lock().unwrap().get(&dir.path().display().to_string()).cloned().unwrap();
        assert_eq!(session.runtime["last"], "b", "les variables du run rejoignent celles de la collection");

        for (format, marker) in [("json", "\"totalRequests\""), ("junit", "<testsuites>"), ("html", "<!doctype html>")]
        {
            let out = dir.path().join(format!("report.{format}"));
            let export = ExportArgs {
                run_id: "run-1".into(),
                root: dir.path().display().to_string(),
                format: format.into(),
                path: out.display().to_string(),
                skip_headers: true,
                skip_bodies: true,
            };
            export_run(app.state::<AppState>(), export).await.unwrap();
            assert!(fs::read_to_string(out).unwrap().contains(marker), "{format}");
        }
        let unknown = ExportArgs {
            run_id: "ancien".into(),
            root: String::new(),
            format: "json".into(),
            path: String::new(),
            skip_headers: false,
            skip_bodies: false,
        };
        assert!(export_run(app.state::<AppState>(), unknown).await.unwrap_err().contains("rapport introuvable"));
    }

    #[tokio::test]
    async fn ef_run_01_start_run_reports_bad_input_as_french_strings() {
        let dir = collection(&serve());
        let app = app();
        let mut run = args(dir.path(), "run-2");
        run.targets = vec!["fantôme".into()];
        let refused = start_run(app.handle().clone(), app.state::<AppState>(), run).await.unwrap_err();
        assert!(refused.contains("aucune requête ni aucun dossier à fantôme"), "{refused}");

        let mut run = args(dir.path(), "run-3");
        run.data = Some(dir.path().join("absent.csv").display().to_string());
        let refused = start_run(app.handle().clone(), app.state::<AppState>(), run).await.unwrap_err();
        assert!(refused.contains("lecture de"), "{refused}");
        assert!(!cancel_run(app.state::<AppState>(), "run-3".into()).unwrap(), "rien à annuler");
    }

    #[tokio::test]
    async fn ef_run_01_bail_halts_the_run_and_cancel_run_raises_the_flag_of_a_live_one() {
        let dir = collection(&serve());
        let app = app();
        let mut run = args(dir.path(), "run-4");
        run.bail = true;
        let done = start_run(app.handle().clone(), app.state::<AppState>(), run).await.unwrap();
        assert_eq!(done.summary.skipped_requests, 0, "b est la dernière : rien à ignorer");
        assert!(matches!(done.halt, Some(Halt::Bail { .. })));

        let state = app.state::<AppState>();
        let flag = state.runs.register("run-5").unwrap();
        assert!(state.runs.register("run-5").is_err(), "un identifiant ne sert qu'une fois");
        assert!(cancel_run(app.state::<AppState>(), "run-5".into()).unwrap());
        assert!(flag.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn ef_run_02_inspect_run_data_counts_rows_and_names_columns() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("rows.json");
        fs::write(&file, r#"[{"id": 1, "name": "ada"}, {"id": 2}]"#).unwrap();
        let info = inspect_run_data(file.display().to_string()).await.unwrap();
        assert_eq!((info.rows, info.columns), (2, vec!["id".to_owned(), "name".to_owned()]));
        assert!(inspect_run_data(dir.path().join("x.txt").display().to_string()).await.is_err());
    }
}
