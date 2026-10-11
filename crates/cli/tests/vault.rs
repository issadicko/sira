use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn serve() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let (status, body) = if raw.starts_with("GET /v1/secret/data/app ") {
                (200, r#"{"data":{"data":{"cle":"valeur-vault"}}}"#)
            } else {
                (200, r#"{"ok":true}"#)
            };
            log.lock().unwrap().push(raw);
            let _ = write!(stream, "HTTP/1.1 {status} OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
        }
    });
    (base, seen)
}

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "opencollection.yml", "opencollection: 1.0.0\n\ninfo:\n  name: V\nbundled: false\nextensions: {}\n");
    write(root, "environments/local.yml", &format!("name: local\nvariables:\n  - name: baseUrl\n    value: {base}\n"));
    write(
        root,
        "r.yml",
        "info:\n  name: R\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/ping\"\n  headers:\n    - name: X-Cle\n      value: \"{{$vault.secret/app#cle}}\"\n\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n",
    );
    dir
}

fn run(dir: &tempfile::TempDir, env: &[(&str, &str)]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xc"))
        .args(["run", dir.path().to_str().unwrap(), "r.yml", "--env", "local", "--noproxy"])
        .env_remove("VAULT_ADDR")
        .env_remove("VAULT_TOKEN")
        .env("HOME", dir.path())
        .envs(env.iter().copied())
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
    )
}

#[test]
fn ef_var_03_xc_run_reads_vault_variables_with_the_standard_vault_environment() {
    let (base, seen) = serve();
    let dir = collection(&base);

    let (code, output) = run(&dir, &[("VAULT_ADDR", &base), ("VAULT_TOKEN", "s.jeton")]);

    assert_eq!(code, 0, "{output}");
    let log = seen.lock().unwrap();
    let ping = log.iter().find(|r| r.starts_with("GET /ping")).expect("la requête part");
    assert!(ping.to_lowercase().contains("x-cle: valeur-vault"), "{ping}");
    let read = log.iter().find(|r| r.starts_with("GET /v1/secret/data/app")).expect("Vault est lu");
    assert!(read.to_lowercase().contains("x-vault-token: s.jeton"), "{read}");
}

#[test]
fn ef_var_03_xc_run_uses_the_vault_token_file_and_fails_clearly_without_vault() {
    let (base, _) = serve();
    let dir = collection(&base);
    write(dir.path(), ".vault-token", "s.fichier\n");

    let (code, output) = run(&dir, &[("VAULT_ADDR", &base)]);
    assert_eq!(code, 0, "{output}");

    let (code, output) = run(&dir, &[]);
    assert_ne!(code, 0);
    assert!(output.contains("Vault n'est pas configuré"), "{output}");
}
