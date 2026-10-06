use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use xc_core::open_collection;
use xc_runner::{
    html_page, json as json_report, junit, read_rows, run_collection, select, Event, Halt, Job, Meta, Redact, Row,
    SelectError, Session, Skip, MAX_JUMPS,
};

type Seen = Arc<Mutex<Vec<String>>>;

/// Un serveur qui répond 200 avec un JSON, sauf `/fail` (500) ; il garde la première ligne de chaque requête.
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
            let head = String::from_utf8_lossy(&buf[..n]);
            let line = head.lines().next().unwrap_or_default().to_owned();
            log.lock().unwrap().push(line.clone());
            let status = if line.contains("/fail") { "500 Internal Server Error" } else { "200 OK" };
            let body = r#"{"ok":true}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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

/// `scripts` : des blocs `(type, code)` ajoutés au fichier.
fn request(root: &Path, file: &str, name: &str, seq: u32, url: &str, scripts: &[(&str, &str)]) {
    let mut text =
        format!("info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: GET\n  url: \"{url}\"\n");
    if !scripts.is_empty() {
        text.push_str("\nruntime:\n  scripts:\n");
        for (kind, code) in scripts {
            text.push_str(&format!("    - type: {kind}\n      code: |-\n"));
            for line in code.lines() {
                text.push_str(&format!("        {line}\n"));
            }
        }
    }
    write(root, file, &text);
}

fn new_collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "opencollection.yml",
        &format!(
            "opencollection: 1.0.0\n\ninfo:\n  name: Shop\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n"
        ),
    );
    dir
}

const PASS: (&str, &str) = ("tests", "test('ok', () => expect(res.status).to.equal(200));");

struct Run {
    report: xc_runner::RunReport,
    events: Vec<String>,
    session: Session,
}

async fn go(root: &Path, targets: &[&str], rows: &[Row], bail: bool, delay: Duration) -> Run {
    go_limited(root, targets, rows, bail, delay, MAX_JUMPS).await
}

async fn go_limited(root: &Path, targets: &[&str], rows: &[Row], bail: bool, delay: Duration, max_jumps: usize) -> Run {
    let collection = open_collection(root).unwrap();
    let targets: Vec<String> = targets.iter().map(|t| (*t).to_owned()).collect();
    let items = select(&collection.items, &targets).unwrap();
    let job = Job {
        root,
        collection_name: &collection.name,
        env: None,
        items: &items,
        rows,
        bail,
        delay,
        max_jumps,
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let mut events = Vec::new();
    let mut session = Session::default();
    let report = run_collection(job, &mut session, &mut |event| {
        events.push(match event {
            Event::Iteration { index, total, .. } => format!("iteration {}/{total}", index + 1),
            Event::Started { item, .. } => format!("start {}", item.name),
            Event::Finished(result) => format!("done {}", result.name),
            Event::Waiting(delay) => format!("wait {}", delay.as_millis()),
            Event::Warning(warning) => format!("warn {warning}"),
        });
    })
    .await;
    Run { report, events, session }
}

fn names(run: &Run) -> Vec<String> {
    run.report.results.iter().map(|r| r.name.clone()).collect()
}

fn three(base: &str) -> tempfile::TempDir {
    let dir = new_collection(base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[PASS]);
    request(dir.path(), "sub/b.yml", "B", 1, "{{baseUrl}}/b", &[PASS]);
    request(dir.path(), "sub/c.yml", "C", 2, "{{baseUrl}}/c", &[PASS]);
    write(dir.path(), "sub/folder.yml", "info:\n  name: sub\n  type: folder\n  seq: 2\n");
    dir
}

#[tokio::test]
async fn ef_run_01_a_collection_runs_every_request_in_order_and_reports_a_summary() {
    let (base, seen) = serve();
    let dir = three(&base);
    let run = go(dir.path(), &[], &[], false, Duration::ZERO).await;

    assert_eq!(names(&run), ["A", "B", "C"]);
    assert_eq!(seen.lock().unwrap().len(), 3);
    let s = run.report.summary();
    assert_eq!((s.total_requests, s.passed_requests, s.total_tests, s.passed_tests), (3, 3, 3, 3));
    assert!(!run.report.failed());
    assert!(run.report.halt.is_none());
    assert_eq!(run.events[0], "iteration 1/1");
    assert_eq!(&run.events[1..3], ["start A", "done A"]);
}

#[tokio::test]
async fn ef_run_01_a_folder_or_a_single_request_can_be_targeted() {
    let (base, _) = serve();
    let dir = three(&base);
    assert_eq!(names(&go(dir.path(), &["sub"], &[], false, Duration::ZERO).await), ["B", "C"]);
    assert_eq!(names(&go(dir.path(), &["sub/c.yml"], &[], false, Duration::ZERO).await), ["C"]);
    assert_eq!(names(&go(dir.path(), &["sub/c"], &[], false, Duration::ZERO).await), ["C"]);
    assert_eq!(names(&go(dir.path(), &["sub/c.yml", "a.yml"], &[], false, Duration::ZERO).await), ["C", "A"]);

    let collection = open_collection(dir.path()).unwrap();
    assert_eq!(select(&collection.items, &["nope".into()]), Err(SelectError::NotFound("nope".into())));
}

#[tokio::test]
async fn ef_run_01_the_delay_separates_requests_but_does_not_follow_the_last_one() {
    let (base, _) = serve();
    let dir = three(&base);
    let started = Instant::now();
    let run = go(dir.path(), &[], &[], false, Duration::from_millis(150)).await;

    assert!(started.elapsed() >= Duration::from_millis(300), "deux attentes de 150 ms : {:?}", started.elapsed());
    assert_eq!(run.events.iter().filter(|e| e.starts_with("wait")).count(), 2);
    assert_eq!(run.events.last().unwrap(), "done C");
}

#[tokio::test]
async fn ef_run_01_bail_stops_at_the_first_failure_and_marks_the_rest_skipped() {
    let (base, seen) = serve();
    let dir = new_collection(&base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[PASS]);
    request(dir.path(), "b.yml", "B", 2, "{{baseUrl}}/b", &[("tests", "test('boom', () => expect(1).to.equal(2));")]);
    request(dir.path(), "c.yml", "C", 3, "{{baseUrl}}/c", &[PASS]);
    request(dir.path(), "d.yml", "D", 4, "{{baseUrl}}/d", &[PASS]);
    let run = go(dir.path(), &[], &[], true, Duration::ZERO).await;

    assert_eq!(names(&run), ["A", "B", "C", "D"]);
    assert_eq!(seen.lock().unwrap().len(), 2, "C et D ne sont pas envoyées");
    assert_eq!(run.report.results[2].skip, Some(Skip::Bail));
    assert_eq!(run.report.halt, Some(Halt::Bail { request: "B".into(), reason: "test failure", remaining: 2 }));
    let s = run.report.summary();
    assert_eq!((s.passed_requests, s.failed_requests, s.skipped_requests, s.skipped_by_bail), (1, 1, 2, 2));
    assert!(run.report.failed());

    let without = go(dir.path(), &[], &[], false, Duration::ZERO).await;
    assert_eq!(without.report.summary().skipped_requests, 0);
    assert_eq!(without.report.summary().failed_requests, 1);
}

#[tokio::test]
async fn ef_run_01_bail_names_the_most_specific_reason() {
    let (base, _) = serve();
    let dir = new_collection(&base);
    write(
        dir.path(),
        "a.yml",
        &format!(
            "info:\n  name: A\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{base}/a\"\n\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"201\"\n"
        ),
    );
    request(dir.path(), "b.yml", "B", 2, "{{baseUrl}}/b", &[PASS]);
    let run = go(dir.path(), &[], &[], true, Duration::ZERO).await;
    assert_eq!(run.report.halt, Some(Halt::Bail { request: "A".into(), reason: "assertion failure", remaining: 1 }));

    let broken = new_collection("http://127.0.0.1:1");
    request(broken.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[]);
    let run = go(broken.path(), &[], &[], true, Duration::ZERO).await;
    assert_eq!(run.report.halt, Some(Halt::Bail { request: "A".into(), reason: "request failure", remaining: 0 }));
}

#[tokio::test]
async fn ef_run_02_each_data_row_is_an_iteration_whose_fields_are_variables() {
    let (base, seen) = serve();
    let dir = new_collection(&base);
    request(
        dir.path(),
        "a.yml",
        "A",
        1,
        "{{baseUrl}}/items/{{id}}",
        &[(
            "tests",
            "test('id is typed', () => expect(bru.getVar('id')).to.be.ok);\nbru.setVar('last', bru.getVar('id'));",
        )],
    );
    let rows: Vec<Row> = [json!({ "id": 1 }), json!({ "id": "two" }), json!({ "id": 3 })]
        .into_iter()
        .map(|v| v.as_object().cloned().unwrap())
        .collect();
    let run = go(dir.path(), &[], &rows, false, Duration::ZERO).await;

    assert_eq!(names(&run), ["A", "A", "A"]);
    let lines = seen.lock().unwrap().clone();
    assert!(
        lines[0].contains("/items/1 ") && lines[1].contains("/items/two ") && lines[2].contains("/items/3 "),
        "{lines:?}"
    );
    assert_eq!(run.report.iterations.len(), 3);
    assert_eq!(run.report.results.iter().map(|r| r.iteration).collect::<Vec<_>>(), [0, 1, 2]);
    assert_eq!(run.session.runtime["last"], 3, "les variables survivent aux itérations, avec leur type");
    assert_eq!(&run.events[..2], ["iteration 1/3", "start A"]);
    assert!(run.events.contains(&"iteration 3/3".to_owned()));

    let doc: Value = serde_json::from_str(&json_report(&run.report, &Redact::default())).unwrap();
    assert_eq!(doc["iterations"][1]["iterationData"], json!({ "id": "two" }));
    assert_eq!(doc["results"][2]["iterationIndex"], 2);
}

#[tokio::test]
async fn ef_run_02_rows_come_from_a_csv_file() {
    let (base, seen) = serve();
    let dir = new_collection(&base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/users/{{name}}", &[PASS]);
    write(dir.path(), "users.csv", "name,role\nada,admin\n\"grace hopper\",dev\n");
    let rows = read_rows(&dir.path().join("users.csv")).unwrap();
    let run = go(dir.path(), &[], &rows, false, Duration::ZERO).await;

    assert_eq!(run.report.results.len(), 2);
    let lines = seen.lock().unwrap().clone();
    assert!(lines[0].contains("/users/ada ") && lines[1].contains("/users/grace%20hopper "), "{lines:?}");
}

#[tokio::test]
async fn ef_run_02_set_next_request_jumps_and_stop_execution_ends_the_run() {
    let (base, _) = serve();
    let dir = new_collection(&base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[("after-response", "bru.setNextRequest('C');")]);
    request(dir.path(), "b.yml", "B", 2, "{{baseUrl}}/b", &[PASS]);
    request(dir.path(), "c.yml", "C", 3, "{{baseUrl}}/c", &[("after-response", "bru.runner.stopExecution();")]);
    request(dir.path(), "d.yml", "D", 4, "{{baseUrl}}/d", &[PASS]);
    let run = go(dir.path(), &[], &[], false, Duration::ZERO).await;

    assert_eq!(names(&run), ["A", "C", "D"], "B est sautée, D ignorée par l'arrêt");
    assert_eq!(run.report.results[2].skip, Some(Skip::StopExecution));
    assert_eq!(run.report.halt, Some(Halt::StopExecution { request: "C".into(), remaining: 1 }));
    let entries = run.report.entries();
    assert_eq!(entries[2].skip_reason.as_deref(), Some("stopExecution"));
    assert_eq!(entries[2].status, "skipped");
}

#[tokio::test]
async fn ef_run_02_a_request_that_keeps_jumping_to_itself_is_cut_as_a_loop() {
    let (base, _) = serve();
    let dir = new_collection(&base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[("after-response", "bru.setNextRequest('A');")]);
    let run = go_limited(dir.path(), &[], &[], false, Duration::ZERO, 20).await;

    assert_eq!(run.report.results.len(), 21, "la 21e requête déclenche la coupure");
    assert_eq!(run.report.halt, Some(Halt::Loop));
    assert!(run.report.failed(), "une boucle sans fin est un échec");
    assert!(run.events.iter().any(|e| e.contains("Too many jumps")));
}

#[tokio::test]
async fn ef_run_01_a_script_can_skip_a_request() {
    let (base, seen) = serve();
    let dir = new_collection(&base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[("before-request", "bru.runner.skipRequest();")]);
    request(dir.path(), "b.yml", "B", 2, "{{baseUrl}}/b", &[PASS]);
    let run = go(dir.path(), &[], &[], false, Duration::ZERO).await;

    assert_eq!(run.report.results[0].skip, Some(Skip::Script));
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(run.report.summary().skipped_requests, 1);
    assert!(!run.report.failed());
}

#[tokio::test]
async fn ef_run_01_a_request_with_prompt_variables_is_skipped_like_bru_run() {
    let (base, seen) = serve();
    let dir = new_collection(&base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a?code={{?Code}}&who={{?Who am I}}", &[PASS]);
    request(dir.path(), "b.yml", "B", 2, "{{baseUrl}}/b", &[PASS]);
    let run = go(dir.path(), &[], &[], false, Duration::ZERO).await;

    assert_eq!(run.report.results[0].skip, Some(Skip::Prompts(vec!["Code".into(), "Who am I".into()])));
    assert_eq!(run.report.results[1].skip, None);
    assert_eq!(seen.lock().unwrap().len(), 1, "seule B part");
    assert_eq!(run.report.summary().skipped_requests, 1);
    assert!(!run.report.failed());

    let report: Value = serde_json::from_str(&xc_runner::json(&run.report, &xc_runner::Redact::default())).unwrap();
    let skipped = &report["results"][0];
    assert_eq!(skipped["status"], "skipped");
    assert_eq!(skipped["skipped"], true);
    assert_eq!(skipped["error"], Value::Null);
    assert_eq!(
        skipped["response"]["statusText"],
        "Prompt variables detected in request. CLI execution is not supported for requests with prompt variables. \nPrompts: Code, Who am I"
    );
}

#[tokio::test]
async fn ef_run_01_websocket_and_grpc_requests_are_reported_as_errors_not_left_out() {
    let (base, seen) = serve();
    let dir = new_collection(&base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[PASS]);
    write(
        dir.path(),
        "w.yml",
        "info:\n  name: W\n  type: websocket\n  seq: 2\n\nwebsocket:\n  url: ws://example.test/live\n",
    );
    write(dir.path(), "g.yml", "info:\n  name: G\n  type: grpc\n  seq: 3\n\ngrpc:\n  url: example.test:50051\n");
    request(dir.path(), "b.yml", "B", 4, "{{baseUrl}}/b", &[PASS]);
    let run = go(dir.path(), &[], &[], false, Duration::ZERO).await;

    assert_eq!(names(&run), ["A", "W", "G", "B"]);
    assert_eq!(seen.lock().unwrap().len(), 2, "seules A et B partent");
    assert!(run.report.failed());
    let report: Value = serde_json::from_str(&xc_runner::json(&run.report, &xc_runner::Redact::default())).unwrap();
    for (index, kind) in [(1, "websocket"), (2, "grpc")] {
        let entry = &report["results"][index];
        assert_eq!(entry["status"], "error");
        assert_eq!(entry["error"], format!("protocole non pris en charge par le runner : {kind}"));
    }
    assert_eq!(report["results"][3]["status"], "pass");
}

#[tokio::test]
async fn ef_run_01_a_multipart_body_is_not_copied_into_the_post_response_script() {
    let (base, _) = serve();
    let dir = new_collection(&base);
    fs::write(dir.path().join("payload.bin"), vec![0xFFu8; 4096]).unwrap();
    write(
        dir.path(),
        "up.yml",
        &format!(
            "info:\n  name: Up\n  type: http\n  seq: 1\n\nhttp:\n  method: POST\n  url: \"{base}/up\"\n  body:\n    type: multipart-form\n    data:\n      - name: file\n        value:\n          - ./payload.bin\n        type: file\n\nruntime:\n  scripts:\n    - type: tests\n      code: |-\n        test('le corps reste celui de la requête', () => expect(typeof req.getBody()).to.not.equal('string'));\n"
        ),
    );
    let run = go(dir.path(), &[], &[], false, Duration::ZERO).await;

    assert!(!run.report.failed(), "{:?}", run.report.results[0].outcome.error);
    assert_eq!(run.report.summary().passed_tests, 1);
}

#[tokio::test]
async fn ef_run_01_prompt_variables_in_a_variable_value_skip_the_requests_that_see_it() {
    let (base, seen) = serve();
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "opencollection.yml",
        &format!("opencollection: 1.0.0\n\ninfo:\n  name: Shop\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n    - name: token\n      value: \"{{{{?Token}}}}\"\n"),
    );
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[PASS]);
    let run = go(dir.path(), &[], &[], false, Duration::ZERO).await;

    assert_eq!(run.report.results[0].skip, Some(Skip::Prompts(vec!["Token".into()])));
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ef_run_01_an_unreadable_file_is_reported_as_skipped_without_stopping_the_run() {
    let (base, _) = serve();
    let dir = three(&base);
    write(dir.path(), "broken.yml", "info:\n  name: Broken\n  type: http\n  seq: 9\n\nhttp: [oops\n");
    let run = go(dir.path(), &[], &[], false, Duration::ZERO).await;

    let broken = run.report.results.iter().find(|r| r.path == "broken.yml");
    assert!(matches!(broken.map(|r| &r.skip), Some(Some(Skip::Unreadable(_)))), "{:?}", names(&run));
    assert_eq!(run.report.summary().passed_requests, 3);
}

#[tokio::test]
async fn ef_run_01_a_cancelled_run_stops_before_the_next_request() {
    let (base, _) = serve();
    let dir = three(&base);
    let collection = open_collection(dir.path()).unwrap();
    let items = select(&collection.items, &[]).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let job = Job {
        root: dir.path(),
        collection_name: "Shop",
        env: None,
        items: &items,
        rows: &[],
        bail: false,
        delay: Duration::ZERO,
        max_jumps: MAX_JUMPS,
        execution_mode: "runner",
        cancel: Arc::clone(&cancel),
    };
    let mut session = Session::default();
    let report = run_collection(job, &mut session, &mut |event| {
        if matches!(event, Event::Finished(r) if r.name == "A") {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .await;

    assert_eq!(report.results.len(), 1);
    assert_eq!(report.halt, Some(Halt::Cancelled));
}

#[tokio::test]
async fn ef_cli_02_the_three_reports_describe_the_same_run() {
    let (base, _) = serve();
    let dir = new_collection(&base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[PASS]);
    request(
        dir.path(),
        "b.yml",
        "B",
        2,
        "{{baseUrl}}/fail",
        &[("tests", "test('is ok', () => expect(res.status).to.equal(200));")],
    );
    let run = go(dir.path(), &[], &[], false, Duration::ZERO).await;
    let redact = Redact::default();

    let doc: Value = serde_json::from_str(&json_report(&run.report, &redact)).unwrap();
    assert_eq!(doc["summary"]["passedRequests"], 1);
    assert_eq!(doc["summary"]["failedRequests"], 1);
    assert_eq!(doc["results"][1]["response"]["status"], 500);
    assert_eq!(doc["results"][1]["testResults"][0]["status"], "fail");

    let xml = junit(&run.report, &redact);
    assert!(xml.contains("<testsuite name=\"A\" file=\"a.yml\" errors=\"0\" failures=\"0\""), "{xml}");
    assert!(xml.contains("<testsuite name=\"B\" file=\"b.yml\" errors=\"0\" failures=\"1\""), "{xml}");

    let page = html_page(
        &run.report,
        &redact,
        &Meta { collection: "Shop".into(), completed_at: "2026-10-06T00:00:00.000Z".into() },
    );
    assert!(page.contains("✗ Échec") && page.contains("is ok") && page.contains("500 Internal Server Error"), "{page}");
}

#[tokio::test]
async fn ef_run_01_cancelling_abandons_a_request_that_never_answers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        let held: Vec<_> = listener.incoming().take(1).flatten().collect();
        thread::sleep(Duration::from_secs(5));
        drop(held);
    });
    let dir = new_collection(&base);
    request(dir.path(), "a.yml", "A", 1, "{{baseUrl}}/a", &[PASS]);
    request(dir.path(), "b.yml", "B", 2, "{{baseUrl}}/b", &[PASS]);
    let collection = open_collection(dir.path()).unwrap();
    let items = select(&collection.items, &[]).unwrap();
    let cancel = Arc::new(AtomicBool::new(false));
    let job = Job {
        root: dir.path(),
        collection_name: "Shop",
        env: None,
        items: &items,
        rows: &[],
        bail: false,
        delay: Duration::ZERO,
        max_jumps: MAX_JUMPS,
        execution_mode: "runner",
        cancel: Arc::clone(&cancel),
    };
    let flag = Arc::clone(&cancel);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        flag.store(true, Ordering::Relaxed);
    });
    let started = Instant::now();
    let report = run_collection(job, &mut Session::default(), &mut |_| {}).await;

    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    assert!(report.results.is_empty(), "la requête abandonnée n'est pas un résultat");
    assert_eq!(report.halt, Some(Halt::Cancelled));
}
