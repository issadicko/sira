use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::thread;

use xc_core::read_request;
use xc_runner::{run_request, Request, Session};

/// Un serveur qui renvoie l'en-tête `X-Token` reçu.
fn echo() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let text = String::from_utf8_lossy(&buf[..n]).into_owned();
            let token = text
                .lines()
                .find_map(|l| l.strip_prefix("X-Token: ").or_else(|| l.strip_prefix("x-token: ")))
                .unwrap_or("absent");
            let body = format!("{{\"token\":\"{token}\"}}");
            let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    base
}

fn setup(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: S\n").unwrap();
    fs::create_dir(root.join("environments")).unwrap();
    fs::write(
        root.join("environments/dev.yml"),
        format!("name: dev\nvariables:\n  - name: base\n    value: {base}\n  - secret: true\n    name: token\n"),
    )
    .unwrap();
    fs::write(
        root.join("api.yml"),
        "info:\n  name: Api\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/a\"\n  headers:\n    - name: X-Token\n      value: \"{{token}}\"\n\nruntime:\n  scripts:\n    - type: tests\n      code: |-\n        test('le script lit le secret', () => expect(bru.getEnvVar('token')).to.equal('s3cr3t'));\n",
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn enf_sec_01_a_secret_given_to_the_session_reaches_the_request_and_the_scripts() {
    let dir = setup(&echo());
    let doc = read_request(dir.path(), "api.yml").unwrap();
    let mut session = Session { secrets: vec![("token".into(), "s3cr3t".into())], ..Session::default() };
    let req = Request {
        root: dir.path(),
        path: "api.yml",
        doc: &doc,
        env: Some("dev"),
        collection_name: "S",
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let out = run_request(req, &mut session).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert!(out.unresolved.is_empty(), "{:?}", out.unresolved);
    assert_eq!(String::from_utf8_lossy(&out.response.as_ref().unwrap().body), r#"{"token":"s3cr3t"}"#);
    assert_eq!(out.tests.results.iter().map(|r| r.status.as_str()).collect::<Vec<_>>(), ["pass"], "{:?}", out.tests);
}

#[tokio::test]
async fn enf_sec_01_without_a_value_the_secret_stays_unresolved_and_is_reported() {
    let dir = setup(&echo());
    let doc = read_request(dir.path(), "api.yml").unwrap();
    let req = Request {
        root: dir.path(),
        path: "api.yml",
        doc: &doc,
        env: Some("dev"),
        collection_name: "S",
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let out = run_request(req, &mut Session::default()).await;

    assert_eq!(out.unresolved, ["token"]);
}
