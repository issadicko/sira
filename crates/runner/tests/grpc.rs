#[path = "support/grpc_server.rs"]
mod grpc_server;

use std::fs;
use std::path::Path;
use std::time::Duration;

use xc_core::read_request;
use xc_engine::GrpcEvent;
use xc_runner::{decode_message, encode_message, open_grpc, GrpcStart, Session};

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

fn collection(address: &str, request: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    copy_dir(&grpc_server::proto_dir(), &root.join("proto"));
    fs::write(
        root.join("opencollection.yml"),
        format!(
            "opencollection: 1.0.0\n\ninfo:\n  name: Grpc\n\nconfig:\n  protobuf:\n    importPaths:\n      - path: proto\n\nrequest:\n  variables:\n    - name: addr\n      value: {address}\n    - name: user\n      value: Ada\n"
        ),
    )
    .unwrap();
    fs::write(root.join("call.yml"), request).unwrap();
    dir
}

fn echo(proto: bool, method: &str) -> String {
    let proto_line = if proto { "  protoFilePath: proto/demo.proto\n" } else { "" };
    format!(
        "info:\n  name: Écho\n  type: grpc\n\ngrpc:\n  url: \"{{{{addr}}}}\"\n  method: {method}\n  methodType: unary\n{proto_line}  metadata:\n    - name: x-user\n      value: \"{{{{user}}}}\"\n  message: '{{\"name\": \"{{{{user}}}}\", \"userId\": 7}}'\n\nruntime:\n  auth:\n    type: bearer\n    token: tok\n"
    )
}

async fn replies(start: &mut GrpcStart) -> (Vec<String>, u32) {
    let mut messages = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(5), start.call.events.recv()).await.unwrap().unwrap() {
            GrpcEvent::Message { data } => messages.push(decode_message(&start.schema, &start.method, &data)),
            GrpcEvent::Status { code, .. } => return (messages, code),
            GrpcEvent::Error { message } => panic!("{message}"),
            GrpcEvent::Headers { .. } => {}
        }
    }
}

async fn send_first_then_finish(start: &GrpcStart) {
    let bytes = encode_message(&start.schema, &start.method, &start.messages[0].data).unwrap();
    start.call.sender.send(bytes).unwrap();
    start.call.sender.finish().unwrap();
}

#[tokio::test]
async fn ef_grpc_01_a_unary_request_from_a_file_with_a_proto_runs_with_variables_metadata_and_auth() {
    let (address, log) = grpc_server::serve(false).await;
    let dir = collection(&address, &echo(true, "demo.Demo/Echo"));
    let doc = read_request(dir.path(), "call.yml").unwrap();

    let mut start = open_grpc(dir.path(), "call.yml", &doc, None, &Session::default()).await.unwrap();
    assert_eq!(start.schema_source, "proto/demo.proto");
    assert_eq!(start.method.kind(), "unary");
    send_first_then_finish(&start).await;
    let (messages, code) = replies(&mut start).await;

    assert_eq!(code, 0);
    assert_eq!(messages.len(), 1);
    let reply: serde_json::Value = serde_json::from_str(&messages[0]).unwrap();
    assert_eq!(reply["message"], "salut Ada|Bearer tok|Ada");
    assert_eq!(reply["userId"], 7);
    assert_eq!(log.lock().unwrap()[0].path, "demo.Demo/Echo");
}

#[tokio::test]
async fn ef_grpc_01_without_a_proto_file_the_schema_comes_from_the_server_reflection() {
    let (address, _) = grpc_server::serve(true).await;
    let dir = collection(&address, &echo(false, "demo.Demo/Echo"));
    let doc = read_request(dir.path(), "call.yml").unwrap();

    let mut start = open_grpc(dir.path(), "call.yml", &doc, None, &Session::default()).await.unwrap();
    assert_eq!(start.schema_source, "réflexion du serveur");
    send_first_then_finish(&start).await;
    let (messages, code) = replies(&mut start).await;
    assert_eq!(code, 0);
    assert!(messages[0].contains("salut Ada"), "{messages:?}");
}

#[tokio::test]
async fn ef_grpc_01_without_a_proto_file_and_without_reflection_the_error_says_both() {
    let (address, _) = grpc_server::serve(false).await;
    let dir = collection(&address, &echo(false, "demo.Demo/Echo"));
    let doc = read_request(dir.path(), "call.yml").unwrap();

    let error = open_grpc(dir.path(), "call.yml", &doc, None, &Session::default()).await.unwrap_err();

    assert!(error.contains("aucun fichier .proto"), "{error}");
    assert!(error.contains("UNIMPLEMENTED"), "{error}");
}

#[tokio::test]
async fn ef_grpc_01_a_method_missing_from_the_proto_file_is_refused_with_the_file_name() {
    let (address, _) = grpc_server::serve(true).await;
    let dir = collection(&address, &echo(true, "demo.Demo/Absent"));
    let doc = read_request(dir.path(), "call.yml").unwrap();

    let error = open_grpc(dir.path(), "call.yml", &doc, None, &Session::default()).await.unwrap_err();

    assert_eq!(error, "la méthode demo.Demo/Absent n'existe pas dans proto/demo.proto");
}

#[tokio::test]
async fn ef_grpc_01_a_non_grpc_request_and_an_unsupported_auth_are_refused() {
    let (address, _) = grpc_server::serve(false).await;
    let dir = collection(&address, "info:\n  name: Http\n  type: http\nhttp:\n  method: GET\n  url: http://x\n");
    let doc = read_request(dir.path(), "call.yml").unwrap();
    let error = open_grpc(dir.path(), "call.yml", &doc, None, &Session::default()).await.unwrap_err();
    assert!(error.contains("n'est pas une requête gRPC"), "{error}");

    let digest = echo(true, "demo.Demo/Echo")
        .replace("type: bearer\n    token: tok", "type: digest\n    username: u\n    password: p");
    fs::write(dir.path().join("call.yml"), digest).unwrap();
    let doc = read_request(dir.path(), "call.yml").unwrap();
    let error = open_grpc(dir.path(), "call.yml", &doc, None, &Session::default()).await.unwrap_err();
    assert!(error.contains("Digest"), "{error}");
}

#[tokio::test]
async fn ef_grpc_01_a_message_that_does_not_fit_the_proto_is_refused_before_sending() {
    let (address, _) = grpc_server::serve(false).await;
    let dir = collection(&address, &echo(true, "demo.Demo/Echo"));
    let doc = read_request(dir.path(), "call.yml").unwrap();
    let start = open_grpc(dir.path(), "call.yml", &doc, None, &Session::default()).await.unwrap();

    let error = encode_message(&start.schema, &start.method, r#"{"inconnu": true}"#).unwrap_err();

    assert!(error.contains("demo.EchoRequest"), "{error}");
}

#[tokio::test]
async fn ef_grpc_01_describing_a_request_lists_the_methods_of_its_proto_or_of_the_server() {
    let (address, _) = grpc_server::serve(true).await;
    let dir = collection(&address, &echo(true, "demo.Demo/Echo"));
    let doc = read_request(dir.path(), "call.yml").unwrap();
    let (schema, source) =
        xc_runner::describe_grpc(dir.path(), "call.yml", &doc, None, &Session::default()).await.unwrap();
    assert_eq!(source, "proto/demo.proto");
    let names: Vec<String> = schema.methods().into_iter().map(|m| m.full_name).collect();
    assert_eq!(names, ["demo.Demo/Echo", "demo.Demo/Watch", "demo.Demo/Upload", "demo.Demo/Chat"]);

    fs::write(dir.path().join("call.yml"), echo(false, "")).unwrap();
    let doc = read_request(dir.path(), "call.yml").unwrap();
    let (schema, source) =
        xc_runner::describe_grpc(dir.path(), "call.yml", &doc, None, &Session::default()).await.unwrap();
    assert_eq!(source, "réflexion du serveur");
    assert_eq!(schema.methods().len(), 4);
}
