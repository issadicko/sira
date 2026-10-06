use std::collections::HashMap;
use std::fs;
use std::path::Path;

use xc_core::vars::{Context, Scope, ScopeOverrides, SECRET_MASK};
use xc_core::{read_request, RequestDoc};

fn collection() -> (tempfile::TempDir, RequestDoc) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: S\n").unwrap();
    fs::create_dir(root.join("environments")).unwrap();
    fs::write(
        root.join("environments/dev.yml"),
        "name: dev\nvariables:\n  - name: base\n    value: http://x\n  - secret: true\n    name: token\n  - secret: true\n    name: other\n",
    )
    .unwrap();
    fs::write(
        root.join("api.yml"),
        "info:\n  name: Api\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/a?t={{token}}\"\n",
    )
    .unwrap();
    let doc = read_request(root, "api.yml").unwrap();
    (dir, doc)
}

fn scope(root: &Path, doc: &RequestDoc, overrides: ScopeOverrides) -> Scope {
    let ctx = Context::load(root, "api.yml").unwrap();
    Scope::build(root, &ctx, "api.yml", doc, Some("dev"), &HashMap::new()).unwrap().with_overrides(overrides)
}

fn secrets(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

#[test]
fn enf_sec_01_a_secret_value_given_by_the_host_is_interpolated() {
    let (dir, doc) = collection();
    let scope = scope(
        dir.path(),
        &doc,
        ScopeOverrides { secrets: secrets(&[("token", "s3cr3t")]), ..ScopeOverrides::default() },
    );
    let mut unresolved = Vec::new();
    assert_eq!(scope.interpolate("{{base}}/a?t={{token}}", &mut unresolved), "http://x/a?t=s3cr3t");
    assert!(unresolved.is_empty());
}

#[test]
fn enf_sec_01_a_secret_without_a_value_is_unresolved() {
    let (dir, doc) = collection();
    let scope = scope(dir.path(), &doc, ScopeOverrides::default());
    let mut unresolved = Vec::new();
    scope.interpolate("{{token}}", &mut unresolved);
    assert_eq!(unresolved, ["token"]);
}

#[test]
fn enf_sec_01_only_names_the_environment_declares_secret_are_accepted() {
    let (dir, doc) = collection();
    let overrides =
        ScopeOverrides { secrets: secrets(&[("base", "http://evil"), ("stray", "x")]), ..ScopeOverrides::default() };
    let scope = scope(dir.path(), &doc, overrides);
    assert_eq!(scope.lookup("base"), Some("http://x"), "une variable ordinaire n'est pas réécrite par le trousseau");
    assert_eq!(scope.lookup("stray"), None);
}

#[test]
fn enf_sec_01_the_interface_sees_that_a_secret_is_set_never_its_value() {
    let (dir, doc) = collection();
    let overrides = ScopeOverrides { secrets: secrets(&[("token", "s3cr3t")]), ..ScopeOverrides::default() };
    let infos = scope(dir.path(), &doc, overrides).infos();
    let token = infos.iter().find(|v| v.name == "token").unwrap();
    assert!(token.secret);
    assert_eq!(token.value.as_deref(), Some(SECRET_MASK));
    assert!(token.rungs.iter().all(|r| r.value.as_deref().is_none_or(|v| v == SECRET_MASK)), "{:?}", token.rungs);
    assert!(!serde_json::to_string(&infos).unwrap().contains("s3cr3t"));
    let other = infos.iter().find(|v| v.name == "other").unwrap();
    assert!(other.secret && other.value.is_none(), "un secret sans valeur reste vide");
}

#[test]
fn enf_sec_01_secrets_survive_the_environment_a_script_rewrote() {
    let (dir, doc) = collection();
    let overrides = ScopeOverrides {
        env: Some(secrets(&[("base", "http://script"), ("token", "stale")])),
        secrets: secrets(&[("token", "fresh")]),
        ..ScopeOverrides::default()
    };
    let scope = scope(dir.path(), &doc, overrides);
    assert_eq!((scope.lookup("base"), scope.lookup("token")), (Some("http://script"), Some("fresh")));
}
