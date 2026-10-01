use std::fs;
use std::path::Path;

use xc_core::request::BLANK_BEFORE;
use xc_core::yaml::{self, Map, Value};
use xc_core::{normalize, Auth, Body, KeyValue, RequestDoc};

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

fn tree(text: &str) -> Map {
    match yaml::parse(text).unwrap() {
        Value::Map(m) => m,
        _ => panic!("table attendue"),
    }
}

#[test]
fn enf_comp_02_bruno_files_round_trip_without_diff() {
    let blank: &[&str] = BLANK_BEFORE;
    for name in [
        "request-full.yml",
        "request-minimal.yml",
        "request-multipart.yml",
        "request-text.yml",
        "request-form.yml",
        "request-form-edited.yml",
        "request-multipart-edited.yml",
        "request-multipart-empty.yml",
        "request-quoting.yml",
        "opencollection.yml",
        "opencollection-min.yml",
        "folder.yml",
        "environment.yml",
        "scalars.yml",
    ] {
        let original = fixture(name);
        let rewritten = normalize(&original, blank).unwrap();
        assert_eq!(rewritten, original, "aller-retour de {name}");
    }
}

#[test]
fn enf_comp_02_unchanged_doc_leaves_tree_identical() {
    for name in ["request-full.yml", "request-quoting.yml", "request-multipart.yml"] {
        let original = fixture(name);
        let mut t = tree(&original);
        let doc = RequestDoc::from_tree(&t);
        doc.apply(&mut t, &doc.clone());
        assert_eq!(yaml::emit(&Value::Map(t), BLANK_BEFORE), original, "{name}");
    }
}

#[test]
fn ef_req_01_reads_method_url_params_and_headers() {
    let doc = RequestDoc::from_tree(&tree(&fixture("request-full.yml")));
    assert_eq!(doc.name, "Create User");
    assert_eq!(doc.seq, Some(3));
    assert_eq!(doc.method, "POST");
    assert_eq!(doc.url, "{{baseUrl}}/users/:id?verbose=true");
    assert_eq!(doc.params.len(), 3);
    assert!(!doc.params[1].enabled);
    assert_eq!(
        doc.headers[1],
        KeyValue {
            name: "Authorization".into(),
            value: "Bearer {{token}}".into(),
            enabled: false,
            description: Some("old token".into())
        }
    );
    assert_eq!(doc.body, Body::Json { data: "{\n  \"name\": \"John\",\n  \"age\": 30\n}".into() });
    assert_eq!(doc.auth, Auth::Bearer { token: "{{token}}".into() });
    assert_eq!(doc.variables[1].value, "42");
    assert_eq!(doc.assertions.len(), 3);
    assert_eq!(doc.assertions[1].value, None);
}

#[test]
fn ef_req_02_editing_only_touches_changed_sections() {
    let original = fixture("request-minimal.yml");
    let mut t = tree(&original);
    let before = RequestDoc::from_tree(&t);
    let mut after = before.clone();
    after.url = "https://api.example.com/x?page=2".into();
    after.headers.push(KeyValue {
        name: "X-Canal".into(),
        value: "{{canal}}".into(),
        enabled: true,
        description: None,
    });
    after.body = Body::Json { data: "{\n  \"a\": 1\n}".into() };
    after.apply(&mut t, &before);
    let written = yaml::emit(&Value::Map(t), BLANK_BEFORE);
    assert_eq!(
        written,
        "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://api.example.com/x?page=2\n  headers:\n    - name: X-Canal\n      value: \"{{canal}}\"\n  body:\n    type: json\n    data: |-\n      {\n        \"a\": 1\n      }\n  auth: inherit\n\nsettings:\n  encodeUrl: true\n  timeout: 0\n  followRedirects: true\n  maxRedirects: 5\n  forwardAuthorizationHeader: true\n"
    );
}
