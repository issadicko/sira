use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::State;
use xc_core::assert::{evaluate, AssertionResult, ResponseView};
use xc_core::pretty::pretty_json;
use xc_core::vars::{Context, Scope, VariableInfo};
use xc_core::{CollectionInfo, EnvVar, RequestDoc};
use xc_engine::Timings;

#[derive(Default)]
struct AppState {
    runtime: Mutex<HashMap<String, HashMap<String, String>>>,
    inflight: Mutex<HashMap<String, tokio::task::AbortHandle>>,
}

type Reply<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn root(path: &str) -> PathBuf {
    PathBuf::from(path)
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
    xc_core::open_collection(Path::new(&root)).map_err(err)
}

#[tauri::command]
fn read_request(root: String, path: String) -> Reply<RequestDoc> {
    xc_core::read_request(Path::new(&root), &path).map_err(err)
}

#[tauri::command]
fn save_request(root: String, path: String, doc: RequestDoc) -> Reply<bool> {
    xc_core::save_request(Path::new(&root), &path, &doc).map_err(err)
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
    let prepared =
        xc_core::prepare(&root(&args.root), &args.path, &args.doc, args.env.as_deref(), &runtime).map_err(err)?;
    let request = prepared.request.clone();
    let task = tokio::spawn(async move { xc_engine::send(&request).await });
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
    let body = String::from_utf8_lossy(&res.body).into_owned();
    Ok(SendResult {
        method: prepared.request.method,
        url: prepared.request.url,
        unresolved: prepared.unresolved,
        assertions,
        response: ResponseDto {
            status: res.status,
            reason: res.reason,
            http_version: res.http_version,
            remote_addr: res.remote_addr,
            size: res.body.len(),
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
            cancel_request
        ])
        .run(tauri::generate_context!())
        .expect("impossible de démarrer l'application");
}
