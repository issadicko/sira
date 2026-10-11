use std::collections::HashMap;
use std::fs;
use std::path::Path;

use xc_core::{prepare_with, read_request, save_request, Auth, Overrides, PrivateKey, SendAuth};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn collection() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "opencollection.yml",
        "opencollection: 1.0.0\n\ninfo:\n  name: A\n\nrequest:\n  variables:\n    - name: base\n      value: http://api.test\n    - name: ck\n      value: cle-consommateur\n    - name: user\n      value: ada\n",
    );
    dir
}

const NTLM: &str = "info:\n  name: N\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/n\"\n  auth:\n    type: ntlm\n    username: \"{{user}}\"\n    password: pw\n    domain: CORP\n";

const OAUTH1: &str = "info:\n  name: O\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{base}}/o\"\n  auth:\n    type: oauth1\n    consumerKey: \"{{ck}}\"\n    consumerSecret: cs\n    accessToken: tok\n    accessTokenSecret: ts\n    signatureMethod: HMAC-SHA256\n    placement: query\n    includeBodyHash: true\n";

#[test]
fn ef_aut_03_ntlm_and_oauth1_are_read_from_the_file() {
    let dir = collection();
    write(dir.path(), "n.yml", NTLM);
    write(dir.path(), "o.yml", OAUTH1);

    let ntlm = read_request(dir.path(), "n.yml").unwrap();
    assert_eq!(ntlm.auth, Auth::Ntlm { username: "{{user}}".into(), password: "pw".into(), domain: "CORP".into() });

    let Auth::Oauth1(oauth) = read_request(dir.path(), "o.yml").unwrap().auth else { panic!("oauth1 attendu") };
    assert_eq!((oauth.consumer_key.as_str(), oauth.access_token_secret.as_str()), ("{{ck}}", "ts"));
    assert_eq!((oauth.signature_method.as_str(), oauth.placement.as_str()), ("HMAC-SHA256", "query"));
    assert!(oauth.include_body_hash && oauth.callback_url.is_empty() && oauth.private_key == PrivateKey::default());
}

#[test]
fn ef_aut_03_saving_an_unchanged_ntlm_or_oauth1_request_writes_nothing() {
    let dir = collection();
    for (file, text) in [("n.yml", NTLM), ("o.yml", OAUTH1)] {
        write(dir.path(), file, text);
        let doc = read_request(dir.path(), file).unwrap();
        assert!(!save_request(dir.path(), file, &doc).unwrap());
        assert_eq!(fs::read_to_string(dir.path().join(file)).unwrap(), text);
    }
}

#[test]
fn ef_aut_03_editing_an_oauth1_field_changes_only_that_field_and_empty_fields_stay_out_of_the_file() {
    let dir = collection();
    write(dir.path(), "o.yml", OAUTH1);
    let mut doc = read_request(dir.path(), "o.yml").unwrap();
    let Auth::Oauth1(config) = &mut doc.auth else { panic!() };
    config.signature_method = "RSA-SHA1".into();
    config.private_key = PrivateKey { kind: "file".into(), value: "/home/ada/.keys/oauth.pem".into() };
    config.verifier = String::new();
    config.include_body_hash = false;

    assert!(save_request(dir.path(), "o.yml", &doc).unwrap());

    let saved = fs::read_to_string(dir.path().join("o.yml")).unwrap();
    assert!(saved.contains("signatureMethod: RSA-SHA1\n"), "{saved}");
    assert!(saved.contains("privateKey:\n      type: file\n      value: /home/ada/.keys/oauth.pem\n"), "{saved}");
    assert!(
        !saved.contains("includeBodyHash") && !saved.contains("verifier") && !saved.contains("callbackUrl"),
        "{saved}"
    );
    assert!(saved.contains("consumerKey: \"{{ck}}\"\n"), "un champ intact garde sa forme : {saved}");
    let Auth::Oauth1(again) = read_request(dir.path(), "o.yml").unwrap().auth else { panic!() };
    assert_eq!(again.private_key.kind, "file");
}

#[test]
fn ef_aut_03_switching_a_request_to_ntlm_writes_the_three_fields() {
    let dir = collection();
    write(
        dir.path(),
        "n.yml",
        "info:\n  name: N\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/n\"\n",
    );
    let mut doc = read_request(dir.path(), "n.yml").unwrap();
    doc.auth = Auth::Ntlm { username: "ada".into(), password: "pw".into(), domain: String::new() };

    save_request(dir.path(), "n.yml", &doc).unwrap();

    let saved = fs::read_to_string(dir.path().join("n.yml")).unwrap();
    assert!(saved.contains("type: ntlm") && saved.contains("username: ada") && saved.contains("domain:"), "{saved}");
}

#[test]
fn ef_aut_03_preparing_resolves_the_variables_of_ntlm_and_oauth1_and_sets_no_header_itself() {
    let dir = collection();
    write(dir.path(), "n.yml", NTLM);
    write(dir.path(), "o.yml", OAUTH1);
    let prepare = |file: &str| {
        let doc = read_request(dir.path(), file).unwrap();
        prepare_with(dir.path(), file, &doc, None, &HashMap::new(), Overrides::default()).unwrap()
    };

    let ntlm = prepare("n.yml");
    assert_eq!(ntlm.auth, SendAuth::Ntlm { username: "ada".into(), password: "pw".into(), domain: "CORP".into() });
    assert!(!ntlm.request.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("authorization")));

    let oauth = prepare("o.yml");
    let SendAuth::Oauth1(settings) = oauth.auth else { panic!("oauth1 attendu") };
    assert_eq!((settings.consumer_key.as_str(), settings.placement.as_str()), ("cle-consommateur", "query"));
    assert!(settings.include_body_hash);
    assert!(!oauth.request.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("authorization")));
}

#[test]
fn ef_aut_03_the_oauth1_private_key_is_read_from_text_or_from_a_file_and_a_missing_file_is_an_error() {
    let dir = collection();
    let oauth = |key: &str| {
        format!("info:\n  name: O\n  type: http\n  seq: 2\n\nhttp:\n  method: GET\n  url: \"{{{{base}}}}/o\"\n  auth:\n    type: oauth1\n    consumerKey: ck\n    signatureMethod: RSA-SHA1\n    privateKey:\n{key}")
    };
    let prepare = |text: String| {
        write(dir.path(), "o.yml", &text);
        let doc = read_request(dir.path(), "o.yml").unwrap();
        prepare_with(dir.path(), "o.yml", &doc, None, &HashMap::new(), Overrides::default())
    };
    let key_of = |p: xc_core::Prepared| match p.auth {
        SendAuth::Oauth1(s) => s.private_key,
        other => panic!("{other:?}"),
    };

    assert_eq!(key_of(prepare(oauth("      type: text\n      value: PEM-EN-LIGNE\n")).unwrap()), "PEM-EN-LIGNE");

    write(dir.path(), "cles/oauth.pem", "PEM-DU-FICHIER\n");
    assert_eq!(key_of(prepare(oauth("      type: file\n      value: cles/oauth.pem\n")).unwrap()), "PEM-DU-FICHIER\n");
    let absolute = dir.path().join("cles/oauth.pem").display().to_string();
    assert_eq!(
        key_of(prepare(oauth(&format!("      type: file\n      value: '{absolute}'\n"))).unwrap()),
        "PEM-DU-FICHIER\n"
    );

    let error = match prepare(oauth("      type: file\n      value: absent.pem\n")) {
        Ok(_) => panic!("une clé introuvable doit échouer"),
        Err(error) => error.to_string(),
    };
    assert!(error.contains("clé privée OAuth 1.0 illisible"), "{error}");
}

#[test]
fn ef_aut_03_a_folder_can_give_ntlm_to_the_requests_that_inherit() {
    let dir = collection();
    write(dir.path(), "api/folder.yml", "info:\n  name: api\n  type: folder\nrequest:\n  auth:\n    type: ntlm\n    username: dossier\n    password: pw\n    domain: D\n");
    write(
        dir.path(),
        "api/r.yml",
        "info:\n  name: R\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{base}}/r\"\n  auth: inherit\n",
    );
    let doc = read_request(dir.path(), "api/r.yml").unwrap();

    let prepared = prepare_with(dir.path(), "api/r.yml", &doc, None, &HashMap::new(), Overrides::default()).unwrap();

    assert_eq!(prepared.auth, SendAuth::Ntlm { username: "dossier".into(), password: "pw".into(), domain: "D".into() });
}
