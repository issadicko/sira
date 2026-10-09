use std::collections::HashMap;
use std::fs;
use std::path::Path;

use xc_core::vars::Context;
use xc_core::{prepare_with, protobuf_config, read_request, save_request, GrpcMessage, Overrides};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn collection() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "opencollection.yml",
        "opencollection: 1.0.0\n\ninfo:\n  name: Grpc\n\nconfig:\n  protobuf:\n    protoFiles:\n      - type: file\n        path: proto/demo.proto\n      - type: file\n        path: proto/off.proto\n        disabled: true\n    importPaths:\n      - path: proto\n\nrequest:\n  variables:\n    - name: base\n      value: localhost:50051\n    - name: user\n      value: Ada\n",
    );
    write(dir.path(), "api/folder.yml", "info:\n  name: api\n  type: folder\nrequest:\n  metadata:\n    - name: x-folder\n      value: dossier-{{user}}\n");
    dir
}

const UNARY: &str = "info:\n  name: Écho\n  type: grpc\n  seq: 1\n\ngrpc:\n  url: \"{{base}}\"\n  method: demo.Demo/Echo\n  methodType: unary\n  protoFilePath: proto/demo.proto\n  metadata:\n    - name: x-user\n      value: \"{{user}}\"\n  message: |-\n    {\n      \"name\": \"{{user}}\"\n    }\n\nruntime:\n  auth:\n    type: bearer\n    token: tok-{{user}}\n";

const STREAM: &str = "info:\n  name: Flux\n  type: grpc\n\ngrpc:\n  url: \"{{base}}\"\n  method: demo.Demo/Chat\n  methodType: bidi-streaming\n  message:\n    - description: Premier\n      message: |-\n        {\n          \"name\": \"un\"\n        }\n    - description: Second\n      message: |-\n        {\n          \"name\": \"deux\"\n        }\n";

#[test]
fn ef_grpc_01_a_grpc_request_is_read_with_its_method_proto_metadata_message_and_auth() {
    let dir = collection();
    write(dir.path(), "api/echo.yml", UNARY);

    let doc = read_request(dir.path(), "api/echo.yml").unwrap();

    assert_eq!(doc.request_type, "grpc");
    assert_eq!(doc.method, "demo.Demo/Echo");
    assert_eq!(doc.grpc_method_type, "unary");
    assert_eq!(doc.proto_file, "proto/demo.proto");
    assert_eq!(doc.headers.len(), 1);
    assert_eq!(doc.headers[0].name, "x-user");
    assert_eq!(
        doc.grpc_messages,
        [GrpcMessage { description: String::new(), message: "{\n  \"name\": \"{{user}}\"\n}".into() }]
    );
    assert!(matches!(doc.auth, xc_core::Auth::Bearer { ref token } if token == "tok-{{user}}"));
}

#[test]
fn ef_grpc_01_a_list_of_messages_keeps_descriptions_and_order() {
    let dir = collection();
    write(dir.path(), "chat.yml", STREAM);
    let doc = read_request(dir.path(), "chat.yml").unwrap();
    let descriptions: Vec<&str> = doc.grpc_messages.iter().map(|m| m.description.as_str()).collect();
    assert_eq!(descriptions, ["Premier", "Second"]);
    assert_eq!(doc.proto_file, "");
}

#[test]
fn ef_grpc_01_saving_an_unchanged_grpc_request_writes_nothing() {
    let dir = collection();
    for (file, text) in [("api/echo.yml", UNARY), ("chat.yml", STREAM)] {
        write(dir.path(), file, text);
        let doc = read_request(dir.path(), file).unwrap();
        assert!(!save_request(dir.path(), file, &doc).unwrap());
        assert_eq!(fs::read_to_string(dir.path().join(file)).unwrap(), text);
    }
}

#[test]
fn ef_grpc_01_editing_a_grpc_request_changes_only_the_edited_fields_and_keeps_the_method_case() {
    let dir = collection();
    write(dir.path(), "api/echo.yml", UNARY);
    let mut doc = read_request(dir.path(), "api/echo.yml").unwrap();
    doc.method = "demo.Demo/Watch".into();
    doc.grpc_method_type = "server-streaming".into();
    doc.headers[0].value = "autre".into();
    doc.grpc_messages[0].message = "{}".into();
    doc.auth = xc_core::Auth::Bearer { token: "neuf".into() };

    assert!(save_request(dir.path(), "api/echo.yml", &doc).unwrap());

    let saved = fs::read_to_string(dir.path().join("api/echo.yml")).unwrap();
    assert!(saved.contains("method: demo.Demo/Watch\n"), "{saved}");
    assert!(saved.contains("methodType: server-streaming\n"), "{saved}");
    assert!(saved.contains("value: autre\n"), "{saved}");
    assert!(
        saved.contains("message: '{}'") || saved.contains("message: \"{}\"") || saved.contains("message: '{}'\n"),
        "{saved}"
    );
    assert!(saved.contains("token: neuf\n"), "{saved}");
    assert!(!saved.contains("http:"), "{saved}");
    assert!(!saved.contains("headers:"), "{saved}");
    let again = read_request(dir.path(), "api/echo.yml").unwrap();
    assert_eq!(again.method, "demo.Demo/Watch");
}

#[test]
fn ef_grpc_01_a_message_added_to_a_single_one_turns_the_field_into_a_list_and_back() {
    let dir = collection();
    write(dir.path(), "api/echo.yml", UNARY);
    let mut doc = read_request(dir.path(), "api/echo.yml").unwrap();
    doc.grpc_messages.push(GrpcMessage { description: "Deux".into(), message: "{}".into() });
    save_request(dir.path(), "api/echo.yml", &doc).unwrap();
    let after = read_request(dir.path(), "api/echo.yml").unwrap();
    assert_eq!(after.grpc_messages.len(), 2);
    assert_eq!(after.grpc_messages[1].description, "Deux");

    let mut back = after.clone();
    back.grpc_messages.truncate(1);
    back.grpc_messages[0].description.clear();
    save_request(dir.path(), "api/echo.yml", &back).unwrap();
    let saved = fs::read_to_string(dir.path().join("api/echo.yml")).unwrap();
    assert!(saved.contains("  message: |-\n"), "{saved}");
}

#[test]
fn ef_grpc_01_preparing_a_grpc_request_resolves_variables_and_merges_folder_metadata() {
    let dir = collection();
    write(dir.path(), "api/echo.yml", UNARY);
    let doc = read_request(dir.path(), "api/echo.yml").unwrap();

    let prepared = prepare_with(dir.path(), "api/echo.yml", &doc, None, &HashMap::new(), Overrides::default()).unwrap();

    assert_eq!(prepared.request.url, "grpc://localhost:50051");
    assert_eq!(prepared.request.method, "demo.Demo/Echo");
    let header = |name: &str| prepared.request.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    assert_eq!(header("x-folder").as_deref(), Some("dossier-Ada"));
    assert_eq!(header("x-user").as_deref(), Some("Ada"));
    assert_eq!(header("Authorization").as_deref(), Some("Bearer tok-Ada"));
    assert_eq!(prepared.messages.len(), 1);
    assert_eq!(prepared.messages[0].data, "{\n  \"name\": \"Ada\"\n}");
}

#[test]
fn ef_grpc_01_the_collection_lists_its_proto_files_and_import_paths_without_the_disabled_ones() {
    let dir = collection();
    let ctx = Context::load(dir.path(), "chat.yml").unwrap();
    let config = protobuf_config(&ctx.collection);
    assert_eq!(config.proto_files, ["proto/demo.proto"]);
    assert_eq!(config.import_paths, ["proto"]);
}
