use std::collections::HashMap;
use std::fs;
use std::path::Path;

use xc_core::assert::{evaluate, ResponseView};
use xc_core::collection::{resolve_path, write_atomic};
use xc_core::vars::{Context, Scope};
use xc_core::{open_collection, prepare, read_request, save_request, Assertion, TreeItem};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn sample() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "opencollection.yml", "opencollection: 1.0.0\n\ninfo:\n  name: API Paiements\n\nrequest:\n  headers:\n    - name: X-Client\n      value: xclient\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n  variables:\n    - name: baseUrl\n      value: https://api.paiements.test/v1\n    - name: timeoutMs\n      value: \"10000\"\nbundled: false\nextensions:\n  bruno:\n    presets:\n      defaultEnvironment: dev\n");
    write(root, "environments/dev.yml", "name: dev\nvariables:\n  - name: baseUrl\n    value: http://127.0.0.1:9\n  - name: txId\n    value: TX-2026-0042\n  - name: canal\n    value: MOBILE\n  - secret: true\n    name: token\n");
    write(
        root,
        "environments/prod.yml",
        "name: prod\nvariables:\n  - name: baseUrl\n    value: https://api.paiements.example/v1\n",
    );
    write(root, "transactions/folder.yml", "info:\n  name: Transactions\n  type: folder\n  seq: 2\n\nrequest:\n  auth: inherit\n  variables:\n    - name: canal\n      value: USSD\n");
    write(root, "transactions/detail.yml", "info:\n  name: Détail d'une transaction\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/transactions/:id?expand=client\"\n  headers:\n    - name: X-Canal\n      value: \"{{canal}}\"\n  params:\n    - name: expand\n      value: client\n      type: query\n    - name: id\n      value: \"{{txId}}\"\n      type: path\n  auth: inherit\n\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n    - expression: res.body.devise\n      operator: eq\n      value: XOF\n\nsettings:\n  encodeUrl: true\n  timeout: 0\n  followRedirects: true\n  maxRedirects: 5\n  forwardAuthorizationHeader: true\n");
    write(root, "transactions/liste.yml", "info:\n  name: Liste des transactions\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/transactions\"\n  auth: inherit\n");
    write(root, "auth/connexion.yml", "info:\n  name: Connexion\n  type: http\n  seq: 1\n\nhttp:\n  method: POST\n  url: \"{{baseUrl}}/auth/connexion\"\n  body:\n    type: json\n    data: |-\n      {\n        \"motDePasse\": \"{{process.env.PSP_PASSWORD}}\"\n      }\n");
    write(root, ".env", "PSP_PASSWORD=s3cret\n");
    write(root, "node_modules/x.yml", "info:\n  name: ignoré\n");
    dir
}

#[test]
fn ef_col_01_tree_follows_bruno_order() {
    let dir = sample();
    let c = open_collection(dir.path()).unwrap();
    assert_eq!(c.name, "API Paiements");
    assert_eq!(c.request_count, 3);
    assert_eq!(c.environments, ["dev", "prod"]);
    assert_eq!(c.default_environment.as_deref(), Some("dev"));
    let names: Vec<_> = c
        .items
        .iter()
        .map(|i| match i {
            TreeItem::Folder { name, .. } | TreeItem::Request { name, .. } => name.as_str(),
        })
        .collect();
    assert_eq!(names, ["auth", "Transactions"]);
    let TreeItem::Folder { children, .. } = &c.items[1] else { panic!() };
    let TreeItem::Request { name, path, method, .. } = &children[0] else { panic!() };
    assert_eq!(
        (name.as_str(), path.as_str(), method.as_str()),
        ("Liste des transactions", "transactions/liste.yml", "GET")
    );
}

#[test]
fn ef_var_01_precedence_matches_bruno() {
    let dir = sample();
    let root = dir.path();
    let doc = read_request(root, "transactions/detail.yml").unwrap();
    let ctx = Context::load(root, "transactions/detail.yml").unwrap();
    let mut runtime = HashMap::new();
    runtime.insert("txId".to_owned(), "TX-RUNTIME".to_owned());
    let scope = Scope::build(root, &ctx, "transactions/detail.yml", &doc, Some("dev"), &runtime).unwrap();

    assert_eq!(scope.lookup("txId"), Some("TX-RUNTIME"), "runtime > environnement");
    assert_eq!(scope.lookup("canal"), Some("USSD"), "dossier > environnement");
    assert_eq!(scope.lookup("baseUrl"), Some("http://127.0.0.1:9"), "environnement > collection");
    assert_eq!(scope.lookup("timeoutMs"), Some("10000"));

    let info = scope.info("baseUrl");
    assert_eq!(info.level.as_deref(), Some("Environnement dev"));
    let shadowed: Vec<_> = info.rungs.iter().filter(|r| r.value.is_some()).map(|r| r.level.as_str()).collect();
    assert_eq!(shadowed, ["Environnement dev", "Collection"]);
    assert!(scope.info("token").secret);
}

#[test]
fn ef_var_02_dynamic_and_process_env_variables() {
    let dir = sample();
    let root = dir.path();
    let doc = read_request(root, "auth/connexion.yml").unwrap();
    let ctx = Context::load(root, "auth/connexion.yml").unwrap();
    let scope = Scope::build(root, &ctx, "auth/connexion.yml", &doc, None, &HashMap::new()).unwrap();
    let mut unresolved = Vec::new();
    let out = scope.interpolate("{{$randomUUID}}|{{process.env.PSP_PASSWORD}}|{{nope}}", &mut unresolved);
    let parts: Vec<_> = out.split('|').collect();
    assert_eq!(parts[0].len(), 36);
    assert_eq!(parts[1], "s3cret");
    assert_eq!(parts[2], "{{nope}}");
    assert_eq!(unresolved, ["nope"]);
}

#[test]
fn ef_aut_04_auth_and_headers_are_inherited() {
    let dir = sample();
    let root = dir.path();
    let doc = read_request(root, "transactions/detail.yml").unwrap();
    let mut runtime = HashMap::new();
    runtime.insert("token".to_owned(), "abc".to_owned());
    let p = prepare(root, "transactions/detail.yml", &doc, Some("dev"), &runtime).unwrap();
    assert_eq!(p.request.url, "http://127.0.0.1:9/transactions/TX-2026-0042?expand=client");
    let header = |n: &str| p.request.headers.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str());
    assert_eq!(header("Authorization"), Some("Bearer abc"));
    assert_eq!(header("X-Client"), Some("xclient"));
    assert_eq!(header("X-Canal"), Some("USSD"));
    assert!(p.unresolved.is_empty());
}

#[test]
fn unresolved_variables_are_reported() {
    let dir = sample();
    let root = dir.path();
    let doc = read_request(root, "transactions/detail.yml").unwrap();
    let p = prepare(root, "transactions/detail.yml", &doc, Some("prod"), &HashMap::new()).unwrap();
    assert!(p.unresolved.contains(&"txId".to_owned()));
    assert!(p.unresolved.contains(&"token".to_owned()));
}

#[test]
fn ef_req_03_saving_without_change_writes_nothing_and_edits_are_atomic() {
    let dir = sample();
    let root = dir.path();
    let path = root.join("transactions/detail.yml");
    let before = fs::read_to_string(&path).unwrap();
    let mut doc = read_request(root, "transactions/detail.yml").unwrap();
    assert!(!save_request(root, "transactions/detail.yml", &doc).unwrap());
    assert_eq!(fs::read_to_string(&path).unwrap(), before);

    doc.method = "HEAD".into();
    assert!(save_request(root, "transactions/detail.yml", &doc).unwrap());
    let after = fs::read_to_string(&path).unwrap();
    assert_eq!(after, before.replace("method: GET", "method: HEAD"));
    let leftovers: Vec<_> = fs::read_dir(root.join("transactions"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty());
}

#[test]
fn paths_outside_the_collection_are_refused() {
    let dir = sample();
    assert!(read_request(dir.path(), "../secret.yml").is_err());
}

#[test]
fn ef_col_01_resolve_path_refuses_every_way_out_of_the_collection() {
    let root = Path::new("/collection");
    for escape in ["/etc/hosts", "../secret.yml", "a/../../b.yml", "a/../b.yml"] {
        assert!(resolve_path(root, escape).is_err(), "{escape} aurait dû être refusé");
    }
    assert_eq!(resolve_path(root, "a/b.yml").unwrap(), root.join("a/b.yml"));
}

#[cfg(windows)]
#[test]
fn ef_col_01_resolve_path_refuses_windows_roots_and_drives() {
    let root = Path::new("C:\\collection");
    for escape in ["/etc/hosts", "\\etc\\hosts", "C:\\Windows\\win.ini", "C:win.ini", "\\\\server\\share\\x.yml"] {
        assert!(resolve_path(root, escape).is_err(), "{escape}");
    }
}

#[test]
fn ef_col_01_requests_expose_their_raw_url() {
    let dir = sample();
    let c = open_collection(dir.path()).unwrap();
    let TreeItem::Folder { children, .. } = &c.items[1] else { panic!() };
    let urls: Vec<_> = children
        .iter()
        .map(|i| match i {
            TreeItem::Request { url, .. } => url.as_str(),
            TreeItem::Folder { .. } => panic!("une requête était attendue"),
        })
        .collect();
    assert_eq!(urls, ["{{baseUrl}}/transactions", "{{baseUrl}}/transactions/:id?expand=client"]);
    assert_eq!(serde_json::to_value(&children[0]).unwrap()["url"], "{{baseUrl}}/transactions");
}

#[test]
fn enf_comp_02_atomic_write_supports_names_at_the_filesystem_limit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{}.yml", "n".repeat(251)));
    write_atomic(&path, "info:\n  name: long\n").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "info:\n  name: long\n");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn ef_tst_02_declarative_assertions() {
    let a = |expression: &str, operator: &str, value: Option<&str>| Assertion {
        expression: expression.into(),
        operator: operator.into(),
        value: value.map(Into::into),
        enabled: true,
        description: None,
    };
    let headers = vec![("content-type".to_owned(), "application/json".to_owned())];
    let body = br#"{"id":"TX-1","montant":15000,"devise":"XOF","historique":[{"statut":"INITIEE"}]}"#;
    let res = ResponseView { status: 200, headers: &headers, body };
    let results = evaluate(
        &[
            a("res.status", "eq", Some("200")),
            a("res.body.devise", "eq", Some("XOF")),
            a("res.body.montant", "gt", Some("1000")),
            a("res.body.montant", "isNumber", None),
            a("res.body.historique[0].statut", "eq", Some("\"INITIEE\"")),
            a("res.headers.content-type", "contains", Some("json")),
            a("res.body.devise", "eq", Some("EUR")),
            a("res.body.absent", "isDefined", None),
        ],
        &res,
    );
    let passed: Vec<bool> = results.iter().map(|r| r.passed).collect();
    assert_eq!(passed, [true, true, true, true, true, true, false, false]);
}
