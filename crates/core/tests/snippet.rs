use std::collections::HashMap;
use std::fs;
use std::path::Path;

use xc_codegen::{Auth as SnippetAuth, Body as SnippetBody, PartValue};
use xc_core::prepare::{snippet, Overrides};
use xc_core::vars::ScopeOverrides;
use xc_core::{read_request, RequestDoc};

fn collection(request: &str) -> (tempfile::TempDir, RequestDoc) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("opencollection.yml"),
        "opencollection: 1.0.0\n\ninfo:\n  name: S\n\nrequest:\n  headers:\n    - name: X-Client\n      value: sira\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n",
    )
    .unwrap();
    fs::create_dir(root.join("environments")).unwrap();
    fs::write(
        root.join("environments/dev.yml"),
        "name: dev\nvariables:\n  - name: base\n    value: https://shop.test\n  - name: who\n    value: Ada\n  - secret: true\n    name: token\n",
    )
    .unwrap();
    fs::write(root.join("req.yml"), request).unwrap();
    let doc = read_request(root, "req.yml").unwrap();
    (dir, doc)
}

fn build(root: &Path, doc: &RequestDoc, overrides: Overrides) -> (xc_codegen::Snippet, Vec<String>) {
    snippet(root, "req.yml", doc, Some("dev"), &HashMap::new(), overrides).unwrap()
}

fn secrets_as_placeholders() -> Overrides {
    Overrides {
        headers: None,
        vars: ScopeOverrides { secrets: vec![("token".into(), "<token>".into())], ..ScopeOverrides::default() },
    }
}

#[test]
fn ef_gen_01_the_snippet_carries_the_resolved_request_headers_and_body() {
    let (dir, doc) = collection(
        "info:\n  name: Create\n  type: http\n\nhttp:\n  method: POST\n  url: \"{{base}}/users?who={{who}}\"\n  headers:\n    - name: Accept\n      value: application/json\n  body:\n    type: json\n    data: '{\"name\": \"{{who}}\"}'\n  auth: inherit\n",
    );
    let (snippet, unresolved) = build(dir.path(), &doc, secrets_as_placeholders());
    assert_eq!((snippet.method.as_str(), snippet.url.as_str()), ("POST", "https://shop.test/users?who=Ada"));
    assert!(unresolved.is_empty(), "{unresolved:?}");
    let headers: Vec<_> = snippet.headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert!(
        headers.contains(&("X-Client", "sira")) && headers.contains(&("Accept", "application/json")),
        "{headers:?}"
    );
    assert!(headers.contains(&("Authorization", "Bearer <token>")), "le secret est un repère : {headers:?}");
    assert!(headers.contains(&("Content-Type", "application/json")), "{headers:?}");
    assert_eq!(snippet.body, SnippetBody::Raw("{\"name\": \"Ada\"}".into()));
}

#[test]
fn ef_gen_01_a_secret_without_a_value_stays_visible_as_unresolved() {
    let (dir, doc) = collection("info:\n  name: Get\n  type: http\n\nhttp:\n  method: GET\n  url: \"{{base}}/me?x={{nope}}\"\n  auth: inherit\n");
    let (snippet, unresolved) = build(dir.path(), &doc, Overrides::default());
    assert!(
        snippet.headers.iter().any(|(k, v)| k == "Authorization" && v == "Bearer {{token}}"),
        "{:?}",
        snippet.headers
    );
    assert!(unresolved.contains(&"nope".to_owned()) && unresolved.contains(&"token".to_owned()), "{unresolved:?}");
}

#[test]
fn ef_gen_01_multipart_keeps_file_paths_without_reading_them_and_drops_the_content_type() {
    let (dir, doc) = collection(
        "info:\n  name: Up\n  type: http\n\nhttp:\n  method: POST\n  url: \"{{base}}/upload\"\n  headers:\n    - name: Content-Type\n      value: multipart/form-data\n  body:\n    type: multipart-form\n    data:\n      - name: title\n        type: text\n        value: \"Hi {{who}}\"\n      - name: pics\n        type: file\n        value:\n          - missing/a.png\n          - missing/b.png\n        contentType: image/png\n  auth: none\n",
    );
    let (snippet, unresolved) = build(dir.path(), &doc, Overrides::default());
    assert!(unresolved.is_empty(), "{unresolved:?}");
    assert!(!snippet.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")), "{:?}", snippet.headers);
    let SnippetBody::Multipart(parts) = snippet.body else { panic!("multipart attendu") };
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0].value, PartValue::Text("Hi Ada".into()));
    assert_eq!(
        (parts[1].value.clone(), parts[1].content_type.as_deref()),
        (PartValue::File("missing/a.png".into()), Some("image/png"))
    );
    assert_eq!(parts[2].value, PartValue::File("missing/b.png".into()));
}

#[test]
fn ef_gen_01_digest_and_oauth2_are_reported_without_leaking_a_token() {
    let (dir, doc) = collection(
        "info:\n  name: D\n  type: http\n\nhttp:\n  method: GET\n  url: \"{{base}}/d\"\n  auth:\n    type: digest\n    username: ada\n    password: \"{{who}}\"\n",
    );
    let (snippet, _) = build(dir.path(), &doc, Overrides::default());
    assert_eq!(snippet.auth, SnippetAuth::Digest { username: "ada".into(), password: "Ada".into() });

    let (dir, doc) = collection(
        "info:\n  name: O\n  type: http\n\nhttp:\n  method: GET\n  url: \"{{base}}/o\"\n  auth:\n    type: oauth2\n    flow: client_credentials\n    accessTokenUrl: \"{{base}}/token\"\n    credentials:\n      clientId: shop\n",
    );
    let (snippet, _) = build(dir.path(), &doc, Overrides::default());
    assert_eq!(snippet.auth, SnippetAuth::None);
    assert!(snippet.notes.iter().any(|n| n.contains("OAuth 2.0")), "{:?}", snippet.notes);
    assert!(!snippet.headers.iter().any(|(k, _)| k == "Authorization"));
}
