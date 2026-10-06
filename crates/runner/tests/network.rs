use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use xc_core::read_request;
use xc_runner::{run_request, Outcome, Request, Session};

type Seen = Arc<Mutex<Vec<String>>>;

/// `/old` redirige vers `/new`, qui répond 200 : le serveur garde la première ligne de chaque requête.
fn serve() -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let head = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or_default().to_owned();
            log.lock().unwrap().push(head.clone());
            let reply = if head.contains("/old") {
                "HTTP/1.1 302 Found\r\nLocation: /new\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_owned()
            } else {
                "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnext".to_owned()
            };
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (base, seen)
}

async fn run(dir: &tempfile::TempDir, settings: &str) -> Outcome {
    fs::write(
        dir.path().join("r.yml"),
        format!("info:\n  name: R\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/old\"\n{settings}"),
    )
    .unwrap();
    let doc = read_request(dir.path(), "r.yml").unwrap();
    let req = Request {
        root: dir.path(),
        path: "r.yml",
        doc: &doc,
        env: None,
        collection_name: "S",
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    run_request(req, &mut Session::default()).await
}

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("opencollection.yml"),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: S\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n"),
    )
    .unwrap();
    dir
}

#[tokio::test]
async fn ef_req_04_a_request_follows_redirects_by_default() {
    let (base, seen) = serve();
    let outcome = run(&collection(&base), "").await;
    let response = outcome.response.expect("une réponse");
    assert_eq!((response.status, response.body.as_slice()), (200, b"next".as_slice()));
    assert_eq!(response.url, format!("{base}/new"));
    assert_eq!(response.redirects.len(), 1);
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn ef_req_04_the_followredirects_setting_of_the_file_switches_it_off() {
    let (base, seen) = serve();
    let outcome = run(&collection(&base), "\nsettings:\n  followRedirects: false\n").await;
    let response = outcome.response.expect("une réponse");
    assert_eq!((response.status, response.redirects.len()), (302, 0));
    assert_eq!(seen.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn ef_req_04_the_maxredirects_setting_of_the_file_bounds_them() {
    let (base, seen) = serve();
    let outcome = run(&collection(&base), "\nsettings:\n  maxRedirects: 0\n").await;
    assert_eq!(outcome.response.expect("une réponse").status, 302);
    assert_eq!(seen.lock().unwrap().len(), 1);
}
