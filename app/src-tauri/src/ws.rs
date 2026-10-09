//! Les connexions WebSocket de l'application : l'interface ouvre, envoie et ferme par identifiant, et reçoit ce qui arrive
//! par l'événement `ws-event`. La connexion elle-même (réseau, cookies, secrets) est celle de `xc-runner::open_websocket`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Manager, State};
use xc_core::RequestDoc;
use xc_engine::{WsEvent, WsSender};
use xc_runner::{now_iso, open_websocket, resolve_message, WsStart};

use crate::{err, loaded_session, AppState, Reply};

/// Les connexions ouvertes, par identifiant : de quoi leur envoyer des messages ou les fermer.
#[derive(Default)]
pub struct Sockets(Mutex<HashMap<String, WsSender>>);

impl Sockets {
    fn get(&self, id: &str) -> Reply<WsSender> {
        self.0.lock().map_err(err)?.get(id).cloned().ok_or_else(|| "la connexion est fermée".to_owned())
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

/// Ce que l'interface sait d'une connexion ouverte.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Connected {
    pub status: u16,
    pub protocol: Option<String>,
    pub url: String,
    pub remote_addr: String,
    pub headers: Vec<(String, String)>,
    pub connect_ms: f64,
    pub unresolved: Vec<String>,
}

/// Un octet sur deux caractères hexadécimaux au plus [`HEX_BYTES`] : un gros message binaire n'est pas envoyé en entier à
/// l'interface, qui n'en affiche qu'un aperçu.
const HEX_BYTES: usize = 4096;

fn hex(data: &[u8]) -> String {
    data.iter().take(HEX_BYTES).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

/// Ce que l'interface reçoit sur `ws-event`.
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
    Text { data: String },
    Binary { size: usize, hex: String },
    Ping { size: usize },
    Pong { size: usize },
    Close { code: Option<u16>, reason: String },
    Error { message: String },
}

impl From<WsEvent> for Incoming {
    fn from(event: WsEvent) -> Self {
        match event {
            WsEvent::Text { data } => Self::Text { data },
            WsEvent::Binary { data } => Self::Binary { size: data.len(), hex: hex(&data) },
            WsEvent::Ping { data } => Self::Ping { size: data.len() },
            WsEvent::Pong { data } => Self::Pong { size: data.len() },
            WsEvent::Close { code, reason } => Self::Close { code, reason },
            WsEvent::Error { message } => Self::Error { message },
        }
    }
}

/// Ouvre la connexion `id` et relaie chaque événement qui arrive ; la connexion est oubliée après `close` ou `error`.
#[tauri::command]
pub async fn ws_connect<R: tauri::Runtime>(
    app: tauri::AppHandle<R>,
    state: State<'_, AppState>,
    args: ConnectArgs,
) -> Reply<Connected> {
    let ConnectArgs { id, root, path, doc, env } = args;
    if state.sockets.0.lock().map_err(err)?.contains_key(&id) {
        return Err("cette connexion est déjà ouverte".into());
    }
    let session = loaded_session(&state, &root, env.as_deref()).await?;
    let WsStart { connection, unresolved, .. } =
        open_websocket(Path::new(&root), &path, &doc, env.as_deref(), &session).await?;
    let opened = connection.opened.clone();
    state.sockets.0.lock().map_err(err)?.insert(id.clone(), connection.sender.clone());

    let mut events = connection.events;
    let app_events = app.clone();
    let socket = id.clone();
    tokio::spawn(async move {
        while let Some(event) = events.recv().await {
            app_events.emit("ws-event", Notice { id: socket.clone(), at: now_iso(), event: event.into() }).ok();
        }
        if let Ok(mut sockets) = app_events.state::<AppState>().sockets.0.lock() {
            sockets.remove(&socket);
        }
    });
    Ok(Connected {
        status: opened.status,
        protocol: opened.protocol,
        url: opened.url,
        remote_addr: opened.remote_addr,
        headers: opened.headers,
        connect_ms: opened.timings.total_ms,
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
    /// Le texte parti, variables résolues.
    pub data: String,
    pub unresolved: Vec<String>,
}

/// Envoie `data` (variables résolues comme pour les messages du fichier) sur la connexion `id`.
#[tauri::command]
pub async fn ws_send(state: State<'_, AppState>, args: SendArgs) -> Reply<Sent> {
    let SendArgs { id, root, path, doc, env, data } = args;
    let sender = state.sockets.get(&id)?;
    let session = loaded_session(&state, &root, env.as_deref()).await?;
    let (data, unresolved) = resolve_message(Path::new(&root), &path, &doc, env.as_deref(), &session, &data)?;
    sender.text(data.clone()).map_err(err)?;
    Ok(Sent { data, unresolved })
}

/// Ferme la connexion `id` (code 1000) ; sans effet si elle est déjà fermée.
#[tauri::command]
pub fn ws_close(state: State<'_, AppState>, id: String) -> Reply<()> {
    if let Ok(sender) = state.sockets.get(&id) {
        sender.close().ok();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::mpsc;
    use std::time::Duration;

    use futures_util::{SinkExt, StreamExt};
    use tauri::{Listener, Manager};
    use tokio::net::TcpListener;

    use super::*;

    async fn echo_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("ws://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let Ok(mut socket) = tokio_tungstenite::accept_async(stream).await else { return };
                    while let Some(Ok(message)) = socket.next().await {
                        if message.is_text() {
                            let _ = socket.send(message).await;
                        }
                    }
                });
            }
        });
        base
    }

    fn collection(base: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: WS\n").unwrap();
        fs::create_dir(root.join("environments")).unwrap();
        fs::write(
            root.join("environments/dev.yml"),
            format!("name: dev\nvariables:\n  - name: base\n    value: {base}\n  - name: user\n    value: Ada\n"),
        )
        .unwrap();
        fs::write(
            root.join("echo.yml"),
            "info:\n  name: Écho\n  type: websocket\n  seq: 1\n\nwebsocket:\n  url: \"{{base}}/echo\"\n  message:\n    type: text\n    data: salut {{user}}\n",
        )
        .unwrap();
        dir
    }

    fn wait_for(events: &mpsc::Receiver<serde_json::Value>, kind: &str) -> serde_json::Value {
        loop {
            let event = events.recv_timeout(Duration::from_secs(5)).expect("un événement ws-event");
            if event["kind"] == kind {
                return event;
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ef_ws_01_the_commands_connect_send_receive_and_close_a_connection() {
        let base = echo_server().await;
        let dir = collection(&base);
        let root = dir.path().display().to_string();
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let state = || app.state::<AppState>();
        let (tx, events) = mpsc::channel();
        app.listen("ws-event", move |event| {
            tx.send(serde_json::from_str::<serde_json::Value>(event.payload()).unwrap()).ok();
        });
        let doc = xc_core::read_request(dir.path(), "echo.yml").unwrap();
        let args = || ConnectArgs {
            id: "c1".into(),
            root: root.clone(),
            path: "echo.yml".into(),
            doc: doc.clone(),
            env: Some("dev".into()),
        };

        let connected = ws_connect(app.handle().clone(), state(), args()).await.unwrap();
        assert_eq!((connected.status, connected.url.as_str()), (101, format!("{base}/echo").as_str()));
        assert!(connected.unresolved.is_empty());
        assert!(ws_connect(app.handle().clone(), state(), args()).await.unwrap_err().contains("déjà ouverte"));

        let sent = ws_send(
            state(),
            SendArgs {
                id: "c1".into(),
                root: root.clone(),
                path: "echo.yml".into(),
                doc: doc.clone(),
                env: Some("dev".into()),
                data: "bonjour {{user}} {{inconnue}}".into(),
            },
        )
        .await
        .unwrap();
        assert_eq!(sent.data, "bonjour Ada {{inconnue}}");
        assert_eq!(sent.unresolved, ["inconnue"]);
        let echoed = wait_for(&events, "text");
        assert_eq!((echoed["id"].as_str(), echoed["data"].as_str()), (Some("c1"), Some("bonjour Ada {{inconnue}}")));
        assert!(echoed["at"].as_str().is_some_and(|at| at.ends_with('Z')), "{echoed}");

        ws_close(state(), "c1".into()).unwrap();
        let closed = wait_for(&events, "close");
        assert_eq!(closed["code"], 1000);
        for _ in 0..50 {
            if state().sockets.0.lock().unwrap().is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(state().sockets.0.lock().unwrap().is_empty(), "la connexion fermée est oubliée");
        ws_close(state(), "c1".into()).unwrap();
        let after = ws_send(
            state(),
            SendArgs { id: "c1".into(), root, path: "echo.yml".into(), doc, env: Some("dev".into()), data: "x".into() },
        )
        .await
        .unwrap_err();
        assert!(after.contains("fermée"), "{after}");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn ef_ws_01_connecting_a_request_that_is_not_websocket_says_so() {
        let dir = collection("ws://127.0.0.1:1");
        fs::write(
            dir.path().join("http.yml"),
            "info:\n  name: H\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"http://x.test\"\n",
        )
        .unwrap();
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let doc = xc_core::read_request(dir.path(), "http.yml").unwrap();
        let args = ConnectArgs {
            id: "c2".into(),
            root: dir.path().display().to_string(),
            path: "http.yml".into(),
            doc,
            env: None,
        };

        let error = ws_connect(app.handle().clone(), app.state::<AppState>(), args).await.unwrap_err();

        assert!(error.contains("n'est pas une requête WebSocket"), "{error}");
        assert!(app.state::<AppState>().sockets.0.lock().unwrap().is_empty());
    }
}
