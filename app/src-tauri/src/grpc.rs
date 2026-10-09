//! Les appels gRPC de l'application : l'interface ouvre, envoie, termine et annule par identifiant, et reçoit ce qui arrive
//! par l'événement `grpc-event`. L'appel lui-même (schéma, réseau, secrets) est celui de `xc-runner::open_grpc`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use xc_core::RequestDoc;
use xc_engine::{GrpcEvent, GrpcSender};
use xc_proto::{MethodInfo, Schema};
use xc_runner::{
    decode_message, describe_grpc, encode_message, now_iso, open_grpc, resolve_message, status_name, GrpcStart,
};

use crate::{err, loaded_session, AppState, Reply};

struct Call {
    sender: GrpcSender,
    schema: Schema,
    method: MethodInfo,
}

/// Les appels ouverts, par identifiant : de quoi leur envoyer des messages, terminer l'envoi ou les annuler.
#[derive(Default)]
pub struct Calls(Mutex<HashMap<String, Call>>);

impl Calls {
    fn sender(&self, id: &str) -> Reply<GrpcSender> {
        self.0
            .lock()
            .map_err(err)?
            .get(id)
            .map(|call| call.sender.clone())
            .ok_or_else(|| "l'appel est terminé".to_owned())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectArgs {
    pub id: String,
    pub root: String,
    pub path: String,
    pub doc: RequestDoc,
    pub env: Option<String>,
}

/// Un message parti, variables résolues.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SentMessage {
    pub description: String,
    pub data: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub url: String,
    pub remote_addr: String,
    pub connect_ms: f64,
    pub schema_source: String,
    pub method: MethodInfo,
    /// Les messages du fichier déjà partis à l'ouverture.
    pub sent: Vec<SentMessage>,
    /// Vrai quand l'envoi est terminé d'office (appel unaire ou flux serveur) ; sinon l'interface propose de le terminer.
    pub finished: bool,
    pub unresolved: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Notice {
    id: String,
    at: String,
    #[serde(flatten)]
    event: Incoming,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Incoming {
    Headers { headers: Vec<(String, String)> },
    Message { data: String, size: usize },
    Status { code: u32, name: String, message: String, metadata: Vec<(String, String)> },
    Error { message: String },
}

fn incoming(event: GrpcEvent, schema: &Schema, method: &MethodInfo) -> Incoming {
    match event {
        GrpcEvent::Headers { headers } => Incoming::Headers { headers },
        GrpcEvent::Message { data } => {
            Incoming::Message { size: data.len(), data: decode_message(schema, method, &data) }
        }
        GrpcEvent::Status { code, message, metadata } => {
            Incoming::Status { code, name: status_name(code).to_owned(), message, metadata }
        }
        GrpcEvent::Error { message } => Incoming::Error { message },
    }
}

/// Ouvre l'appel `id` et envoie les messages du fichier : un seul puis la fin de l'envoi pour un appel unaire ou un flux
/// serveur, tous pour un flux client ou bidirectionnel (l'interface termine l'envoi). Chaque événement qui arrive est
/// relayé ; l'appel est oublié après son statut ou une erreur.
#[tauri::command]
pub async fn grpc_connect<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    args: ConnectArgs,
) -> Reply<Connected> {
    let ConnectArgs { id, root, path, doc, env } = args;
    if state.calls.0.lock().map_err(err)?.contains_key(&id) {
        return Err("cet appel est déjà ouvert".into());
    }
    let session = loaded_session(&state, &root, env.as_deref()).await?;
    let GrpcStart { call, schema, method, schema_source, messages, unresolved } =
        open_grpc(Path::new(&root), &path, &doc, env.as_deref(), &session).await?;

    let mut outgoing: Vec<SentMessage> =
        messages.into_iter().map(|m| SentMessage { description: m.title, data: m.data }).collect();
    if outgoing.is_empty() && !method.client_streaming {
        outgoing.push(SentMessage { description: String::new(), data: "{}".into() });
    }
    if !method.client_streaming {
        outgoing.truncate(1);
    }
    let mut encoded = Vec::new();
    for message in &outgoing {
        match encode_message(&schema, &method, &message.data) {
            Ok(bytes) => encoded.push(bytes),
            Err(error) => {
                call.sender.cancel();
                return Err(error);
            }
        }
    }
    for bytes in encoded {
        call.sender.send(bytes).map_err(err)?;
    }
    let finished = !method.client_streaming;
    if finished {
        call.sender.finish().map_err(err)?;
    }

    let opened = call.opened.clone();
    state
        .calls
        .0
        .lock()
        .map_err(err)?
        .insert(id.clone(), Call { sender: call.sender.clone(), schema: schema.clone(), method: method.clone() });
    let mut events = call.events;
    let (relay_schema, relay_method) = (schema, method.clone());
    let app_events = app.clone();
    let call_id = id.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            let last = matches!(event, GrpcEvent::Status { .. } | GrpcEvent::Error { .. });
            let event = incoming(event, &relay_schema, &relay_method);
            app_events.emit("grpc-event", Notice { id: call_id.clone(), at: now_iso(), event }).ok();
            if last {
                break;
            }
        }
        if let Ok(mut calls) = app_events.state::<AppState>().calls.0.lock() {
            calls.remove(&call_id);
        }
    });
    Ok(Connected {
        url: opened.url,
        remote_addr: opened.remote_addr,
        connect_ms: opened.timings.total_ms,
        schema_source,
        method,
        sent: outgoing,
        finished,
        unresolved,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendArgs {
    pub id: String,
    pub root: String,
    pub path: String,
    pub doc: RequestDoc,
    pub env: Option<String>,
    pub data: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sent {
    pub data: String,
    pub unresolved: Vec<String>,
}

/// Envoie le message JSON `data` (variables résolues) sur l'appel `id`, s'il accepte d'autres messages.
#[tauri::command]
pub async fn grpc_send(state: State<'_, AppState>, args: SendArgs) -> Reply<Sent> {
    let SendArgs { id, root, path, doc, env, data } = args;
    let (sender, schema, method) = {
        let calls = state.calls.0.lock().map_err(err)?;
        let call = calls.get(&id).ok_or_else(|| "l'appel est terminé".to_owned())?;
        (call.sender.clone(), call.schema.clone(), call.method.clone())
    };
    if !method.client_streaming {
        return Err(format!(
            "{} est un appel {} : un seul message part à l'ouverture",
            method.full_name,
            method.kind()
        ));
    }
    let session = loaded_session(&state, &root, env.as_deref()).await?;
    let (data, unresolved) = resolve_message(Path::new(&root), &path, &doc, env.as_deref(), &session, &data)?;
    let bytes = encode_message(&schema, &method, &data)?;
    sender.send(bytes).map_err(err)?;
    Ok(Sent { data, unresolved })
}

/// Annonce qu'aucun autre message ne partira (demi-fermeture) ; la réponse et le statut suivent.
#[tauri::command]
pub fn grpc_finish(state: State<'_, AppState>, id: String) -> Reply<()> {
    state.calls.sender(&id)?.finish().map_err(err)
}

/// Annule l'appel `id` ; sans effet s'il est déjà terminé.
#[tauri::command]
pub fn grpc_cancel(state: State<'_, AppState>, id: String) -> Reply<()> {
    if let Ok(sender) = state.calls.sender(&id) {
        sender.cancel();
    }
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodsArgs {
    pub root: String,
    pub path: String,
    pub doc: RequestDoc,
    pub env: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MethodView {
    #[serde(flatten)]
    pub info: MethodInfo,
    /// Un message JSON complet à valeurs par défaut, point de départ d'une requête.
    pub skeleton: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Methods {
    pub source: String,
    pub methods: Vec<MethodView>,
}

/// Les méthodes que la requête peut appeler (fichiers `.proto`, sinon réflexion du serveur), avec un message d'exemple.
#[tauri::command]
pub async fn grpc_methods(state: State<'_, AppState>, args: MethodsArgs) -> Reply<Methods> {
    let MethodsArgs { root, path, doc, env } = args;
    let session = loaded_session(&state, &root, env.as_deref()).await?;
    let (schema, source) = describe_grpc(Path::new(&root), &path, &doc, env.as_deref(), &session).await?;
    let methods = schema
        .methods()
        .into_iter()
        .map(|info| MethodView { skeleton: schema.skeleton(&info.full_name).unwrap_or_default(), info })
        .collect();
    Ok(Methods { source, methods })
}

#[cfg(test)]
#[path = "../../../crates/runner/tests/support/grpc_server.rs"]
mod grpc_server;

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::sync::mpsc;
    use std::time::Duration;

    use tauri::{Listener, Manager};

    use super::*;

    fn copy_dir(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                fs::copy(entry.path(), target).unwrap();
            }
        }
    }

    fn collection(address: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        copy_dir(&grpc_server::proto_dir(), &root.join("proto"));
        fs::write(
            root.join("opencollection.yml"),
            "opencollection: 1.0.0\n\ninfo:\n  name: Grpc\n\nconfig:\n  protobuf:\n    importPaths:\n      - path: proto\n",
        )
        .unwrap();
        fs::create_dir(root.join("environments")).unwrap();
        fs::write(
            root.join("environments/dev.yml"),
            format!("name: dev\nvariables:\n  - name: addr\n    value: {address}\n  - name: user\n    value: Ada\n"),
        )
        .unwrap();
        let request = |method: &str, kind: &str, message: &str| {
            format!(
                "info:\n  name: R\n  type: grpc\n  seq: 1\n\ngrpc:\n  url: \"{{{{addr}}}}\"\n  method: {method}\n  methodType: {kind}\n  protoFilePath: proto/demo.proto\n{message}"
            )
        };
        fs::write(
            root.join("echo.yml"),
            request("demo.Demo/Echo", "unary", "  message: '{\"name\": \"{{user}}\", \"userId\": 3}'\n"),
        )
        .unwrap();
        fs::write(
            root.join("chat.yml"),
            request("demo.Demo/Chat", "bidi-streaming", "  message: '{\"name\": \"un\"}'\n"),
        )
        .unwrap();
        dir
    }

    fn wait_for(events: &mpsc::Receiver<serde_json::Value>, kind: &str) -> serde_json::Value {
        loop {
            let event = events.recv_timeout(Duration::from_secs(5)).expect("un événement grpc-event");
            if event["kind"] == kind {
                return event;
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ef_grpc_01_the_commands_open_a_unary_call_relay_its_reply_and_forget_it() {
        let (address, _) = grpc_server::serve(false).await;
        let dir = collection(&address);
        let root = dir.path().display().to_string();
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = || app.state::<AppState>();
        let (tx, events) = mpsc::channel();
        app.listen("grpc-event", move |event| {
            tx.send(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap()).ok();
        });
        let doc = xc_core::read_request(dir.path(), "echo.yml").unwrap();
        let args = || ConnectArgs {
            id: "g1".into(),
            root: root.clone(),
            path: "echo.yml".into(),
            doc: doc.clone(),
            env: Some("dev".into()),
        };

        let connected = grpc_connect(app.handle().clone(), state(), args()).await.unwrap();

        assert_eq!(connected.method.full_name, "demo.Demo/Echo");
        assert_eq!(connected.schema_source, "proto/demo.proto");
        assert!(connected.finished);
        assert_eq!(connected.sent.len(), 1);
        assert_eq!(connected.sent[0].data, r#"{"name": "Ada", "userId": 3}"#);
        let reply = wait_for(&events, "message");
        assert!(reply["data"].as_str().unwrap().contains("salut Ada"), "{reply}");
        assert!(reply["at"].as_str().is_some_and(|at| at.ends_with('Z')), "{reply}");
        let status = wait_for(&events, "status");
        assert_eq!((status["code"].as_u64(), status["name"].as_str()), (Some(0), Some("OK")));
        for _ in 0..50 {
            if state().calls.0.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(state().calls.0.lock().unwrap().is_empty(), "l'appel terminé est oublié");
        grpc_cancel(state(), "g1".into()).unwrap();
        assert!(grpc_finish(state(), "g1".into()).unwrap_err().contains("terminé"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ef_grpc_01_a_bidirectional_call_takes_more_messages_until_the_send_is_finished() {
        let (address, _) = grpc_server::serve(false).await;
        let dir = collection(&address);
        let root = dir.path().display().to_string();
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = || app.state::<AppState>();
        let (tx, events) = mpsc::channel();
        app.listen("grpc-event", move |event| {
            tx.send(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap()).ok();
        });
        let doc = xc_core::read_request(dir.path(), "chat.yml").unwrap();
        let args = ConnectArgs {
            id: "g2".into(),
            root: root.clone(),
            path: "chat.yml".into(),
            doc: doc.clone(),
            env: Some("dev".into()),
        };

        let connected = grpc_connect(app.handle().clone(), state(), args).await.unwrap();
        assert!(!connected.finished);
        assert!(wait_for(&events, "message")["data"].as_str().unwrap().contains("re:un"));

        let sent = grpc_send(
            state(),
            SendArgs {
                id: "g2".into(),
                root: root.clone(),
                path: "chat.yml".into(),
                doc: doc.clone(),
                env: Some("dev".into()),
                data: r#"{"name": "{{user}}"}"#.into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(sent.data, r#"{"name": "Ada"}"#);
        assert!(wait_for(&events, "message")["data"].as_str().unwrap().contains("re:Ada"));

        let bad = grpc_send(
            state(),
            SendArgs { id: "g2".into(), root, path: "chat.yml".into(), doc, env: Some("dev".into()), data: "{".into() },
        )
        .await
        .unwrap_err();
        assert!(bad.contains("demo.EchoRequest"), "{bad}");

        grpc_finish(state(), "g2".into()).unwrap();
        assert_eq!(wait_for(&events, "status")["name"], "OK");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ef_grpc_01_the_methods_of_a_request_come_with_an_example_message() {
        let (address, _) = grpc_server::serve(false).await;
        let dir = collection(&address);
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let doc = xc_core::read_request(dir.path(), "echo.yml").unwrap();

        let methods = grpc_methods(
            app.state::<AppState>(),
            MethodsArgs {
                root: dir.path().display().to_string(),
                path: "echo.yml".into(),
                doc,
                env: Some("dev".into()),
            },
        )
        .await
        .unwrap();

        assert_eq!(methods.source, "proto/demo.proto");
        let names: Vec<&str> = methods.methods.iter().map(|m| m.info.full_name.as_str()).collect();
        assert_eq!(names, ["demo.Demo/Echo", "demo.Demo/Watch", "demo.Demo/Upload", "demo.Demo/Chat"]);
        let skeleton: serde_json::Value = serde_json::from_str(&methods.methods[0].skeleton).unwrap();
        assert_eq!(skeleton["userId"], 0);
    }
}
