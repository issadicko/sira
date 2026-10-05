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
        "request-graphql.yml",
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
fn ef_req_01_editing_a_graphql_request_stays_in_its_own_section() {
    let original = fixture("request-graphql.yml");
    let mut t = tree(&original);
    let previous = RequestDoc::from_tree(&t);
    assert_eq!((previous.method.as_str(), previous.headers.len()), ("POST", 1));
    assert_eq!(previous.url, "https://localhost/api/v2/graphql");

    let mut doc = previous.clone();
    doc.url.push_str("/beta");
    doc.headers[0].value = "def".into();
    doc.apply(&mut t, &previous);

    let saved = yaml::emit(&Value::Map(t), BLANK_BEFORE);
    assert_eq!(saved, original.replace("/graphql\n", "/graphql/beta\n").replace("value: abc", "value: def"));
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

#[test]
fn ef_req_01_lowercase_method_is_read_uppercase_and_left_untouched_on_save() {
    let text =
        "info:\n  name: List users\n  type: http\n  seq: 1\n\nhttp:\n  method: get\n  url: https://x.test/users\n";
    let mut root = tree(text);
    let doc = RequestDoc::from_tree(&root);
    assert_eq!(doc.method, "GET");
    let mut edited = doc.clone();
    edited.url = "https://x.test/users?page=2".into();
    edited.apply(&mut root, &doc);
    let written = yaml::emit(&Value::Map(root), BLANK_BEFORE);
    assert!(written.contains("method: get\n"), "{written}");
}

const OAUTH: &str = "info:\n  name: Token\n  type: http\n\nhttp:\n  method: GET\n  url: https://x.test\n  auth:\n    type: oauth2\n    flow: client_credentials\n    credentials:\n      clientId: id\n      clientSecret: secret\n    scope: read\n";

fn auth_of(text: &str) -> Auth {
    RequestDoc::from_tree(&tree(text)).auth
}

#[test]
fn ef_req_01_an_auth_of_an_untyped_kind_carries_its_whole_configuration() {
    let reordered = "info:\n  name: Token\n  type: http\n\nhttp:\n  method: GET\n  url: https://x.test\n  auth:\n    scope: read\n    credentials:\n      clientSecret: secret\n      clientId: id\n    type: oauth2\n    flow: client_credentials\n";
    assert_eq!(auth_of(OAUTH), auth_of(reordered), "l'ordre des clés ne compte pas");
    let Auth::Other { label, config } = auth_of(OAUTH) else { panic!("oauth2 n'est pas détaillé") };
    assert_eq!(label, "oauth2");
    assert_eq!(
        config, "credentials:\n  clientId: id\n  clientSecret: secret\nflow: client_credentials\nscope: read",
        "clés triées, sans le type"
    );
    for (from, to) in [("clientId: id", "clientId: other"), ("scope: read", "scope: write"), ("    scope: read\n", "")]
    {
        assert_ne!(auth_of(OAUTH), auth_of(&OAUTH.replace(from, to)), "{from} -> {to}");
    }
    assert_ne!(auth_of(OAUTH), auth_of(&OAUTH.replace("oauth2", "digest")));
    let bare = OAUTH.replace("    flow: client_credentials\n    credentials:\n      clientId: id\n      clientSecret: secret\n    scope: read\n", "");
    assert_eq!(auth_of(&bare), Auth::Other { label: "oauth2".into(), config: String::new() });
}

#[test]
fn ef_req_01_a_body_of_an_untyped_kind_carries_its_whole_configuration() {
    let file = |path: &str| {
        format!("info:\n  name: Up\n  type: http\n\nhttp:\n  method: POST\n  url: https://x.test\n  body:\n    type: file\n    data:\n      - filePath: {path}\n        selected: true\n")
    };
    let body = |text: &str| RequestDoc::from_tree(&tree(text)).body;
    assert_eq!(body(&file("a.bin")), body(&file("a.bin")));
    assert_ne!(body(&file("a.bin")), body(&file("b.bin")));
    let Body::Other { label, config } = body(&file("a.bin")) else { panic!("file n'est pas détaillé") };
    assert_eq!((label.as_str(), config.contains("filePath: a.bin")), ("file", true));
}

#[test]
fn ef_req_01_the_json_of_an_untyped_auth_keeps_the_interface_contract() {
    let doc = RequestDoc::from_tree(&tree(OAUTH));
    let json = serde_json::to_value(&doc.auth).unwrap();
    assert_eq!((json["type"].as_str(), json["label"].as_str()), (Some("other"), Some("oauth2")));
    assert!(json["config"].as_str().is_some_and(|config| config.contains("clientId: id")), "{json}");
    let without_config: Auth = serde_json::from_value(serde_json::json!({"type": "other", "label": "oauth2"})).unwrap();
    assert_eq!(without_config, Auth::Other { label: "oauth2".into(), config: String::new() });
    let roundtrip: RequestDoc = serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();
    assert_eq!(roundtrip, doc);
}

#[test]
fn ef_req_02_saving_an_untyped_auth_or_body_changes_nothing_in_the_file() {
    let mut root = tree(OAUTH);
    let before = RequestDoc::from_tree(&root);
    let mut edited = before.clone();
    edited.url = "https://x.test/other".into();
    edited.apply(&mut root, &before);
    let written = yaml::emit(&Value::Map(root), BLANK_BEFORE);
    assert!(written.contains("url: https://x.test/other") && written.contains("clientSecret: secret"), "{written}");

    let mut stripped = before.clone();
    stripped.auth = Auth::Other { label: "oauth2".into(), config: String::new() };
    let mut root = tree(OAUTH);
    stripped.apply(&mut root, &before);
    assert_eq!(yaml::emit(&Value::Map(root), BLANK_BEFORE), OAUTH, "une auth sans config ne réécrit pas l'existante");
}

const RICH: &str = "info:
  name: Rich
  type: http

http:
  method: POST
  url: https://x.test/u/:id
  headers:
    - name: X-A
      value: 1
      description: first
      x-note: note-a
    - name: X-B
      value: b
      disabled: true
      x-note: note-b
  params:
    - name: id
      value: \"7\"
      type: path
      x-param: keep id
  body:
    type: form-urlencoded
    x-body: keep body
    data:
      - name: f
        value: v
        x-field: keep f
  auth:
    type: bearer
    token: abc
    x-auth: keep auth

runtime:
  assertions:
    - expression: res.status
      operator: eq
      value: \"200\"
      x-assert: keep assert
";

fn saved(edit: impl FnOnce(&mut RequestDoc)) -> String {
    let mut root = tree(RICH);
    let before = RequestDoc::from_tree(&root);
    let mut after = before.clone();
    edit(&mut after);
    after.apply(&mut root, &before);
    yaml::emit(&Value::Map(root), BLANK_BEFORE)
}

#[test]
fn ef_req_02_rewriting_a_list_keeps_the_unknown_keys_of_each_entry() {
    let written = saved(|doc| {
        doc.headers[0].value = "2".into();
        doc.headers.push(KeyValue { name: "X-C".into(), value: "c".into(), enabled: true, description: None });
        doc.params[0].value = "8".into();
        doc.body = Body::FormUrlEncoded {
            fields: vec![KeyValue { name: "f".into(), value: "w".into(), enabled: true, description: None }],
        };
        doc.auth = Auth::Bearer { token: "def".into() };
        doc.assertions[0].value = Some("201".into());
    });
    for expected in [
        "      value: \"2\"\n      description: first\n      x-note: note-a\n",
        "      disabled: true\n      x-note: note-b\n",
        "      type: path\n      x-param: keep id\n",
        "    x-body: keep body\n",
        "        value: w\n        x-field: keep f\n",
        "    token: def\n    x-auth: keep auth\n",
        "      value: \"201\"\n      x-assert: keep assert\n",
        "    - name: X-C\n      value: c\n",
    ] {
        assert!(written.contains(expected), "{expected:?} absent de\n{written}");
    }
    assert!(written.contains("value: \"8\"\n      type: path"), "{written}");
}

#[test]
fn ef_req_02_entries_are_matched_by_name_and_kind_and_follow_the_new_order() {
    let written = saved(|doc| doc.headers.reverse());
    let b = written.find("name: X-B").unwrap();
    let a = written.find("name: X-A").unwrap();
    assert!(b < a, "{written}");
    assert!(
        written.contains("disabled: true\n      x-note: note-b") && written.contains("x-note: note-a"),
        "{written}"
    );

    let removed = saved(|doc| {
        doc.headers.remove(0);
    });
    assert!(!removed.contains("note-a") && removed.contains("note-b"), "{removed}");

    let renamed = saved(|doc| doc.headers[0].name = "X-Z".into());
    assert!(renamed.contains("name: X-Z") && !renamed.contains("note-a"), "un élément renommé est un nouvel élément");
}

#[test]
fn ef_req_02_scalars_that_did_not_change_keep_their_form_in_the_file() {
    let written = saved(|doc| doc.headers[0].description = Some("second".into()));
    assert!(written.contains("      value: 1\n      description: second\n"), "value: 1 reste un entier :\n{written}");
    assert!(written.contains("x-note: note-a"), "{written}");
}

#[test]
fn ef_req_02_a_body_or_auth_of_the_same_type_keeps_its_unknown_keys_and_another_type_replaces_them() {
    let written = saved(|doc| doc.auth = Auth::Basic { username: "u".into(), password: "p".into() });
    assert!(
        written.contains("type: basic\n    username: u\n    password: p\n") && !written.contains("keep auth"),
        "{written}"
    );
    let written = saved(|doc| doc.body = Body::Json { data: "{}".into() });
    assert!(written.contains("body:\n    type: json\n    data: \"{}\"\n"), "{written}");
    assert!(!written.contains("keep body"), "{written}");
    let written = saved(|doc| doc.auth = Auth::Bearer { token: "abc".into() });
    assert!(written.contains("x-auth: keep auth"), "inchangée : {written}");
}
