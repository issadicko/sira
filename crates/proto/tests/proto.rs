use std::path::PathBuf;

use xc_proto::{
    file_by_symbol_request, list_services_request, parse_reflection_response, ProtoError, ReflectionAnswer, Schema,
};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn schema() -> Schema {
    Schema::from_files(&[fixtures().join("demo.proto")], &[fixtures()]).unwrap()
}

#[test]
fn ef_grpc_01_the_proto_file_and_its_imports_give_every_service_and_method_with_its_kind() {
    let methods = schema().methods();
    let kinds: Vec<(String, &str)> = methods.iter().map(|m| (m.full_name.clone(), m.kind())).collect();
    assert_eq!(
        kinds,
        [
            ("demo.Demo/Echo".to_owned(), "unary"),
            ("demo.Demo/Watch".to_owned(), "server-streaming"),
            ("demo.Demo/Upload".to_owned(), "client-streaming"),
            ("demo.Demo/Chat".to_owned(), "bidi-streaming"),
        ]
    );
    assert_eq!(methods[0].input, "demo.EchoRequest");
    assert_eq!(methods[0].output, "demo.EchoReply");
}

#[test]
fn ef_grpc_01_a_json_message_becomes_protobuf_bytes_with_every_field() {
    let schema = schema();
    let json = r#"{"name":"Ada","userId":7,"trace":{"requestId":"r-1"},"tags":["a","b"],"big":"9007199254740993","level":"HIGH"}"#;
    let bytes = schema.encode_request("demo.Demo/Echo", json).unwrap();
    assert_eq!(bytes, schema.encode_request("/demo.Demo/Echo", json).unwrap());
    let expected: Vec<u8> = [
        &[0x0A, 3][..],
        b"Ada",
        &[0x10, 7, 0x1A, 5, 0x0A, 3],
        b"r-1",
        &[0x22, 1],
        b"a",
        &[0x22, 1],
        b"b",
        &[0x28, 0x81, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x10, 0x30, 1],
    ]
    .concat();
    assert_eq!(bytes, expected);
}

#[test]
fn ef_grpc_01_a_reply_is_read_from_protobuf_bytes_into_json() {
    let schema = schema();
    let reply = schema.decode_response("demo.Demo/Echo", &[&[0x0A, 2][..], b"ok", &[0x10, 9]].concat()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&reply).unwrap();
    assert_eq!(value, serde_json::json!({"message": "ok", "userId": 9}));
    assert!(matches!(schema.decode_response("demo.Demo/Echo", &[0x0A, 9]), Err(ProtoError::Decode { .. })));
}

#[test]
fn ef_grpc_01_an_empty_reply_shows_every_field_at_its_default() {
    let reply = schema().decode_response("demo.Demo/Echo", &[]).unwrap();
    assert!(reply.contains("\"message\": \"\""), "{reply}");
    assert!(reply.contains("\"userId\": 0"), "{reply}");
}

#[test]
fn ef_grpc_01_a_message_that_does_not_fit_the_schema_is_refused_with_the_message_name() {
    let schema = schema();
    for bad in [r#"{"inconnu": 1}"#, r#"{"userId": "abc"}"#, "{", r#"{"name":"a"} {"#] {
        match schema.encode_request("demo.Demo/Echo", bad) {
            Err(ProtoError::Json { message, .. }) => assert_eq!(message, "demo.EchoRequest"),
            other => panic!("{bad} : {other:?}"),
        }
    }
    assert!(schema.encode_request("demo.Demo/Echo", "  ").is_ok());
}

#[test]
fn ef_grpc_01_an_unknown_method_and_a_broken_proto_are_errors() {
    assert!(matches!(schema().method("demo.Demo/Nope"), Err(ProtoError::UnknownMethod(_))));
    assert!(matches!(schema().method("Echo"), Err(ProtoError::UnknownMethod(_))));
    let dir = std::env::temp_dir().join(format!("xc-proto-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let broken = dir.join("broken.proto");
    std::fs::write(&broken, "syntax = \"proto3\"; message {").unwrap();
    assert!(matches!(Schema::from_files(&[broken], &[dir.clone()]), Err(ProtoError::Compile(_))));
    assert!(matches!(Schema::from_files(&[dir.join("absent.proto")], &[dir.clone()]), Err(ProtoError::Compile(_))));
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn ef_grpc_01_the_skeleton_of_a_request_lists_every_field_with_its_default() {
    let skeleton: serde_json::Value = serde_json::from_str(&schema().skeleton("demo.Demo/Echo").unwrap()).unwrap();
    assert_eq!(
        skeleton,
        serde_json::json!({"name": "", "userId": 0, "trace": {"requestId": ""}, "tags": [], "big": "0", "level": "LOW"})
    );
}

#[test]
fn ef_grpc_01_descriptors_received_by_reflection_build_the_same_schema_in_any_order() {
    use prost::Message as _;
    let direct = Schema::from_files(&[fixtures().join("demo.proto")], &[fixtures()]).unwrap();
    let mut files: Vec<Vec<u8>> = Vec::new();
    let compiled = protox::compile([fixtures().join("demo.proto")], [fixtures()]).unwrap();
    for file in &compiled.file {
        files.push(file.encode_to_vec());
    }
    files.reverse();
    let rebuilt = Schema::from_descriptors(&files).unwrap();
    assert_eq!(rebuilt.methods(), direct.methods());
    assert!(matches!(Schema::from_descriptors(&files[..1]), Err(ProtoError::Descriptor(_))));
}

#[test]
fn ef_grpc_01_reflection_requests_and_answers_are_encoded_and_read() {
    assert_eq!(list_services_request(), [0x3A, 0x00]);
    let symbol = file_by_symbol_request("demo.Demo");
    assert_eq!(symbol, [&[0x22u8, 9][..], b"demo.Demo"].concat());

    let mut services = vec![0x32, 13, 0x0A, 11, 0x0A, 9];
    services.extend_from_slice(b"demo.Demo");
    assert_eq!(parse_reflection_response(&services).unwrap(), ReflectionAnswer::Services(vec!["demo.Demo".into()]));

    let files = [&[0x22u8, 5, 0x0A, 3][..], b"abc"].concat();
    assert_eq!(parse_reflection_response(&files).unwrap(), ReflectionAnswer::Files(vec![b"abc".to_vec()]));

    let error = [&[0x3Au8, 6, 0x08, 5, 0x12, 2][..], b"no"].concat();
    assert_eq!(parse_reflection_response(&error).unwrap(), ReflectionAnswer::Error { code: 5, message: "no".into() });
}
