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
    assert!(out.contains("res.body.devise eq EUR  expected 'XOF' to equal 'EUR'"), "{out}");
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

fn scripted_collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "opencollection.yml",
        &format!("opencollection: 1.0.0\n\ninfo:\n  name: E2E\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n"),
    );
    let request = |name: &str, seq: u32, scripts: &str| {
        format!("info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/{name}\"\n\nruntime:\n  scripts:\n{scripts}")
    };
    write(
        root,
        "first.yml",
        &request(
            "first",
            1,
            "    - type: before-request\n      code: console.log('about to send'); bru.setVar('who', 'Ada');\n    - type: after-response\n      code: bru.setNextRequest('third');\n    - type: tests\n      code: |-\n        test('devise', () => expect(res.body.devise).to.equal('XOF'));\n        test('who survives', () => expect(bru.getVar('who')).to.equal('Ada'));\n",
        ),
    );
    write(root, "second.yml", &request("second", 2, "    - type: tests\n      code: test('never runs', () => {});\n"));
    write(
        root,
        "third.yml",
        &request(
            "third",
            3,
            "    - type: tests\n      code: |-\n        test('wrong on purpose', () => expect(1).to.equal(2));\n",
        ),
    );
    dir
}

#[test]
fn ef_scr_01_run_executes_scripts_and_tests_and_follows_set_next_request() {
    let base = serve(2);
    let dir = scripted_collection(&base);
    let (code, out) = xc(&["run", dir.path().to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("[log] about to send"), "{out}");
    assert!(out.contains("✓ devise") && out.contains("✓ who survives"), "{out}");
    assert!(out.contains("✗ wrong on purpose  expected 1 to equal 2"), "{out}");
    assert!(!out.contains("never runs") && !out.contains("second"), "{out}");
    assert!(out.contains("1 réussie(s), 1 en échec"), "{out}");
}

fn xc_both(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xc")).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// `a` échoue (un test), `b` réussit ; chacune interroge `/nom` et le serveur répond 200.
fn failing_pair(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "opencollection.yml",
        &format!("opencollection: 1.0.0\n\ninfo:\n  name: E2E\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n"),
    );
    let request = |name: &str, seq: u32, test: &str| {
        format!("info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/{name}\"\n\nruntime:\n  scripts:\n    - type: tests\n      code: {test}\n")
    };
    write(root, "a.yml", &request("a", 1, "test('fails on purpose', () => expect(1).to.equal(2));"));
    write(root, "b.yml", &request("b", 2, "test('passes', () => expect(res.status).to.equal(200));"));
    dir
}

#[test]
fn ef_run_01_bail_skips_what_follows_a_failure() {
    let (code, out) = xc(&["run", failing_pair(&serve(2)).path().to_str().unwrap()]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("1 réussie(s), 1 en échec, 2 au total"), "{out}");

    let dir = failing_pair(&serve(2));
    let (code, out) = xc(&["run", dir.path().to_str().unwrap(), "--bail"]);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("0 réussie(s), 1 en échec, 1 ignorée(s), 2 au total"), "{out}");
    assert!(out.contains("Arrêt au premier échec : test failure dans « a »"), "{out}");
    assert!(!out.contains("✓ passes"), "{out}");
}

#[test]
fn ef_run_01_delay_waits_between_requests() {
    let dir = failing_pair(&serve(2));
    let started = std::time::Instant::now();
    let (_, out) = xc(&["run", dir.path().to_str().unwrap(), "--delay", "300"]);
    assert!(started.elapsed() >= std::time::Duration::from_millis(300), "{out}");
    assert!(out.contains("attente de 300 ms"), "{out}");
}

fn iteration_collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "opencollection.yml",
        &format!("opencollection: 1.0.0\n\ninfo:\n  name: E2E\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n"),
    );
    write(
        root,
        "user.yml",
        "info:\n  name: user\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/users/{{name}}\"\n\nruntime:\n  scripts:\n    - type: tests\n      code: test('url follows the row', () => expect(res.body.ligne).to.contain('/users/' + bru.getVar('name') + ' '));\n",
    );
    dir
}

#[test]
fn ef_run_02_a_csv_or_json_file_drives_one_iteration_per_row() {
    let dir = iteration_collection(&serve(4));
    let (root, data) = (dir.path(), tempfile::tempdir().unwrap());
    let csv = data.path().join("rows.csv");
    fs::write(&csv, "name\nada\ngrace\n").unwrap();
    let (code, out) = xc(&["run", root.to_str().unwrap(), "--data", csv.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("itération 1/2") && out.contains("itération 2/2"), "{out}");
    assert_eq!(out.matches("✓ url follows the row").count(), 2, "{out}");

    let json = data.path().join("rows.json");
    fs::write(&json, r#"[{"name": "ada"}, {"name": "grace"}]"#).unwrap();
    let (code, out) = xc(&["run", root.to_str().unwrap(), "--data", json.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("2 réussie(s), 0 en échec, 2 au total"), "{out}");
}

#[test]
fn ef_cli_02_run_writes_json_junit_and_html_reports() {
    let dir = failing_pair(&serve(2));
    let out_dir = tempfile::tempdir().unwrap();
    let (json, xml, html) =
        (out_dir.path().join("r.json"), out_dir.path().join("r.xml"), out_dir.path().join("r.html"));
    let (code, out) = xc(&[
        "run",
        dir.path().to_str().unwrap(),
        "--reporter-json",
        json.to_str().unwrap(),
        "--reporter-junit",
        xml.to_str().unwrap(),
        "--reporter-html",
        html.to_str().unwrap(),
    ]);
    assert_eq!(code, 1, "{out}");

    let doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(doc["summary"]["totalRequests"], 2);
    assert_eq!(doc["summary"]["failedRequests"], 1);
    assert_eq!(doc["results"][0]["testResults"][0]["description"], "fails on purpose");
    let xml = fs::read_to_string(&xml).unwrap();
    assert!(xml.contains("<testsuite name=\"a\" file=\"a.yml\" errors=\"0\" failures=\"1\""), "{xml}");
    assert!(xml.contains("<testsuite name=\"b\" file=\"b.yml\" errors=\"0\" failures=\"0\""), "{xml}");
    let html = fs::read_to_string(&html).unwrap();
    assert!(html.contains("✗ Échec") && html.contains("fails on purpose"), "{html}");
}

#[test]
fn ef_cli_02_output_with_format_writes_one_report_and_the_skip_options_trim_it() {
    let dir = failing_pair(&serve(2));
    let out_dir = tempfile::tempdir().unwrap();
    let junit = out_dir.path().join("r.xml");
    let (_, out) = xc(&["run", dir.path().to_str().unwrap(), "-o", junit.to_str().unwrap(), "-f", "junit"]);
    assert!(out.contains("Résultats écrits dans"), "{out}");
    assert!(fs::read_to_string(&junit).unwrap().starts_with("<?xml"));

    let dir = failing_pair(&serve(2));
    let json = out_dir.path().join("r.json");
    xc(&[
        "run",
        dir.path().to_str().unwrap(),
        "-o",
        json.to_str().unwrap(),
        "--reporter-skip-all-headers",
        "--reporter-skip-body",
    ]);
    let doc: serde_json::Value = serde_json::from_str(&fs::read_to_string(&json).unwrap()).unwrap();
    assert_eq!(doc["results"][0]["response"]["headers"], serde_json::json!({}));
    assert!(doc["results"][0]["response"].get("data").is_none());
}

#[test]
fn ef_cli_01_input_errors_exit_with_two_and_say_why() {
    let dir = collection("http://127.0.0.1:9", "XOF");
    let root = dir.path().to_str().unwrap();
    let (code, _, err) = xc_both(&["run", root, "--env", "nope"]);
    assert_eq!((code, err.contains("« nope » introuvable (disponibles : local)")), (2, true), "{err}");
    let (code, _, err) = xc_both(&["run", root, "ghost"]);
    assert_eq!((code, err.contains("aucune requête ni aucun dossier à ghost")), (2, true), "{err}");
    let (code, _, err) = xc_both(&["run", root, "-o", "/nonexistent-dir/r.json"]);
    assert_eq!((code, err.contains("n'existe pas")), (2, true), "{err}");
    let (code, _, err) = xc_both(&["run", root, "--data", "/nonexistent.csv"]);
    assert_eq!((code, err.contains("lecture de")), (2, true), "{err}");
}
