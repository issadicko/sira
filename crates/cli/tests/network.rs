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
