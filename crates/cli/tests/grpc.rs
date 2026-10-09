#[path = "../../runner/tests/support/grpc_server.rs"]
mod grpc_server;

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

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
    write(
        root,
        "opencollection.yml",
        "opencollection: 1.0.0\n\ninfo:\n  name: Grpc\nbundled: false\nextensions: {}\nconfig:\n  protobuf:\n    importPaths:\n      - path: proto\n",
    );
    write(root, "environments/local.yml", &format!("name: local\nvariables:\n  - name: addr\n    value: {address}\n"));
    let request = |name: &str, method: &str, kind: &str, message: &str| {
        format!(
            "info:\n  name: {name}\n  type: grpc\n  seq: 1\n\ngrpc:\n  url: \"{{{{addr}}}}\"\n  method: {method}\n  methodType: {kind}\n  protoFilePath: proto/demo.proto\n  metadata:\n    - name: x-user\n      value: Ada\n{message}"
        )
    };
    write(
        root,
        "echo.yml",
        &request("Écho", "demo.Demo/Echo", "unary", "  message: '{\"name\": \"Ada\", \"userId\": 7}'\n"),
    );
    write(
        root,
        "watch.yml",
        &request("Flux", "demo.Demo/Watch", "server-streaming", "  message: '{\"name\": \"x\"}'\n"),
    );
    write(
        root,
        "upload.yml",
        &request(
            "Envoi",
            "demo.Demo/Upload",
            "client-streaming",
            "  message:\n    - description: un\n      message: '{\"name\": \"a\"}'\n    - description: deux\n      message: '{\"name\": \"b\"}'\n",
        ),
    );
    write(root, "absent.yml", &request("Absent", "demo.Demo/Absent", "unary", ""));
    dir
}

async fn xc(dir: &tempfile::TempDir, request: &str, extra: &[&str]) -> Output {
    let mut args = vec!["grpc".to_owned(), dir.path().display().to_string(), request.to_owned()];
    args.extend(["--env", "local", "--noproxy"].map(str::to_owned));
    args.extend(extra.iter().map(|s| (*s).to_owned()));
    tokio::task::spawn_blocking(move || Command::new(env!("CARGO_BIN_EXE_xc")).args(args).output().unwrap())
        .await
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[tokio::test]
async fn ef_grpc_01_a_unary_request_prints_the_sent_and_received_messages_and_the_status() {
    let (address, _) = grpc_server::serve(false).await;
    let dir = collection(&address);

    let out = xc(&dir, "echo", &[]).await;
    let stdout = text(&out.stdout);

    assert_eq!(out.status.code(), Some(0), "{stdout}\n{}", text(&out.stderr));
    assert!(
        stdout.contains("demo.Demo/Echo") && stdout.contains("unary") && stdout.contains("proto/demo.proto"),
        "{stdout}"
    );
    assert!(stdout.contains("→ message"), "{stdout}");
    assert!(stdout.contains("salut Ada||Ada"), "{stdout}");
    assert!(stdout.contains("\"userId\": 7"), "{stdout}");
    assert!(stdout.contains("← statut OK"), "{stdout}");
}

#[tokio::test]
async fn ef_grpc_01_a_server_stream_prints_every_reply_in_order() {
    let (address, _) = grpc_server::serve(false).await;
    let dir = collection(&address);

    let out = xc(&dir, "watch.yml", &[]).await;
    let stdout = text(&out.stdout);

    assert_eq!(out.status.code(), Some(0), "{stdout}");
    let (one, two, three) = (stdout.find("w1").unwrap(), stdout.find("w2").unwrap(), stdout.find("w3").unwrap());
    assert!(one < two && two < three, "{stdout}");
}

#[tokio::test]
async fn ef_grpc_01_a_client_stream_sends_every_file_message_and_send_replaces_them() {
    let (address, log) = grpc_server::serve(false).await;
    let dir = collection(&address);

    let out = xc(&dir, "upload", &[]).await;
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stdout));
    assert!(text(&out.stdout).contains("2 reçus"), "{}", text(&out.stdout));

    let out = xc(&dir, "upload", &["--send", r#"{"name":"seul"}"#]).await;
    assert!(text(&out.stdout).contains("1 reçus"), "{}", text(&out.stdout));
    let seen = log.lock().unwrap();
    assert_eq!(seen[0].messages.len(), 2);
    assert_eq!(seen[1].messages.len(), 1);
    assert!(seen[1].messages[0].contains("seul"));
}

#[tokio::test]
async fn ef_grpc_01_two_messages_on_a_unary_call_are_refused_with_exit_code_2() {
    let (address, _) = grpc_server::serve(false).await;
    let dir = collection(&address);

    let out = xc(&dir, "echo", &["--send", "{}", "--send", "{}"]).await;

    assert_eq!(out.status.code(), Some(2), "{}", text(&out.stdout));
    assert!(text(&out.stderr).contains("un seul message"), "{}", text(&out.stderr));
}

#[tokio::test]
async fn ef_grpc_01_a_missing_method_and_an_invalid_message_fail_with_a_reason() {
    let (address, _) = grpc_server::serve(false).await;
    let dir = collection(&address);

    let out = xc(&dir, "absent", &[]).await;
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("n'existe pas dans proto/demo.proto"), "{}", text(&out.stderr));

    let out = xc(&dir, "echo", &["--send", r#"{"inconnu":1}"#]).await;
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("demo.EchoRequest"), "{}", text(&out.stderr));
}

#[tokio::test]
async fn ef_grpc_01_a_server_that_is_down_fails_the_command() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let dir = collection(&format!("127.0.0.1:{port}"));

    let out = xc(&dir, "echo", &[]).await;

    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("appel impossible"), "{}", text(&out.stderr));
}
