use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{json, Value};
use std::time::Duration;

use xc_core::graphql::read_stored;
use xc_core::{open_collection, read_request, RequestDoc};
use xc_runner::{
    fetch_schema, run_collection, run_request, schema_url, select, Job, Request, SchemaSource, Session, MAX_JUMPS,
};

type Seen = Arc<Mutex<Vec<String>>>;

fn serve(reply: &'static str) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            log.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).into_owned());
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    (base, seen)
}

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("opencollection.yml"),
        format!(
            "opencollection: 1.0.0\n\ninfo:\n  name: Shop\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n  headers:\n    - name: X-Team\n      value: sira\n  scripts:\n    - type: before-request\n      code: bru.setVar('ran', '1');\n"
        ),
    )
    .unwrap();
    dir
}

const PRODUCT: &str = "info:\n  name: Product\n  type: graphql\n  seq: 1\n\ngraphql:\n  method: POST\n  url: \"{{baseUrl}}/graphql\"\n  body:\n    query: |-\n      query Product($sku: String!) {\n        product(sku: $sku) { name }\n      }\n    variables: |-\n      {\n        \"sku\": \"DS4-B\"\n      }\n\nruntime:\n  assertions:\n    - expression: res.body.data.product.name\n      operator: eq\n      value: Gamepad\n";

fn request(root: &Path) -> (String, RequestDoc) {
    fs::write(root.join("product.yml"), PRODUCT).unwrap();
    ("product.yml".to_owned(), read_request(root, "product.yml").unwrap())
}

fn body_of(raw: &str) -> Value {
    serde_json::from_str(raw.split("\r\n\r\n").nth(1).unwrap()).unwrap()
}

#[tokio::test]
async fn ef_gql_01_a_graphql_request_runs_with_its_scripts_and_assertions() {
    let (base, seen) = serve(r#"{"data":{"product":{"name":"Gamepad"}}}"#);
    let dir = collection(&base);
    let (path, doc) = request(dir.path());
    let mut session = Session::default();
    let outcome = run_request(
        Request {
            root: dir.path(),
            path: &path,
            doc: &doc,
            env: None,
            collection_name: "Shop",
            execution_mode: "cli",
            cancel: Arc::new(AtomicBool::new(false)),
        },
        &mut session,
    )
    .await;
    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(outcome.passed(), "{:?}", outcome.assertions);
    assert_eq!(session.runtime.get("ran"), Some(&json!("1")), "les scripts de la collection tournent");

    let raw = seen.lock().unwrap()[0].clone();
    assert!(raw.starts_with("POST /graphql "), "{raw}");
    assert!(raw.to_lowercase().contains("content-type: application/json"), "{raw}");
    assert!(raw.contains("X-Team: sira") || raw.contains("x-team: sira"), "{raw}");
    let sent = body_of(&raw);
    assert_eq!(sent["variables"], json!({ "sku": "DS4-B" }));
    assert!(sent["query"].as_str().unwrap().starts_with("query Product("));
}

#[tokio::test]
async fn ef_gql_01_the_schema_is_introspected_kept_on_disk_and_found_again_by_url() {
    let reply = r#"{"data":{"__schema":{"queryType":{"name":"Query"},"types":[{"kind":"OBJECT","name":"Query","fields":[]}]}}}"#;
    let (base, seen) = serve(reply);
    let dir = collection(&base);
    let (path, doc) = request(dir.path());
    let mut session = Session::default();
    let source = SchemaSource { root: dir.path(), path: &path, doc: &doc, env: None };

    assert!(read_stored(dir.path(), &schema_url(&source, &session).unwrap()).is_none());
    let schema =
        fetch_schema(SchemaSource { root: dir.path(), path: &path, doc: &doc, env: None }, &mut session).await.unwrap();
    assert_eq!(schema.url, format!("{base}/graphql"));
    assert_eq!(schema.introspection["__schema"]["queryType"]["name"], "Query");

    let kept = read_stored(dir.path(), &schema_url(&source, &session).unwrap()).unwrap();
    assert_eq!(kept, schema);
    assert!(dir.path().join(".oc-sync/graphql").is_dir());

    let sent = body_of(&seen.lock().unwrap()[0]);
    assert!(sent["query"].as_str().unwrap().contains("__schema"));
    assert_eq!(sent["variables"], json!({}));
    assert!(session.runtime.get("ran").is_none(), "l'introspection ne lance aucun script");
}

#[tokio::test]
async fn ef_gql_01_a_server_that_refuses_introspection_gives_its_reason() {
    let (base, _) = serve(r#"{"errors":[{"message":"introspection is disabled"}]}"#);
    let dir = collection(&base);
    let (path, doc) = request(dir.path());
    let mut session = Session::default();
    let source = SchemaSource { root: dir.path(), path: &path, doc: &doc, env: None };
    let error = fetch_schema(source, &mut session).await.unwrap_err();
    assert!(error.contains("introspection is disabled"), "{error}");
    assert!(!dir.path().join(".oc-sync").exists(), "rien n'est gardé");
}

#[tokio::test]
async fn ef_gql_01_a_collection_run_sends_graphql_requests_with_the_others() {
    let (base, seen) = serve(r#"{"data":{"product":{"name":"Gamepad"}}}"#);
    let dir = collection(&base);
    request(dir.path());
    fs::write(
        dir.path().join("health.yml"),
        "info:\n  name: Health\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/health\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("stream.yml"),
        "info:\n  name: Stream\n  type: grpc\n  seq: 3\n\ngrpc:\n  url: localhost:50051\n",
    )
    .unwrap();

    let info = open_collection(dir.path()).unwrap();
    let items = select(&info.items, &[]).unwrap();
    let paths: Vec<&str> = items.iter().map(|i| i.path.as_str()).collect();
    assert_eq!(paths, ["product.yml", "health.yml"], "GraphQL se lance, gRPC reste ignoré");

    let job = Job {
        root: dir.path(),
        collection_name: &info.name,
        env: None,
        items: &items,
        rows: &[],
        bail: false,
        delay: Duration::ZERO,
        max_jumps: MAX_JUMPS,
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let report = run_collection(job, &mut Session::default(), &mut |_| {}).await;
    assert_eq!(report.results.len(), 2);
    assert!(!report.failed(), "{:?}", report.summary());
    let requests = seen.lock().unwrap();
    assert!(requests[0].starts_with("POST /graphql ") && requests[1].starts_with("GET /health "), "{requests:?}");
}
