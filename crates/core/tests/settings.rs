use std::fs;
use std::path::Path;

use xc_core::prepare::redirects_of;
use xc_core::request::BLANK_BEFORE;
use xc_core::yaml::{self, Value};
use xc_core::{normalize, RequestDoc};
use xc_engine::Redirects;

const BARE: &str = "info:\n  name: Bare\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://localhost/\n";

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

fn tree(text: &str) -> yaml::Map {
    match yaml::parse(text).unwrap() {
        Value::Map(m) => m,
        _ => panic!("table attendue"),
    }
}

fn saved(tree: yaml::Map) -> String {
    yaml::emit(&Value::Map(tree), BLANK_BEFORE)
}

#[test]
fn ef_req_04_the_request_settings_are_read() {
    let doc = RequestDoc::from_tree(&tree(&fixture("request-settings.yml")));
    assert_eq!(doc.timeout_ms, Some(2000));
    assert_eq!(
        (doc.follow_redirects, doc.max_redirects, doc.forward_authorization_header),
        (Some(false), Some(3), Some(false))
    );
    let bare = RequestDoc::from_tree(&tree(BARE));
    assert_eq!((bare.follow_redirects, bare.max_redirects, bare.forward_authorization_header), (None, None, None));
}

#[test]
fn ef_req_04_settings_round_trip_without_diff() {
    let original = fixture("request-settings.yml");
    assert_eq!(normalize(&original, BLANK_BEFORE).unwrap(), original);
    let mut t = tree(&original);
    let doc = RequestDoc::from_tree(&t);
    doc.apply(&mut t, &doc.clone());
    assert_eq!(saved(t), original);
}

#[test]
fn ef_req_04_editing_one_setting_changes_one_line() {
    let original = fixture("request-settings.yml");
    let mut t = tree(&original);
    let previous = RequestDoc::from_tree(&t);
    let mut doc = previous.clone();
    doc.max_redirects = Some(10);
    doc.apply(&mut t, &previous);
    assert_eq!(saved(t), original.replace("maxRedirects: 3", "maxRedirects: 10"));
}

#[test]
fn ef_req_04_clearing_the_timeout_writes_zero_like_bruno() {
    let original = fixture("request-settings.yml");
    let mut t = tree(&original);
    let previous = RequestDoc::from_tree(&t);
    let mut doc = previous.clone();
    doc.timeout_ms = None;
    doc.apply(&mut t, &previous);
    assert_eq!(saved(t), original.replace("timeout: 2000", "timeout: 0"));
}

#[test]
fn ef_req_04_settings_are_created_in_a_file_that_had_none() {
    let mut t = tree(BARE);
    let previous = RequestDoc::from_tree(&t);
    let mut doc = previous.clone();
    doc.follow_redirects = Some(false);
    doc.timeout_ms = Some(500);
    doc.apply(&mut t, &previous);
    let text = saved(t);
    assert!(text.ends_with("\nsettings:\n  timeout: 500\n  followRedirects: false\n"), "{text}");
    let reread = RequestDoc::from_tree(&tree(&text));
    assert_eq!((reread.timeout_ms, reread.follow_redirects), (Some(500), Some(false)));
}

#[test]
fn ef_req_04_a_setting_taken_off_the_form_is_removed_from_the_file() {
    let original = fixture("request-settings.yml");
    let mut t = tree(&original);
    let previous = RequestDoc::from_tree(&t);
    let mut doc = previous.clone();
    doc.forward_authorization_header = None;
    doc.apply(&mut t, &previous);
    assert_eq!(saved(t), original.replace("  forwardAuthorizationHeader: false\n", ""));
}

#[test]
fn ef_req_04_the_runtime_defaults_follow_bruno() {
    let bare = RequestDoc::from_tree(&tree(BARE));
    assert_eq!(redirects_of(&bare), Redirects { follow: true, max: 5, forward_authorization: true });
    let doc = RequestDoc::from_tree(&tree(&fixture("request-settings.yml")));
    assert_eq!(redirects_of(&doc), Redirects { follow: false, max: 3, forward_authorization: false });
}
