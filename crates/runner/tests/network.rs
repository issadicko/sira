use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use xc_core::{read_request, NetworkPrefs, ProxyConfig, ProxyMode, ProxyPref};
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
    run_with(dir, settings, &mut Session::default()).await
}

async fn run_with(dir: &tempfile::TempDir, settings: &str, session: &mut Session) -> Outcome {
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
    run_request(req, session).await
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

fn through(base: &str) -> NetworkPrefs {
    let (host, port) = base.trim_start_matches("http://").split_once(':').unwrap();
    NetworkPrefs {
        proxy: ProxyPref {
            mode: ProxyMode::Manual,
            config: ProxyConfig { hostname: host.into(), port: port.into(), ..ProxyConfig::default() },
        },
        ..NetworkPrefs::default()
    }
}

#[tokio::test]
async fn ef_req_04_the_host_proxy_carries_every_hop_of_a_request() {
    let (proxy, seen) = serve();
    let dir = collection("http://cible.test");
    let mut session = Session { network: through(&proxy), ..Session::default() };

    let outcome = run_with(&dir, "", &mut session).await;

    assert_eq!(outcome.response.expect("une réponse").redirects.len(), 1);
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.iter().map(|l| l.to_lowercase()).collect::<Vec<_>>(),
        vec!["get http://cible.test/old http/1.1".to_owned(), "get http://cible.test/new http/1.1".to_owned(),]
    );
}

#[tokio::test]
async fn ef_req_04_requests_started_by_scripts_keep_the_host_network_settings() {
    let (proxy, seen) = serve();
    let dir = collection("http://cible.test");
    fs::write(
        dir.path().join("b.yml"),
        "info:\n  name: B\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: http://cible.test/b\n",
    )
    .unwrap();
    let script =
        "const axios = require('axios');\nawait bru.runRequest('b');\nawait axios.get('http://cible.test/s');\n";
    let indented = script.lines().map(|l| format!("        {l}\n")).collect::<String>();
    fs::write(
        dir.path().join("a.yml"),
        format!(
            "info:\n  name: A\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://cible.test/a\n\nruntime:\n  scripts:\n    - type: before-request\n      code: |-\n{indented}"
        ),
    )
    .unwrap();
    let doc = read_request(dir.path(), "a.yml").unwrap();
    let req = Request {
        root: dir.path(),
        path: "a.yml",
        doc: &doc,
        env: None,
        collection_name: "S",
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let mut session = Session { network: through(&proxy), ..Session::default() };

    let outcome = run_request(req, &mut session).await;

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let seen = seen.lock().unwrap();
    let mut lines: Vec<String> = seen.iter().map(|l| l.to_lowercase()).collect();
    lines.sort();
    assert_eq!(
        lines,
        vec![
            "get http://cible.test/a http/1.1".to_owned(),
            "get http://cible.test/b http/1.1".to_owned(),
            "get http://cible.test/s http/1.1".to_owned(),
        ]
    );
}
