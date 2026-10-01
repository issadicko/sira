use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::json;
use xc_core::request::BLANK_BEFORE;
use xc_core::yaml::{self, Map, Value};
use xc_core::{prepare, read_request, Body, CoreError, KeyValue, MultipartField, MultipartValue, Prepared, RequestDoc};

fn fixture_bytes(name: &str) -> Vec<u8> {
    fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

fn fixture(name: &str) -> String {
    String::from_utf8(fixture_bytes(name)).unwrap()
}

fn tree(text: &str) -> Map {
    match yaml::parse(text).unwrap() {
        Value::Map(m) => m,
        _ => panic!("table attendue"),
    }
}

fn edited(name: &str, edit: impl FnOnce(&mut RequestDoc)) -> String {
    let mut t = tree(&fixture(name));
    let before = RequestDoc::from_tree(&t);
    let mut after = before.clone();
    edit(&mut after);
    after.apply(&mut t, &before);
    yaml::emit(&Value::Map(t), BLANK_BEFORE)
}

fn kv(name: &str, value: &str, enabled: bool, description: Option<&str>) -> KeyValue {
    KeyValue { name: name.into(), value: value.into(), enabled, description: description.map(Into::into) }
}

fn part(name: &str, value: MultipartValue, content_type: Option<&str>) -> MultipartField {
    MultipartField {
        name: name.into(),
        value,
        enabled: true,
        content_type: content_type.map(Into::into),
        description: None,
    }
}

fn text(s: &str) -> MultipartValue {
    MultipartValue::Text(s.into())
}

fn files(paths: &[&str]) -> MultipartValue {
    MultipartValue::File(paths.iter().map(|p| (*p).to_owned()).collect())
}

#[test]
fn ef_req_02_reads_form_urlencoded_and_multipart_bodies() {
    let form = RequestDoc::from_tree(&tree(&fixture("request-form.yml")));
    assert_eq!(
        form.body,
        Body::FormUrlEncoded { fields: vec![kv("a", "1", true, None), kv("b", "x: y", false, None)] }
    );

    let multipart = RequestDoc::from_tree(&tree(&fixture("request-multipart.yml")));
    assert_eq!(
        multipart.body,
        Body::MultipartForm {
            fields: vec![part("f", files(&["/tmp/a.png"]), Some("image/png")), part("t", text("hello"), None)]
        }
    );
}

#[test]
fn ef_req_02_unchanged_form_bodies_leave_tree_identical() {
    for name in [
        "request-form.yml",
        "request-form-edited.yml",
        "request-multipart.yml",
        "request-multipart-edited.yml",
        "request-multipart-empty.yml",
    ] {
        assert_eq!(edited(name, |_| {}), fixture(name), "{name}");
    }
}

#[test]
fn ef_req_02_editing_form_fields_writes_like_bruno() {
    let written = edited("request-form.yml", |doc| {
        let Body::FormUrlEncoded { fields } = &mut doc.body else { panic!("corps form-urlencoded attendu") };
        fields[0].value = "{{x}} & y".into();
        fields[1].enabled = true;
        fields.push(kv("c d", "", false, Some("note")));
        fields.push(kv("lignes", "l1\nl2", true, Some("  ")));
    });
    assert_eq!(written, fixture("request-form-edited.yml"));
}

#[test]
fn ef_req_02_editing_multipart_fields_writes_like_bruno() {
    let written = edited("request-multipart.yml", |doc| {
        let Body::MultipartForm { fields } = &mut doc.body else { panic!("corps multipart attendu") };
        fields[0].value = files(&["/tmp/a.png", "pieces/b.pdf"]);
        fields[0].content_type = None;
        fields[1] = MultipartField {
            enabled: false,
            description: Some("méta".into()),
            ..part("t", text("{\"a\": 1}"), Some("application/json"))
        };
        fields.push(part("vide", files(&[]), None));
        fields.push(MultipartField { description: Some(" ".into()), ..part("texte", text("l1\nl2"), Some("")) });
    });
    assert_eq!(written, fixture("request-multipart-edited.yml"));
}

#[test]
fn ef_req_02_new_empty_multipart_body_writes_like_bruno() {
    let written = edited("request-minimal.yml", |doc| doc.body = Body::MultipartForm { fields: vec![] });
    assert_eq!(written, fixture("request-multipart-empty.yml"));
}

#[test]
fn ef_req_02_body_json_mirrors_the_typescript_model() {
    let body = Body::MultipartForm {
        fields: vec![
            part("f", files(&["a.png"]), Some("image/png")),
            MultipartField { enabled: false, ..part("t", text("x"), None) },
        ],
    };
    let expected = json!({
        "type": "multipart-form",
        "fields": [
            { "name": "f", "kind": "file", "value": ["a.png"], "enabled": true, "contentType": "image/png" },
            { "name": "t", "kind": "text", "value": "x", "enabled": false }
        ]
    });
    assert_eq!(serde_json::to_value(&body).unwrap(), expected);
    assert_eq!(serde_json::from_value::<Body>(expected).unwrap(), body);

    let form = Body::FormUrlEncoded { fields: vec![kv("a", "1", true, None)] };
    let expected = json!({ "type": "form-urlencoded", "fields": [{ "name": "a", "value": "1", "enabled": true }] });
    assert_eq!(serde_json::to_value(&form).unwrap(), expected);
    assert_eq!(serde_json::from_value::<Body>(expected).unwrap(), form);
}

fn collection(request: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: Formulaires\n").unwrap();
    fs::create_dir_all(root.join("files")).unwrap();
    fs::write(root.join("files/logo.png"), [0x89, b'P', b'N', b'G', 0x00, 0xff]).unwrap();
    fs::write(root.join("files/notes.txt"), "bonjour").unwrap();
    fs::write(root.join("req.yml"), request).unwrap();
    dir
}

fn send(dir: &tempfile::TempDir) -> Result<Prepared, CoreError> {
    let doc = read_request(dir.path(), "req.yml").unwrap();
    let runtime: HashMap<String, String> =
        [("x", "1+1 é"), ("champ", "nom"), ("cle", "clé")].map(|(k, v)| (k.to_owned(), v.to_owned())).into();
    prepare(dir.path(), "req.yml", &doc, None, &runtime)
}

fn content_types(p: &Prepared) -> Vec<&str> {
    p.request.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str()).collect()
}

#[test]
fn ef_req_02_form_urlencoded_is_sent_like_bruno() {
    let dir = collection("info:\n  name: F\n  type: http\n\nhttp:\n  method: POST\n  url: https://x.test\n  body:\n    type: form-urlencoded\n    data:\n      - name: a\n        value: \"{{x}} & y\"\n      - name: off\n        value: ignoré\n        disabled: true\n      - name: na me\n        value: '{\"k\": \"v\"}*-._~!''()'\n      - name: \"{{cle}}\"\n        value: ok\n      - name: \"\"\n        value: v\n      - name: l\n        value: |-\n          l1\n          l2\n");
    let p = send(&dir).unwrap();
    assert_eq!(
        String::from_utf8(p.request.body.clone().unwrap()).unwrap(),
        "a=1%2B1+%C3%A9+%26+y&na+me=%7B%22k%22%3A+%22v%22%7D*-._%7E%21%27%28%29&cl%C3%A9=ok&=v&l=l1%0Al2"
    );
    assert_eq!(content_types(&p), ["application/x-www-form-urlencoded"]);
    assert!(p.unresolved.is_empty());
}

#[test]
fn ef_req_02_user_content_type_is_kept() {
    let dir = collection("info:\n  name: F\n  type: http\n\nhttp:\n  method: POST\n  url: https://x.test\n  headers:\n    - name: content-type\n      value: application/x-www-form-urlencoded; charset=utf-8\n  body:\n    type: form-urlencoded\n    data:\n      - name: a\n        value: \"1\"\n");
    let p = send(&dir).unwrap();
    assert_eq!(p.request.body.as_deref(), Some(&b"a=1"[..]));
    assert_eq!(content_types(&p), ["application/x-www-form-urlencoded; charset=utf-8"]);
}

const MULTIPART: &str = "info:\n  name: M\n  type: http\n\nhttp:\n  method: POST\n  url: https://x.test\nHEADERS  body:\n    type: multipart-form\n    data:\n      - name: \"{{champ}}\"\n        type: text\n        value: \"{{x}}\"\n      - name: off\n        type: text\n        value: ignoré\n        disabled: true\n      - name: pieces\n        type: file\n        value:\n          - files/logo.png\n          - \" files/notes.txt \"\n      - name: typed\n        type: file\n        value:\n          - files/notes.txt\n        contentType: text/markdown\n      - name: meta\n        type: text\n        value: '{\"a\":1}'\n        contentType: application/json\n      - name: vide\n        type: file\n        value: []\n";

/// Corps produit par le paquet `form-data` de Bruno pour `MULTIPART`, avec la frontière `XYZ`.
fn bruno_multipart_body(boundary: &str) -> Vec<u8> {
    let reference = fixture_bytes("multipart-body.bin");
    let mut out = Vec::new();
    let mut rest = &reference[..];
    while let Some(i) = rest.windows(3).position(|w| w == b"XYZ") {
        out.extend_from_slice(&rest[..i]);
        out.extend_from_slice(boundary.as_bytes());
        rest = &rest[i + 3..];
    }
    out.extend_from_slice(rest);
    out
}

#[test]
fn ef_req_02_multipart_is_sent_like_bruno_with_files_read_from_the_collection() {
    let p = send(&collection(&MULTIPART.replace("HEADERS", ""))).unwrap();
    let [content_type] = content_types(&p)[..] else { panic!("un seul Content-Type attendu") };
    let boundary = content_type.strip_prefix("multipart/form-data; boundary=").unwrap();
    let random = boundary.strip_prefix("--------------------------").unwrap();
    assert!(random.len() == 24 && random.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')), "{boundary}");
    assert_eq!(p.request.body.as_deref(), Some(&bruno_multipart_body(boundary)[..]));
    assert!(p.unresolved.is_empty());
}

#[test]
fn ef_req_02_multipart_keeps_user_boundary_and_sends_nothing_without_enabled_fields() {
    let header = "  headers:\n    - name: Content-Type\n      value: multipart/form-data; boundary=XYZ\n";
    let p = send(&collection(&MULTIPART.replace("HEADERS", header))).unwrap();
    assert_eq!(content_types(&p), ["multipart/form-data; boundary=XYZ"]);
    assert_eq!(p.request.body.unwrap(), bruno_multipart_body("XYZ"));

    let dir = collection("info:\n  name: M\n  type: http\n\nhttp:\n  method: POST\n  url: https://x.test\n  headers:\n    - name: Content-Type\n      value: multipart/mixed\n  body:\n    type: multipart-form\n    data:\n      - name: t\n        type: text\n        value: a\n        disabled: true\n");
    let p = send(&dir).unwrap();
    assert!(content_types(&p)[0].starts_with("multipart/mixed; boundary=--------------------------"));
    assert_eq!(p.request.body, Some(Vec::new()));
}

#[test]
fn ef_req_02_multipart_refuses_files_outside_the_collection() {
    let absolute = std::env::temp_dir().join("notes.txt").to_string_lossy().into_owned();
    for path in ["../secret.txt", absolute.as_str()] {
        let request = MULTIPART.replace("HEADERS", "").replace("files/logo.png", path);
        assert!(matches!(send(&collection(&request)), Err(CoreError::OutsideCollection(p)) if p == path), "{path}");
    }
}
