use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::thread;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn serve(responses: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        for stream in listener.incoming().take(responses) {
            let mut stream = stream.unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            let request = String::from_utf8_lossy(&buf[..n]).to_string();
            let line = request.lines().next().unwrap_or_default().to_owned();
            let body = format!(r#"{{"ligne":"{line}","devise":"XOF"}}"#);
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });
    format!("http://{addr}")
}

fn collection(base: &str, expected_devise: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "opencollection.yml", "opencollection: 1.0.0\n\ninfo:\n  name: E2E\nbundled: false\nextensions: {}\n");
    write(root, "environments/local.yml", &format!("name: local\nvariables:\n  - name: baseUrl\n    value: {base}\n"));
    write(
        root,
        "tx/detail.yml",
        &format!(
            "info:\n  name: Détail\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/transactions/{{{{txId}}}}\"\n\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n    - expression: res.body.devise\n      operator: eq\n      value: {expected_devise}\n"
        ),
    );
    dir
}

fn xc(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xc")).args(args).output().unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned())
}

#[test]
fn ef_cli_01_run_passes_with_env_and_env_var() {
    let base = serve(1);
    let dir = collection(&base, "XOF");
    let root = dir.path().to_str().unwrap();
    let (code, out) = xc(&["run", root, "--env", "local", "--env-var", "txId=TX-9"]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("✓ GET Détail  200 OK"), "{out}");
    assert!(out.contains("1 réussie(s), 0 en échec"), "{out}");
}

#[test]
fn ef_cli_01_failed_assertion_gives_non_zero_exit() {
    let base = serve(1);
    let dir = collection(&base, "EUR");
    let (code, out) = xc(&["run", dir.path().to_str().unwrap(), "tx", "--env", "local", "--env-var", "txId=TX-9"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("res.body.devise eq EUR  (reçu \"XOF\")"), "{out}");
}

#[test]
fn ef_cli_01_unknown_collection_exits_with_two() {
    let dir = tempfile::tempdir().unwrap();
    let (code, _) = xc(&["run", dir.path().to_str().unwrap()]);
    assert_eq!(code, 2);
}

#[test]
fn enf_comp_02_check_reports_identical_files() {
    let dir = collection("http://127.0.0.1:9", "XOF");
    let (code, out) = xc(&["check", dir.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("3 identique(s)"), "{out}");
}
