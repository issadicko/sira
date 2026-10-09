#![allow(clippy::result_large_err)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// `/echo` renvoie ce qu'il reçoit, `/close` ferme en 4000 « bye », `/reject` répond 401.
async fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("ws://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut path = String::new();
                let callback = |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
                    path = request.uri().path().to_owned();
                    if path == "/reject" {
                        let mut refused = ErrorResponse::new(Some("jeton manquant".to_owned()));
                        *refused.status_mut() = StatusCode::UNAUTHORIZED;
                        return Err(refused);
                    }
                    Ok(response)
                };
                let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, callback).await else { return };
                if path == "/close" {
                    let frame = CloseFrame { code: CloseCode::from(4000), reason: "bye".into() };
                    let _ = socket.send(Message::Close(Some(frame))).await;
                }
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
    write(root, "opencollection.yml", "opencollection: 1.0.0\n\ninfo:\n  name: WS\nbundled: false\nextensions: {}\n");
    write(root, "environments/local.yml", &format!("name: local\nvariables:\n  - name: ws\n    value: {base}\n"));
    let request = |name: &str, path: &str, message: &str| {
        format!(
            "info:\n  name: {name}\n  type: websocket\n  seq: 1\n\nwebsocket:\n  url: \"{{{{ws}}}}{path}\"\n{message}"
        )
    };
    write(
        root,
        "echo.yml",
        &request(
            "Écho",
            "/echo",
            "  message:\n    - title: Coché\n      selected: true\n      message:\n        type: text\n        data: coché {{ws}}\n    - title: Pas coché\n      selected: false\n      message:\n        type: text\n        data: ne part pas\n",
        ),
    );
    write(root, "close.yml", &request("Fermeture", "/close", ""));
    write(root, "reject.yml", &request("Refus", "/reject", ""));
    dir
}

async fn xc(args: Vec<String>) -> Output {
    tokio::task::spawn_blocking(move || Command::new(env!("CARGO_BIN_EXE_xc")).args(args).output().unwrap())
        .await
        .unwrap()
}

fn run(dir: &tempfile::TempDir, request: &str, extra: &[&str]) -> Vec<String> {
    let mut args = vec!["ws".to_owned(), dir.path().display().to_string(), request.to_owned()];
    args.extend(["--env", "local", "--noproxy"].map(str::to_owned));
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    args
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[tokio::test]
async fn ef_ws_01_the_checked_messages_go_out_and_the_replies_are_shown() {
    let base = serve().await;
    let dir = collection(&base);

    let out = xc(run(&dir, "echo", &["--send", "en plus", "--idle", "0.4"])).await;
    let stdout = text(&out.stdout);

    assert_eq!(out.status.code(), Some(0), "{stdout}\n{}", text(&out.stderr));
    assert!(stdout.contains(&format!("✓ connecté à {base}/echo (101)")), "{stdout}");
    assert!(stdout.contains(&format!("→ texte ({} o) : coché {base}", format!("coché {base}").len())), "{stdout}");
    assert!(stdout.contains("→ texte (7 o) : en plus"), "{stdout}");
    assert!(stdout.contains("← texte (7 o) : en plus"), "{stdout}");
    assert!(!stdout.contains("ne part pas"), "{stdout}");
    assert!(stdout.contains("→ fermeture (1000)") && stdout.contains("← fermeture (1000)"), "{stdout}");
}

#[tokio::test]
async fn ef_ws_01_no_send_connects_without_sending_the_file_messages() {
    let base = serve().await;
    let dir = collection(&base);

    let out = xc(run(&dir, "echo.yml", &["--no-send", "--idle", "0.3"])).await;
    let stdout = text(&out.stdout);

    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(!stdout.contains("→ texte"), "{stdout}");
}

#[tokio::test]
async fn ef_ws_01_a_close_from_the_server_is_shown_with_its_code_and_ends_the_command() {
    let base = serve().await;
    let dir = collection(&base);

    let out = xc(run(&dir, "close", &["--idle", "5"])).await;
    let stdout = text(&out.stdout);

    assert_eq!(out.status.code(), Some(0), "{stdout}");
    assert!(stdout.contains("← fermeture (4000) : bye"), "{stdout}");
}

#[tokio::test]
async fn ef_ws_01_a_refused_upgrade_fails_the_command_with_the_reason() {
    let base = serve().await;
    let dir = collection(&base);

    let out = xc(run(&dir, "reject", &["--idle", "0.3"])).await;
    let stderr = text(&out.stderr);

    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(
        stderr.contains("connexion impossible") && stderr.contains("401") && stderr.contains("jeton manquant"),
        "{stderr}"
    );
}

#[tokio::test]
async fn ef_ws_01_a_request_that_does_not_exist_or_is_not_websocket_is_an_input_error() {
    let base = serve().await;
    let dir = collection(&base);
    write(
        dir.path(),
        "http.yml",
        "info:\n  name: H\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"http://x.test\"\n",
    );

    let missing = xc(run(&dir, "absente", &["--idle", "0.3"])).await;
    assert_eq!(missing.status.code(), Some(2));
    let wrong = xc(run(&dir, "http", &["--idle", "0.3"])).await;
    assert_eq!(wrong.status.code(), Some(1), "{}", text(&wrong.stderr));
    assert!(text(&wrong.stderr).contains("n'est pas une requête WebSocket"), "{}", text(&wrong.stderr));
}
