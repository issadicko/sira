use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::mpsc;
use std::thread;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Un serveur qui répond « ok » à `connections` connexions et envoie la première ligne de chaque requête.
fn serve(connections: usize) -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for stream in listener.incoming().take(connections) {
            let mut stream = stream.unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            sender.send(request.lines().next().unwrap_or_default().to_owned()).ok();
            write!(stream, "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok").unwrap();
        }
    });
    (port, receiver)
}

fn collection(proxy_port: u16, origin_port: u16) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "opencollection.yml",
        &format!(
            "opencollection: 1.0.0\n\ninfo:\n  name: Réseau\nconfig:\n  proxy:\n    inherit: false\n    config:\n      protocol: http\n      hostname: 127.0.0.1\n      port: {proxy_port}\nbundled: false\nextensions: {{}}\n"
        ),
    );
    write(
        root,
        "ping.yml",
        &format!("info:\n  name: Ping\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://127.0.0.1:{origin_port}/ping\n"),
    );
    dir
}

fn xc(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xc")).args(args).output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn ef_req_04_run_goes_through_the_collection_proxy() {
    let (proxy, proxy_seen) = serve(1);
    let dir = collection(proxy, 9);

    let (code, out) = xc(&["run", dir.path().to_str().unwrap()]);

    assert_eq!(code, 0, "{out}");
    assert_eq!(proxy_seen.recv().unwrap().to_lowercase(), "get http://127.0.0.1:9/ping http/1.1");
}

#[test]
fn ef_req_04_noproxy_ignores_the_collection_proxy() {
    let (proxy, proxy_seen) = serve(1);
    let (origin, origin_seen) = serve(1);
    let dir = collection(proxy, origin);

    let (code, out) = xc(&["run", dir.path().to_str().unwrap(), "--noproxy"]);

    assert_eq!(code, 0, "{out}");
    assert_eq!(origin_seen.recv().unwrap().to_lowercase(), "get /ping http/1.1");
    assert!(proxy_seen.try_recv().is_err(), "le proxy n'a rien reçu");
}

/// Un serveur qui pose un cookie à la première requête et garde les deux premières lignes de requêtes reçues.
fn serve_cookie() -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        for (i, stream) in listener.incoming().take(2).enumerate() {
            let mut stream = stream.unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let cookie = String::from_utf8_lossy(&buf[..n])
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("cookie:"))
                .unwrap_or("cookie: (aucun)")
                .to_ascii_lowercase();
            sender.send(cookie).ok();
            let set = if i == 0 { "set-cookie: sid=abc; Path=/\r\n" } else { "" };
            write!(stream, "HTTP/1.1 200 OK\r\n{set}content-length: 2\r\nconnection: close\r\n\r\nok").unwrap();
        }
    });
    (port, receiver)
}

fn two_requests(origin: u16) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "opencollection.yml",
        "opencollection: 1.0.0\n\ninfo:\n  name: Cookies\nbundled: false\nextensions: {}\n",
    );
    for (name, seq) in [("a", 1), ("b", 2)] {
        write(
            root,
            &format!("{name}.yml"),
            &format!("info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: http://127.0.0.1:{origin}/{name}\n"),
        );
    }
    dir
}

#[test]
fn ef_ux_02_run_carries_a_cookie_from_one_request_to_the_next() {
    let (origin, seen) = serve_cookie();
    let dir = two_requests(origin);

    let (code, out) = xc(&["run", dir.path().to_str().unwrap()]);

    assert_eq!(code, 0, "{out}");
    assert_eq!(seen.recv().unwrap(), "cookie: (aucun)");
    assert_eq!(seen.recv().unwrap(), "cookie: sid=abc");
}

#[test]
fn ef_ux_02_disable_cookies_keeps_the_jar_empty() {
    let (origin, seen) = serve_cookie();
    let dir = two_requests(origin);

    let (code, out) = xc(&["run", dir.path().to_str().unwrap(), "--disable-cookies"]);

    assert_eq!(code, 0, "{out}");
    assert_eq!(seen.recv().unwrap(), "cookie: (aucun)");
    assert_eq!(seen.recv().unwrap(), "cookie: (aucun)");
}
