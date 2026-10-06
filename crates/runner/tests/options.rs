use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use xc_core::open_collection;
use xc_runner::{
    filter_items, has_executable_test, load_env_file, persist_variables, run_collection, select, EnvWrites, Filter,
    Job, Session, MAX_JUMPS,
};

type Seen = Arc<Mutex<Vec<String>>>;

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
            log.lock().unwrap().push(head.lines().next().unwrap_or_default().to_owned());
            let body = r#"{"ok":true}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
    write(
        dir.path(),
        "opencollection.yml",
        &format!("opencollection: 1.0.0\n\ninfo:\n  name: Shop\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n"),
    );
    dir
}

/// `extra` : les lignes ajoutées sous `info:`, `runtime` : le bloc `runtime:` (sans son titre).
fn request(root: &Path, file: &str, seq: u32, extra: &str, runtime: &str) {
    let mut text = format!(
        "info:\n  name: {file}\n  type: http\n  seq: {seq}\n{extra}\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/{file}\"\n"
    );
    if !runtime.is_empty() {
        text.push_str("\nruntime:\n");
        text.push_str(runtime);
    }
    write(root, file, &text);
}

fn script(kind: &str, code: &str) -> String {
    let mut out = format!("  scripts:\n    - type: {kind}\n      code: |-\n");
    for line in code.lines() {
        out.push_str(&format!("        {line}\n"));
    }
    out
}

fn kept(root: &Path, filter: &Filter) -> Vec<String> {
    let info = open_collection(root).unwrap();
    let items = select(&info.items, &[]).unwrap();
    filter_items(root, items, filter).into_iter().map(|i| i.path).collect()
}

#[test]
fn ef_run_03_a_test_call_counts_like_bru_run_tests_only() {
    assert!(has_executable_test("test('ok', () => {});"));
    assert!(has_executable_test("  test ('espace avant la parenthèse', fn)"));
    assert!(has_executable_test("const x = 1;\ntest(\"après une ligne\", fn)"));
    assert!(!has_executable_test("// test('commenté', fn)"));
    assert!(!has_executable_test("/* test('commenté', fn) */"));
    assert!(!has_executable_test("const s = \"test(\";"));
    assert!(!has_executable_test("const s = 'test(' + `test(`;"));
    assert!(!has_executable_test("regex.test(value)"));
    assert!(!has_executable_test("testing(1)"));
    assert!(!has_executable_test(""));
    assert!(has_executable_test("obj.test(1); test('vrai', fn)"));
    assert!(
        !has_executable_test("const url = \"http://x\"; test('après', fn)"),
        "comme Bruno, les commentaires de ligne partent avant les textes : le « // » d'une adresse coupe la ligne"
    );
    assert!(has_executable_test("const url = \"http://x\";\ntest('ligne suivante', fn)"));
}

#[test]
fn ef_run_03_tests_only_keeps_requests_with_an_active_assertion_or_a_test() {
    let dir = collection("http://127.0.0.1:1");
    let root = dir.path();
    request(root, "tested.yml", 1, "", &script("tests", "test('ok', () => {});"));
    request(root, "pre.yml", 2, "", &script("before-request", "test('avant', () => {});"));
    request(root, "post.yml", 3, "", &script("after-response", "test('après', () => {});"));
    request(root, "commented.yml", 4, "", &script("tests", "// test('non', () => {});"));
    request(
        root,
        "asserted.yml",
        5,
        "",
        "  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n",
    );
    request(
        root,
        "disabled.yml",
        6,
        "",
        "  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n      disabled: true\n",
    );
    request(root, "bare.yml", 7, "", "");

    let only = Filter { tests_only: true, ..Filter::default() };
    assert_eq!(kept(root, &only), ["tested.yml", "pre.yml", "post.yml", "asserted.yml"]);
    assert_eq!(kept(root, &Filter::default()).len(), 7);
}

#[test]
fn ef_run_03_tags_include_a_request_with_one_of_them_and_exclude_wins() {
    let dir = collection("http://127.0.0.1:1");
    let root = dir.path();
    request(root, "a.yml", 1, "  tags:\n    - smoke\n    - users\n", "");
    request(root, "b.yml", 2, "  tags:\n    - slow\n", "");
    request(root, "c.yml", 3, "  tags:\n    - smoke\n    - slow\n", "");
    request(root, "d.yml", 4, "", "");

    let tags = |include: &str, exclude: &str| Filter {
        tags: Filter::split(Some(include)),
        exclude_tags: Filter::split(Some(exclude)),
        ..Filter::default()
    };
    assert_eq!(kept(root, &tags("smoke", "")), ["a.yml", "c.yml"]);
    assert_eq!(kept(root, &tags("smoke,slow", "")), ["a.yml", "b.yml", "c.yml"]);
    assert_eq!(kept(root, &tags("", "slow")), ["a.yml", "d.yml"]);
    assert_eq!(kept(root, &tags("smoke", "slow")), ["a.yml"]);
    assert_eq!(Filter::split(Some("")), Vec::<String>::new());
    assert_eq!(Filter::split(None), Vec::<String>::new());
}

#[test]
fn ef_run_03_a_file_that_cannot_be_read_stays_in_the_run_so_its_error_shows() {
    let dir = collection("http://127.0.0.1:1");
    write(dir.path(), "broken.yml", "info:\n  name: Broken\n  type: http\n  seq: 1\nhttp: [\n");
    request(dir.path(), "ok.yml", 2, "  tags:\n    - a\n", "");
    let only_a = Filter { tags: vec!["a".into()], ..Filter::default() };
    let paths = kept(dir.path(), &only_a);
    assert!(paths.contains(&"ok.yml".to_owned()), "{paths:?}");
}

fn envfile(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, text).unwrap();
    path
}

#[test]
fn ef_run_03_a_json_environment_file_gives_its_enabled_variables_and_its_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = envfile(
        dir.path(),
        "ci.json",
        r#"{"name":" Recette ","variables":[{"name":"host","value":"h1","enabled":true},{"name":"off","value":"x","enabled":false},{"name":"n","value":3},{"name":"s","value":"v","secret":true}]}"#,
    );
    let EnvWrites { name, vars } = load_env_file(&path).unwrap();
    assert_eq!(name, None, "pas de fichier dans environments/");
    assert_eq!(vars.get(xc_script::ENV_NAME).unwrap(), "Recette");
    assert_eq!(vars.get("host").unwrap(), "h1");
    assert_eq!(vars.get("n").unwrap(), 3);
    assert!(vars.get("off").is_none());
    assert_eq!(vars.get("s").unwrap(), "v");

    let unnamed = envfile(dir.path(), "staging.json", r#"{"variables":[]}"#);
    assert_eq!(load_env_file(&unnamed).unwrap().vars.get(xc_script::ENV_NAME).unwrap(), "staging");
}

#[test]
fn ef_run_03_an_environment_file_that_is_not_an_environment_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let wrong = envfile(dir.path(), "list.json", r#"[1,2]"#);
    assert!(load_env_file(&wrong).unwrap_err().contains("variables"));
    let invalid = envfile(dir.path(), "bad.json", "{");
    assert!(load_env_file(&invalid).unwrap_err().contains("JSON"));
    let bru = envfile(dir.path(), "local.bru", "vars { a: 1 }");
    assert!(load_env_file(&bru).unwrap_err().contains(".bru"));
    assert!(load_env_file(&dir.path().join("absent.json")).is_err());
}

#[test]
fn ef_run_03_a_yaml_environment_file_gives_its_enabled_values_without_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let path = envfile(
        dir.path(),
        "ci.yml",
        "name: ci\nvariables:\n  - name: host\n    value: h2\n  - name: off\n    value: x\n    disabled: true\n  - secret: true\n    name: token\n",
    );
    let vars = load_env_file(&path).unwrap().vars;
    assert_eq!(vars.get(xc_script::ENV_NAME).unwrap(), "ci");
    assert_eq!(vars.get("host").unwrap(), "h2");
    assert!(vars.get("off").is_none() && vars.get("token").is_none());
}

#[tokio::test]
async fn ef_run_03_a_run_reads_its_variables_from_an_environment_file_outside_the_collection() {
    let (base, seen) = serve();
    let dir = collection(&base);
    write(
        dir.path(),
        "ping.yml",
        "info:\n  name: Ping\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{origin}}/ping\"\n",
    );
    let outside = tempfile::tempdir().unwrap();
    let file =
        envfile(outside.path(), "ci.json", &format!(r#"{{"variables":[{{"name":"origin","value":"{base}"}}]}}"#));
    let writes = load_env_file(&file).unwrap();

    let info = open_collection(dir.path()).unwrap();
    let items = select(&info.items, &[]).unwrap();
    let job = Job {
        root: dir.path(),
        collection_name: &info.name,
        env: None,
        items: &items,
        rows: &[],
        bail: false,
        delay: Duration::ZERO,
        max_jumps: MAX_JUMPS,
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let mut session = Session { env: Some(writes), ..Session::default() };
    let report = run_collection(job, &mut session, &mut |_| {}).await;

    assert!(!report.failed(), "{:?}", report.results[0].outcome.error);
    assert_eq!(seen.lock().unwrap().as_slice(), ["GET /ping HTTP/1.1"]);
}

#[tokio::test]
async fn ef_run_03_a_run_with_no_request_left_gives_an_empty_report() {
    let dir = collection("http://127.0.0.1:1");
    request(dir.path(), "a.yml", 1, "  tags:\n    - a\n", "");
    let info = open_collection(dir.path()).unwrap();
    let items = filter_items(
        dir.path(),
        select(&info.items, &[]).unwrap(),
        &Filter { tags: vec!["absent".into()], ..Filter::default() },
    );
    assert!(items.is_empty());
    let job = Job {
        root: dir.path(),
        collection_name: &info.name,
        env: None,
        items: &items,
        rows: &[],
        bail: false,
        delay: Duration::ZERO,
        max_jumps: MAX_JUMPS,
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let report = run_collection(job, &mut Session::default(), &mut |_| {}).await;
    assert!(report.results.is_empty() && !report.failed() && report.halt.is_none());
}

/// `/slow` répond après deux secondes, `/redirect` renvoie vers `/target`, le reste répond 200 tout de suite.
fn serve_slow_and_redirecting() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            thread::spawn(move || {
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).into_owned();
                let line = head.lines().next().unwrap_or_default().to_owned();
                if line.contains("/slow") {
                    thread::sleep(Duration::from_secs(2));
                }
                let reply = if line.contains("/redirect") {
                    "HTTP/1.1 302 Found\r\nLocation: /target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        .to_owned()
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok".to_owned()
                };
                let _ = stream.write_all(reply.as_bytes());
            });
        }
    });
    base
}

async fn run_one(root: &Path) -> xc_runner::RunReport {
    let info = open_collection(root).unwrap();
    let items = select(&info.items, &[]).unwrap();
    let job = Job {
        root,
        collection_name: &info.name,
        env: None,
        items: &items,
        rows: &[],
        bail: false,
        delay: Duration::ZERO,
        max_jumps: MAX_JUMPS,
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    run_collection(job, &mut Session::default(), &mut |_| {}).await
}

#[tokio::test]
async fn ef_run_03_req_set_timeout_in_a_pre_request_script_limits_the_wait() {
    let base = serve_slow_and_redirecting();
    let dir = collection(&base);
    request(dir.path(), "slow.yml", 1, "", &script("before-request", "req.setTimeout(300);"));
    request(dir.path(), "patient.yml", 2, "", &script("before-request", "req.setTimeout(10000);"));
    let report = run_one(dir.path()).await;

    let started = std::time::Instant::now();
    assert!(report.results[0].outcome.error.is_some(), "300 ms ne suffisent pas pour /slow");
    assert!(report.results[1].outcome.error.is_none(), "{:?}", report.results[1].outcome.error);
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn ef_run_03_req_set_max_redirects_in_a_pre_request_script_limits_the_hops() {
    let base = serve_slow_and_redirecting();
    let dir = collection(&base);
    request(dir.path(), "redirect.yml", 1, "", &script("before-request", "req.setMaxRedirects(0);"));
    request(dir.path(), "follow.yml", 2, "", "");
    write(
        dir.path(),
        "followed.yml",
        &format!(
            "info:\n  name: Followed\n  type: http\n  seq: 3\n\nhttp:\n  method: GET\n  url: \"{base}/redirect\"\n"
        ),
    );
    let report = run_one(dir.path()).await;

    let status = |i: usize| report.results[i].outcome.response.as_ref().map(|r| r.status);
    assert_eq!(status(0), Some(302), "aucun saut suivi");
    assert_eq!(status(2), Some(200), "sans script, la redirection est suivie");
}

async fn run_in_env(root: &Path, env: Option<&str>) -> Session {
    let info = open_collection(root).unwrap();
    let items = select(&info.items, &[]).unwrap();
    let job = Job {
        root,
        collection_name: &info.name,
        env,
        items: &items,
        rows: &[],
        bail: false,
        delay: Duration::ZERO,
        max_jumps: MAX_JUMPS,
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let mut session = Session::default();
    run_collection(job, &mut session, &mut |_| {}).await;
    session
}

fn persisting_collection(base: &str, code: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "opencollection.yml",
        "opencollection: 1.0.0\n\ninfo:\n  name: PV\n\nrequest:\n  variables:\n    - name: region\n      value: us\n    - name: kept\n      value: keep\n      disabled: true\n",
    );
    write(
        dir.path(),
        "environments/dev.yml",
        "name: dev\nvariables:\n  - name: host\n    value: old\n  - name: stale\n    value: gone\n  - name: off\n    value: kept\n    disabled: true\n  - secret: true\n    name: token\n",
    );
    write(
        dir.path(),
        "a.yml",
        &format!(
            "info:\n  name: A\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{base}/a\"\n\nruntime:\n{}",
            script("after-response", code)
        ),
    );
    dir
}

#[tokio::test]
async fn ef_run_03_persisted_variables_follow_the_merge_rules_of_bru_run() {
    let (base, _) = serve();
    let dir = persisting_collection(
        &base,
        "bru.setEnvVar('host', 'new');\nbru.setEnvVar('port', 3000);\nbru.setEnvVar('flag', true);\nbru.setEnvVar('cfg', {a: 1});\nbru.deleteEnvVar('stale');\nbru.setCollectionVar('region', 'eu');\nbru.setCollectionVar('retries', 3);",
    );
    let root = dir.path();
    let session = run_in_env(root, Some("dev")).await;
    let done = persist_variables(root, &session, Some("dev")).unwrap();

    assert_eq!(done.environment.as_deref(), Some("environments/dev.yml"));
    assert!(done.collection);
    assert_eq!(
        fs::read_to_string(root.join("environments/dev.yml")).unwrap(),
        "name: dev\nvariables:\n  - name: host\n    value: new\n  - name: off\n    value: kept\n    disabled: true\n  - secret: true\n    name: token\n  - name: port\n    value:\n      type: number\n      data: \"3000\"\n  - name: flag\n    value:\n      type: boolean\n      data: \"true\"\n  - name: cfg\n    value:\n      type: object\n      data: |-\n        {\n          \"a\": 1\n        }\n",
        "valeur mise à jour, retirée, désactivée gardée, nouvelles en fin dans l'ordre du script, typées"
    );
    let collection = fs::read_to_string(root.join("opencollection.yml")).unwrap();
    assert!(collection.contains("name: region\n      value: eu"), "{collection}");
    assert!(collection.contains("name: kept\n      value: keep\n      disabled: true"), "{collection}");
    assert!(
        collection.contains("name: retries\n      value:\n        type: number\n        data: \"3\""),
        "{collection}"
    );
}

#[tokio::test]
async fn ef_run_03_nothing_is_written_when_no_script_touched_a_variable() {
    let (base, _) = serve();
    let dir = persisting_collection(&base, "console.log('rien');");
    let root = dir.path();
    let (env_before, collection_before) = (
        fs::read_to_string(root.join("environments/dev.yml")).unwrap(),
        fs::read_to_string(root.join("opencollection.yml")).unwrap(),
    );
    let session = run_in_env(root, Some("dev")).await;
    let done = persist_variables(root, &session, Some("dev")).unwrap();

    assert_eq!(done, xc_runner::Persisted::default());
    assert_eq!(fs::read_to_string(root.join("environments/dev.yml")).unwrap(), env_before);
    assert_eq!(fs::read_to_string(root.join("opencollection.yml")).unwrap(), collection_before);
}

#[tokio::test]
async fn ef_run_03_a_secret_row_is_never_dropped_nor_given_a_value() {
    let (base, _) = serve();
    let dir = persisting_collection(&base, "bru.setEnvVar('token', 'rotated');\nbru.setEnvVar('other', 'x');");
    let root = dir.path();
    let session = run_in_env(root, Some("dev")).await;
    persist_variables(root, &session, Some("dev")).unwrap();

    let env = fs::read_to_string(root.join("environments/dev.yml")).unwrap();
    assert!(env.contains("  - secret: true\n    name: token\n"), "{env}");
    assert!(!env.contains("rotated"), "{env}");
    assert!(env.contains("name: other"), "{env}");
}

#[tokio::test]
async fn ef_run_03_a_secret_missing_from_the_script_variables_stays_in_the_file() {
    let (base, _) = serve();
    let dir = persisting_collection(&base, "bru.setEnvVar('other', 'x');");
    let root = dir.path();
    let session = run_in_env(root, Some("dev")).await;
    assert!(
        !session.env.as_ref().unwrap().vars.contains_key("token"),
        "pas de trousseau : le script ne voit pas le secret"
    );
    persist_variables(root, &session, Some("dev")).unwrap();
    let env = fs::read_to_string(root.join("environments/dev.yml")).unwrap();
    assert!(env.contains("  - secret: true\n    name: token\n"), "{env}");
}

#[tokio::test]
async fn ef_run_03_without_an_environment_only_collection_variables_are_written() {
    let (base, _) = serve();
    let dir = persisting_collection(&base, "bru.setEnvVar('x', 1);\nbru.setCollectionVar('region', 'eu');");
    let root = dir.path();
    let session = run_in_env(root, None).await;
    let done = persist_variables(root, &session, None).unwrap();
    assert_eq!(done.environment, None);
    assert!(done.collection);
    assert!(!fs::read_to_string(root.join("environments/dev.yml")).unwrap().contains("name: x"));
}

#[tokio::test]
async fn ef_scr_03_deleting_an_environment_variable_keeps_the_order_of_the_others() {
    let (base, _) = serve();
    let dir = collection(&base);
    write(
        dir.path(),
        "environments/dev.yml",
        "name: dev\nvariables:\n  - name: a\n    value: 1\n  - name: b\n    value: 2\n  - name: c\n    value: 3\n",
    );
    request(dir.path(), "a.yml", 1, "", &script("after-response", "bru.setEnvVar('d', 4);\nbru.deleteEnvVar('a');"));
    let session = run_in_env(dir.path(), Some("dev")).await;
    let keys: Vec<&str> =
        session.env.as_ref().unwrap().vars.keys().map(String::as_str).filter(|k| *k != "__name__").collect();
    assert_eq!(keys, ["b", "c", "d"]);
}
