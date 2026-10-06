use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::json;
use xc_core::{read_request, RequestDoc};
use xc_runner::{run_request, Outcome, Request, Session, Stage};
use xc_script::NextRequest;

type Seen = Arc<Mutex<Vec<String>>>;

/// Un serveur qui répond `{"name":"Ada","id":7}` et garde chaque requête reçue.
fn serve() -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            log.lock().unwrap().push(String::from_utf8_lossy(&buf[..n]).into_owned());
            let body = r#"{"name":"Ada","id":7}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Id: 7\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base, seen)
}

fn write(root: &Path, file: &str, text: &str) {
    let path = root.join(file);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(
        root,
        "opencollection.yml",
        &format!(
            "opencollection: 1.0.0\n\ninfo:\n  name: Shop\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n  scripts:\n    - type: before-request\n      code: bru.setVar('pre', (bru.getVar('pre') || '') + 'C');\n    - type: after-response\n      code: bru.setVar('post', (bru.getVar('post') || '') + 'C');\n"
        ),
    );
    write(
        root,
        "users/folder.yml",
        "info:\n  name: users\n  type: folder\n  seq: 1\n\nrequest:\n  scripts:\n    - type: before-request\n      code: bru.setVar('pre', (bru.getVar('pre') || '') + 'F');\n    - type: after-response\n      code: bru.setVar('post', (bru.getVar('post') || '') + 'F');\n",
    );
    dir
}

fn request(root: &Path, file: &str, text: &str) -> (String, RequestDoc) {
    write(root, file, text);
    (file.to_owned(), read_request(root, file).unwrap())
}

const SHOW: &str = "info:\n  name: Show user\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/users/{{id}}\"\n\nruntime:\n  scripts:\n";

async fn run(root: &Path, path: &str, doc: &RequestDoc, session: &mut Session) -> Outcome {
    let req = Request {
        root,
        path,
        doc,
        env: None,
        collection_name: "Shop",
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    run_request(req, session).await
}

#[tokio::test]
async fn ef_scr_01_scripts_run_collection_folder_request_before_and_the_reverse_after() {
    let (base, _) = serve();
    let dir = collection(&base);
    let (path, doc) = request(
        dir.path(),
        "users/show.yml",
        &format!(
            "{SHOW}    - type: before-request\n      code: bru.setVar('pre', bru.getVar('pre') + 'R'); bru.setVar('id', 7);\n    - type: after-response\n      code: bru.setVar('post', (bru.getVar('post') || '') + 'R');\n"
        ),
    );
    let mut session = Session::default();
    let out = run(dir.path(), &path, &doc, &mut session).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(session.runtime["pre"], "CFR");
    assert_eq!(session.runtime["post"], "RFC");
    assert!(out.url.ends_with("/users/7"), "{}", out.url);
    assert_eq!(out.response.as_ref().unwrap().status, 200);
}

#[tokio::test]
async fn ef_scr_01_a_pre_request_script_changes_what_is_sent() {
    let (base, seen) = serve();
    let dir = collection(&base);
    let (path, doc) = request(
        dir.path(),
        "users/show.yml",
        &format!(
            "{SHOW}    - type: before-request\n      code: |-\n        req.setUrl(req.getUrl() + '?via=script');\n        req.setHeader('X-Trace', bru.interpolate('{{{{id}}}}'));\n        bru.setVar('id', 42);\n"
        ),
    );
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    let request = seen.lock().unwrap().remove(0);
    assert!(request.starts_with("GET /users/42?via=script HTTP/1.1"), "{request}");
    assert!(
        request.to_lowercase().contains("x-trace: {{id}}") || request.to_lowercase().contains("x-trace: 42"),
        "{request}"
    );
}

#[tokio::test]
async fn ef_scr_01_an_error_in_the_pre_request_script_stops_before_sending() {
    let (base, seen) = serve();
    let dir = collection(&base);
    let (path, doc) = request(
        dir.path(),
        "users/show.yml",
        &format!("{SHOW}    - type: before-request\n      code: |-\n        bru.setVar('kept', 1);\n        throw new Error('no token');\n"),
    );
    let mut session = Session::default();
    let out = run(dir.path(), &path, &doc, &mut session).await;

    assert!(!out.passed());
    let error = out.error.expect("erreur");
    assert_eq!((error.stage, error.message.as_str()), (Stage::PreRequestScript, "no token"));
    assert!(seen.lock().unwrap().is_empty());
    assert_eq!(session.runtime["kept"], 1);
}

#[tokio::test]
async fn ef_scr_01_skip_request_and_runner_controls_come_back_to_the_caller() {
    let (base, seen) = serve();
    let dir = collection(&base);
    let (path, doc) = request(
        dir.path(),
        "users/show.yml",
        &format!(
            "{SHOW}    - type: before-request\n      code: bru.runner.skipRequest(); bru.setNextRequest('Other');\n"
        ),
    );
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;
    assert!(out.skipped && out.response.is_none() && out.error.is_none());
    assert_eq!(out.next_request, NextRequest::Named("Other".into()));
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ef_tst_01_tests_see_the_response_and_report_pass_and_fail() {
    let (base, _) = serve();
    let dir = collection(&base);
    let (path, doc) = request(
        dir.path(),
        "users/show.yml",
        &format!(
            "{SHOW}    - type: before-request\n      code: bru.setVar('id', 7);\n    - type: after-response\n      code: |-\n        res.setBody({{ name: 'Grace', id: res.body.id }});\n    - type: tests\n      code: |-\n        test('status', () => expect(res.status).to.equal(200));\n        test('body replaced by the post script', () => expect(res.body.name).to.equal('Grace'));\n        test('fails', () => expect(res.getHeader('x-id')).to.equal('8'));\n"
        ),
    );
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;
    let results: Vec<_> = out.tests.results.iter().map(|r| (r.description.as_str(), r.status.as_str())).collect();
    assert_eq!(results, [("fails", "fail"), ("status", "pass"), ("body replaced by the post script", "pass")]);
    assert!(!out.passed());
    let body = String::from_utf8(out.response.unwrap().body).unwrap();
    assert_eq!(serde_json::from_str::<serde_json::Value>(&body).unwrap(), json!({ "name": "Grace", "id": 7 }));
}

#[tokio::test]
async fn ef_scr_02_variables_written_by_a_script_carry_over_to_the_next_request() {
    let (base, _) = serve();
    let dir = collection(&base);
    let (login_path, login) = request(
        dir.path(),
        "users/login.yml",
        &format!("{SHOW}    - type: before-request\n      code: bru.setVar('id', 7);\n    - type: after-response\n      code: bru.setVar('token', res.body.name); bru.setEnvVar('seen', 'yes'); bru.setGlobalEnvVar('g', 1);\n"),
    );
    let (next_path, next) = request(
        dir.path(),
        "users/next.yml",
        &format!("{SHOW}    - type: before-request\n      code: bru.setVar('id', 1); bru.setVar('saw', [bru.getVar('token'), bru.getEnvVar('seen'), bru.getGlobalEnvVar('g')]);\n"),
    );
    let mut session = Session::default();
    run(dir.path(), &login_path, &login, &mut session).await;
    let out = run(dir.path(), &next_path, &next, &mut session).await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(session.runtime["saw"], json!(["Ada", "yes", 1]));
}

#[tokio::test]
async fn ef_scr_02_a_connection_failure_reports_the_send_stage_without_running_after_scripts() {
    let dir = collection("http://127.0.0.1:9");
    let (path, doc) = request(
        dir.path(),
        "users/show.yml",
        &format!("{SHOW}    - type: before-request\n      code: bru.setVar('id', 1);\n    - type: tests\n      code: test('never', () => {{}});\n"),
    );
    let out = run(dir.path(), &path, &doc, &mut Session::default()).await;
    assert_eq!(out.error.as_ref().map(|e| e.stage), Some(Stage::Send));
    assert!(out.tests.results.is_empty());
}

#[tokio::test]
async fn ef_var_03_post_response_variables_are_set_before_the_post_script_and_reach_the_next_request() {
    let (base, _) = serve();
    let dir = collection(&base);
    let (login_path, login) = request(
        dir.path(),
        "users/login.yml",
        &format!(
            "{SHOW}    - type: before-request\n      code: bru.setVar('id', 7);\n    - type: after-response\n      code: bru.setVar('seen_by_script', bru.getVar('user-name'));\n  actions:\n    - type: set-variable\n      phase: after-response\n      selector:\n        expression: res.body.name\n        method: jsonq\n      variable:\n        name: user-name\n        scope: runtime\n    - type: set-variable\n      phase: after-response\n      selector:\n        expression: res.body.nope.deeper\n        method: jsonq\n      variable:\n        name: broken\n        scope: runtime\n    - type: set-variable\n      phase: after-response\n      selector:\n        expression: res.status\n        method: jsonq\n      variable:\n        name: off\n        scope: runtime\n      disabled: true\n"
        ),
    );
    let mut session = Session::default();
    let out = run(dir.path(), &login_path, &login, &mut session).await;
    assert!(out.error.is_none() && out.post.error.is_none(), "{:?} {:?}", out.error, out.post.error);
    assert_eq!(session.runtime["user-name"], "Ada");
    assert_eq!(session.runtime["seen_by_script"], "Ada");
    assert_eq!(
        session.runtime["broken"]["name"], "TypeError",
        "une expression qui échoue donne son erreur à la variable"
    );
    assert!(!session.runtime.contains_key("off"));
}

#[tokio::test]
async fn ef_scr_02_run_request_runs_another_request_with_the_callers_variables() {
    let (base, seen) = serve();
    let dir = collection(&base);
    let (_, _) = request(
        dir.path(),
        "auth/token.yml",
        &format!(
            "{}    - type: before-request\n      code: bru.setVar('id', 'token');\n    - type: after-response\n      code: bru.setVar('token', res.body.name); bru.setEnvVar('from-nested', 'yes');\n",
            SHOW.replace("users", "auth")
        ),
    );
    let (path, doc) = request(
        dir.path(),
        "users/show.yml",
        &format!(
            "{SHOW}    - type: before-request\n      code: |-\n        const reply = await bru.runRequest('auth/token');\n        const missing = await bru.runRequest('auth/none');\n        bru.setVar('id', 1);\n        bru.setVar('reply', [reply.status, reply.data.name, bru.getVar('token'), bru.getEnvVar('from-nested'), missing]);\n"
        ),
    );
    let mut session = Session::default();
    let out = run(dir.path(), &path, &doc, &mut session).await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(session.runtime["reply"], json!([200, "Ada", "Ada", "yes", {}]));
    assert_eq!(session.runtime["token"], "Ada");
    let requests = seen.lock().unwrap();
    assert!(requests[0].starts_with("GET /auth/token") && requests[1].starts_with("GET /users/1"), "{requests:?}");
}

#[tokio::test]
async fn ef_scr_03_scripts_send_requests_with_axios_and_bru_send_request() {
    let (base, seen) = serve();
    let dir = collection(&base);
    let (path, doc) = request(
        dir.path(),
        "users/show.yml",
        &format!(
            "{SHOW}    - type: before-request\n      code: |-\n        const axios = require('axios');\n        bru.setVar('id', 1);\n        const a = await axios.post(bru.getCollectionVar('baseUrl') + '/echo', {{ n: 1 }}, {{ headers: {{ 'X-Key': 'k' }} }});\n        const b = await bru.sendRequest({{ method: 'GET', url: bru.getCollectionVar('baseUrl') + '/search', params: {{ q: 'a b' }} }});\n        let gone;\n        try {{ await axios.get('http://127.0.0.1:9/nope'); }} catch (e) {{ gone = [e.isAxiosError, e.code]; }}\n        let viaCallback;\n        await bru.sendRequest(bru.getCollectionVar('baseUrl') + '/cb', (err, res) => {{ viaCallback = [err, res.status]; }});\n        bru.setVar('seen', [a.status, a.data.name, b.status, gone, viaCallback]);\n"
        ),
    );
    let mut session = Session::default();
    let out = run(dir.path(), &path, &doc, &mut session).await;
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(session.runtime["seen"], json!([200, "Ada", 200, [true, "ECONNREFUSED"], [null, 200]]));
    let requests = seen.lock().unwrap();
    assert!(requests[0].starts_with("POST /echo"), "{requests:?}");
    assert!(requests[0].contains(r#"{"n":1}"#) && requests[0].to_lowercase().contains("x-key: k"), "{}", requests[0]);
    assert!(requests[1].starts_with("GET /search?q=a+b"), "{requests:?}");
}
