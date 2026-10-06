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

const NTLM: &str = "info:\n  name: Token\n  type: http\n\nhttp:\n  method: GET\n  url: https://x.test\n  auth:\n    type: ntlm\n    username: u\n    password: p\n    domain: corp\n";

fn auth_of(text: &str) -> Auth {
    RequestDoc::from_tree(&tree(text)).auth
}

#[test]
fn ef_req_01_an_auth_of_an_untyped_kind_carries_its_whole_configuration() {
    let reordered = "info:\n  name: Token\n  type: http\n\nhttp:\n  method: GET\n  url: https://x.test\n  auth:\n    domain: corp\n    password: p\n    type: ntlm\n    username: u\n";
    assert_eq!(auth_of(NTLM), auth_of(reordered), "l'ordre des clés ne compte pas");
    let Auth::Other { label, config } = auth_of(NTLM) else { panic!("ntlm n'est pas détaillé") };
    assert_eq!(label, "ntlm");
    assert_eq!(config, "domain: corp\npassword: p\nusername: u", "clés triées, sans le type");
    for (from, to) in [("username: u", "username: other"), ("domain: corp", "domain: home"), ("    domain: corp\n", "")]
    {
        assert_ne!(auth_of(NTLM), auth_of(&NTLM.replace(from, to)), "{from} -> {to}");
    }
    assert_ne!(auth_of(NTLM), auth_of(&NTLM.replace("ntlm", "wsse")));
    let bare = NTLM.replace("    username: u\n    password: p\n    domain: corp\n", "");
    assert_eq!(auth_of(&bare), Auth::Other { label: "ntlm".into(), config: String::new() });
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
    let doc = RequestDoc::from_tree(&tree(NTLM));
    let json = serde_json::to_value(&doc.auth).unwrap();
    assert_eq!((json["type"].as_str(), json["label"].as_str()), (Some("other"), Some("ntlm")));
    assert!(json["config"].as_str().is_some_and(|config| config.contains("username: u")), "{json}");
    let without_config: Auth = serde_json::from_value(serde_json::json!({"type": "other", "label": "ntlm"})).unwrap();
    assert_eq!(without_config, Auth::Other { label: "ntlm".into(), config: String::new() });
    let roundtrip: RequestDoc = serde_json::from_value(serde_json::to_value(&doc).unwrap()).unwrap();
    assert_eq!(roundtrip, doc);
}

#[test]
fn ef_req_02_saving_an_untyped_auth_or_body_changes_nothing_in_the_file() {
    let mut root = tree(NTLM);
    let before = RequestDoc::from_tree(&root);
    let mut edited = before.clone();
    edited.url = "https://x.test/other".into();
    edited.apply(&mut root, &before);
    let written = yaml::emit(&Value::Map(root), BLANK_BEFORE);
    assert!(written.contains("url: https://x.test/other") && written.contains("password: p"), "{written}");

    let mut stripped = before.clone();
    stripped.auth = Auth::Other { label: "ntlm".into(), config: String::new() };
    let mut root = tree(NTLM);
    stripped.apply(&mut root, &before);
    assert_eq!(yaml::emit(&Value::Map(root), BLANK_BEFORE), NTLM, "une auth sans config ne réécrit pas l'existante");
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

#[test]
fn ef_scr_01_editing_one_script_changes_only_its_code_and_keeps_the_others() {
    let original = fixture("request-full.yml");
    let mut t = tree(&original);
    let previous = RequestDoc::from_tree(&t);
    assert_eq!(previous.scripts.len(), 3);

    let mut doc = previous.clone();
    doc.scripts[0].code = "bru.setVar(\"ts\", 1);".into();
    doc.apply(&mut t, &previous);
    assert_eq!(
        yaml::emit(&Value::Map(t), BLANK_BEFORE),
        original.replace("bru.setVar(\"ts\", Date.now());", "bru.setVar(\"ts\", 1);")
    );
}

#[test]
fn ef_scr_01_a_new_script_is_added_in_order_and_an_emptied_one_is_dropped() {
    let original = fixture("request-minimal.yml");
    let mut t = tree(&original);
    let previous = RequestDoc::from_tree(&t);
    assert!(previous.scripts.is_empty());

    let mut doc = previous.clone();
    doc.scripts.push(xc_core::request::Script {
        kind: "tests".into(),
        code: "test('a', () => {});\ntest('b', () => {});".into(),
    });
    doc.apply(&mut t, &previous);
    let written = yaml::emit(&Value::Map(t), BLANK_BEFORE);
    assert!(written.contains("runtime:\n  scripts:\n    - type: tests\n      code: |-\n        test('a', () => {});\n        test('b', () => {});"), "{written}");

    let mut again = tree(&written);
    let before = RequestDoc::from_tree(&again);
    let mut emptied = before.clone();
    emptied.scripts[0].code = "  \n".into();
    emptied.apply(&mut again, &before);
    assert_eq!(yaml::emit(&Value::Map(again), BLANK_BEFORE), original);
}

const DIGEST: &str = "info:\n  name: D\n  type: http\n\nhttp:\n  method: GET\n  url: https://x.test\n  auth:\n    type: digest\n    username: Mufasa\n    password: secret\n";
const AWS: &str = "info:\n  name: A\n  type: http\n\nhttp:\n  method: GET\n  url: https://x.test\n  auth:\n    type: awsv4\n    accessKeyId: AKID\n    secretAccessKey: key\n    service: s3\n    region: eu-west-1\n";

fn edited(text: &str, change: impl FnOnce(&mut RequestDoc)) -> String {
    let mut root = tree(text);
    let before = RequestDoc::from_tree(&root);
    let mut after = before.clone();
    change(&mut after);
    after.apply(&mut root, &before);
    yaml::emit(&Value::Map(root), BLANK_BEFORE)
}

#[test]
fn ef_aut_01_digest_and_aws_auth_are_read_into_their_own_fields() {
    assert_eq!(auth_of(DIGEST), Auth::Digest { username: "Mufasa".into(), password: "secret".into() });
    assert_eq!(
        auth_of(AWS),
        Auth::Awsv4 {
            access_key_id: "AKID".into(),
            secret_access_key: "key".into(),
            session_token: String::new(),
            service: "s3".into(),
            region: "eu-west-1".into(),
            profile_name: String::new(),
        }
    );
}

#[test]
fn ef_aut_01_editing_one_auth_field_changes_one_line_and_saving_changes_nothing() {
    assert_eq!(edited(DIGEST, |_| {}), DIGEST);
    assert_eq!(edited(AWS, |_| {}), AWS, "les champs absents ne sont pas ajoutés");
    let changed = edited(DIGEST, |d| d.auth = Auth::Digest { username: "Mufasa".into(), password: "other".into() });
    assert_eq!(changed, DIGEST.replace("password: secret", "password: other"));
    let aws = edited(AWS, |d| {
        let Auth::Awsv4 { region, session_token, .. } = &mut d.auth else { panic!("awsv4") };
        *region = "us-east-1".into();
        *session_token = "TOKEN".into();
    });
    assert_eq!(
        aws,
        AWS.replace("region: eu-west-1", "region: us-east-1")
            .replace("    service: s3\n", "    sessionToken: TOKEN\n    service: s3\n")
    );
}

#[test]
fn ef_aut_01_switching_to_digest_or_aws_writes_the_new_table_in_bruno_order() {
    let digest = edited(AWS, |d| d.auth = Auth::Digest { username: "u".into(), password: "p".into() });
    assert!(digest.contains("  auth:\n    type: digest\n    username: u\n    password: p\n"), "{digest}");
    assert!(!digest.contains("accessKeyId"), "{digest}");
    let aws = edited(DIGEST, |d| {
        d.auth = Auth::Awsv4 {
            access_key_id: "A".into(),
            secret_access_key: "S".into(),
            session_token: String::new(),
            service: "s3".into(),
            region: "r".into(),
            profile_name: String::new(),
        }
    });
    assert!(aws.contains("    type: awsv4\n    accessKeyId: A\n    secretAccessKey: S\n    sessionToken: \"\"\n    service: s3\n    region: r\n    profileName: \"\"\n"), "{aws}");
}

#[test]
fn ef_aut_01_the_json_of_digest_and_aws_uses_camel_case_fields() {
    let aws = serde_json::to_value(auth_of(AWS)).unwrap();
    assert_eq!(aws["type"], "awsv4");
    assert_eq!(
        (aws["accessKeyId"].as_str(), aws["secretAccessKey"].as_str(), aws["sessionToken"].as_str()),
        (Some("AKID"), Some("key"), Some(""))
    );
    let back: Auth = serde_json::from_value(aws).unwrap();
    assert_eq!(back, auth_of(AWS));
    assert_eq!(serde_json::to_value(auth_of(DIGEST)).unwrap()["type"], "digest");
}

const OAUTH2: &str = "info:\n  name: T\n  type: http\n\nhttp:\n  method: GET\n  url: https://x.test\n  auth:\n    type: oauth2\n    flow: client_credentials\n    accessTokenUrl: https://x/token\n    credentials:\n      clientId: id\n      clientSecret: secret\n      placement: body\n    scope: read\n    tokenConfig:\n      id: credentials\n      placement:\n        header: Bearer\n      source: access_token\n    settings:\n      autoFetchToken: true\n      autoRefreshToken: false\n";

fn oauth2_edit(text: &str, change: impl FnOnce(&mut xc_core::oauth2::OAuth2)) -> String {
    edited(text, |d| {
        let Auth::Oauth2(config) = &mut d.auth else { panic!("oauth2") };
        change(config);
    })
}

#[test]
fn ef_aut_02_an_oauth2_auth_is_typed_and_saving_it_unchanged_writes_nothing() {
    let Auth::Oauth2(config) = auth_of(OAUTH2) else { panic!("oauth2 est détaillé") };
    assert_eq!(
        (config.flow.as_str(), config.client_id.as_str(), config.scope.as_str()),
        ("client_credentials", "id", "read")
    );
    assert_eq!(edited(OAUTH2, |_| {}), OAUTH2);
    let sparse = OAUTH2.replace("    scope: read\n", "").replace("    settings:\n      autoFetchToken: true\n      autoRefreshToken: false\n", "").replace(
        "    tokenConfig:\n      id: credentials\n      placement:\n        header: Bearer\n      source: access_token\n",
        "",
    );
    assert_eq!(edited(&sparse, |_| {}), sparse, "les réglages absents ne sont pas ajoutés");
}

#[test]
fn ef_aut_02_editing_one_oauth2_field_changes_only_its_key() {
    assert_eq!(oauth2_edit(OAUTH2, |c| c.scope = "write".into()), OAUTH2.replace("scope: read", "scope: write"));
    assert_eq!(
        oauth2_edit(OAUTH2, |c| c.access_token_url = "https://y/token".into()),
        OAUTH2.replace("https://x/token", "https://y/token")
    );
    assert_eq!(
        oauth2_edit(OAUTH2, |c| c.auto_refresh_token = true),
        OAUTH2.replace("autoRefreshToken: false", "autoRefreshToken: true")
    );
    let nested = oauth2_edit(OAUTH2, |c| c.client_secret = "other".into());
    assert_eq!(nested, OAUTH2.replace("clientSecret: secret", "clientSecret: other"));
}

#[test]
fn ef_aut_02_changing_the_flow_replaces_the_whole_table() {
    let code = oauth2_edit(OAUTH2, |c| {
        c.flow = xc_core::oauth2::AUTHORIZATION_CODE.into();
        c.authorization_url = "https://x/auth".into();
        c.pkce = true;
    });
    assert!(
        code.contains(
            "    flow: authorization_code\n    authorizationUrl: https://x/auth\n    accessTokenUrl: https://x/token\n"
        ),
        "{code}"
    );
    assert!(code.contains("    pkce: {}\n"), "{code}");
    let to_oauth = edited(DIGEST, |d| d.auth = Auth::Oauth2(Box::default()));
    assert!(to_oauth.contains("    type: oauth2\n    flow: client_credentials\n"), "{to_oauth}");
    assert!(!to_oauth.contains("username"), "{to_oauth}");
}

#[test]
fn ef_aut_02_the_json_of_an_oauth2_auth_is_flat_with_its_type() {
    let json = serde_json::to_value(auth_of(OAUTH2)).unwrap();
    assert_eq!(
        (json["type"].as_str(), json["flow"].as_str(), json["clientId"].as_str()),
        (Some("oauth2"), Some("client_credentials"), Some("id"))
    );
    assert_eq!(json["tokenPrefix"], "Bearer");
    let back: Auth = serde_json::from_value(json).unwrap();
    assert_eq!(back, auth_of(OAUTH2));
}
