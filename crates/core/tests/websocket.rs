use std::collections::HashMap;
use std::fs;
use std::path::Path;

use xc_core::{prepare_with, read_request, save_request, NetworkPrefs, Overrides, WsMessage};

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
        "opencollection: 1.0.0\n\ninfo:\n  name: WS\n\nrequest:\n  variables:\n    - name: wsBase\n      value: ws://demo.test:9000\n    - name: user\n      value: Ada\n",
    );
    dir
}

const SINGLE: &str = "info:\n  name: Écho\n  type: websocket\n  seq: 1\n\nwebsocket:\n  url: \"{{wsBase}}/echo\"\n  headers:\n    - name: X-Demo\n      value: \"{{user}}\"\n  message:\n    type: json\n    data: '{\"user\":\"{{user}}\"}'\n\nsettings:\n  timeout: 2000\n  keepAliveInterval: 30000\n";

const LIST: &str = "info:\n  name: Liste\n  type: websocket\n  seq: 2\n\nwebsocket:\n  url: \"{{wsBase}}/chat\"\n  message:\n    - title: Salut\n      selected: true\n      message:\n        type: text\n        data: |-\n          hello {{user}}\n          bye\n    - title: Ping\n      selected: false\n      message:\n        type: text\n        data: ping\n";

#[test]
fn ef_ws_01_a_single_message_and_the_settings_are_read() {
    let dir = collection();
    write(dir.path(), "echo.yml", SINGLE);

    let doc = read_request(dir.path(), "echo.yml").unwrap();

    assert_eq!(doc.request_type, "websocket");
    assert_eq!(doc.url, "{{wsBase}}/echo");
    assert_eq!(
        doc.ws_messages,
        [WsMessage {
            title: String::new(),
            selected: true,
            kind: "json".into(),
            data: "{\"user\":\"{{user}}\"}".into()
        }]
    );
    assert_eq!((doc.timeout_ms, doc.keep_alive_ms), (Some(2000), Some(30000)));
}

#[test]
fn ef_ws_01_a_list_of_messages_keeps_titles_selection_and_multiline_data() {
    let dir = collection();
    write(dir.path(), "chat.yml", LIST);

    let doc = read_request(dir.path(), "chat.yml").unwrap();

    let summary: Vec<(&str, bool, &str, &str)> =
        doc.ws_messages.iter().map(|m| (m.title.as_str(), m.selected, m.kind.as_str(), m.data.as_str())).collect();
    assert_eq!(summary, [("Salut", true, "text", "hello {{user}}\nbye"), ("Ping", false, "text", "ping")]);
    assert_eq!((doc.keep_alive_ms, doc.timeout_ms), (None, None));
}

#[test]
fn ef_ws_01_saving_an_unchanged_websocket_request_writes_nothing() {
    let dir = collection();
    for (file, text) in [("echo.yml", SINGLE), ("chat.yml", LIST)] {
        write(dir.path(), file, text);
        let doc = read_request(dir.path(), file).unwrap();
        assert!(!save_request(dir.path(), file, &doc).unwrap());
        assert_eq!(fs::read_to_string(dir.path().join(file)).unwrap(), text);
    }
}

#[test]
fn ef_ws_01_editing_one_message_changes_only_that_message() {
    let dir = collection();
    write(dir.path(), "chat.yml", LIST);
    let mut doc = read_request(dir.path(), "chat.yml").unwrap();
    doc.ws_messages[1].data = "ping 2".into();
    doc.ws_messages[1].selected = true;

    assert!(save_request(dir.path(), "chat.yml", &doc).unwrap());

    let expected = LIST.replace("data: ping\n", "data: ping 2\n").replace("selected: false", "selected: true");
    assert_eq!(fs::read_to_string(dir.path().join("chat.yml")).unwrap(), expected);
}

#[test]
fn ef_ws_01_a_message_added_to_a_single_one_turns_the_file_into_a_list_and_back() {
    let dir = collection();
    write(dir.path(), "echo.yml", SINGLE);
    let mut doc = read_request(dir.path(), "echo.yml").unwrap();
    let original = doc.ws_messages.clone();
    doc.ws_messages[0].title = "JSON".into();
    doc.ws_messages.push(WsMessage {
        title: "Texte".into(),
        selected: false,
        kind: "text".into(),
        data: "salut".into(),
    });
    save_request(dir.path(), "echo.yml", &doc).unwrap();

    let list = read_request(dir.path(), "echo.yml").unwrap();
    assert_eq!(list.ws_messages, doc.ws_messages);
    let text = fs::read_to_string(dir.path().join("echo.yml")).unwrap();
    assert!(
        text.contains("  message:\n    - title: JSON\n      selected: true\n      message:\n        type: json"),
        "{text}"
    );

    let mut back = list;
    back.ws_messages = original.clone();
    save_request(dir.path(), "echo.yml", &back).unwrap();
    assert_eq!(read_request(dir.path(), "echo.yml").unwrap().ws_messages, original);
    assert!(fs::read_to_string(dir.path().join("echo.yml")).unwrap().contains("  message:\n    type: json"));
}

#[test]
fn ef_ws_01_unknown_keys_of_a_message_survive_an_edit() {
    let dir = collection();
    write(
        dir.path(),
        "chat.yml",
        "info:\n  name: C\n  type: websocket\n  seq: 1\n\nwebsocket:\n  url: ws://x.test\n  message:\n    - title: A\n      selected: true\n      color: red\n      message:\n        type: text\n        data: un\n        extra: 1\n",
    );
    let mut doc = read_request(dir.path(), "chat.yml").unwrap();
    doc.ws_messages[0].data = "deux".into();
    save_request(dir.path(), "chat.yml", &doc).unwrap();

    let text = fs::read_to_string(dir.path().join("chat.yml")).unwrap();
    assert!(text.contains("color: red") && text.contains("extra: 1") && text.contains("data: deux"), "{text}");
}

#[test]
fn ef_ws_01_the_keep_alive_interval_is_written_and_removed_with_the_other_settings_untouched() {
    let dir = collection();
    write(dir.path(), "echo.yml", SINGLE);
    let mut doc = read_request(dir.path(), "echo.yml").unwrap();
    doc.keep_alive_ms = Some(45000);
    save_request(dir.path(), "echo.yml", &doc).unwrap();
    let text = fs::read_to_string(dir.path().join("echo.yml")).unwrap();
    assert!(text.contains("timeout: 2000\n  keepAliveInterval: 45000"), "{text}");

    doc.keep_alive_ms = None;
    save_request(dir.path(), "echo.yml", &doc).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("echo.yml")).unwrap(),
        SINGLE.replace("  keepAliveInterval: 30000\n", "")
    );
}

fn prepared(dir: &tempfile::TempDir, file: &str) -> xc_core::Prepared {
    let doc = read_request(dir.path(), file).unwrap();
    let overrides = Overrides { network: Some(NetworkPrefs::default()), ..Overrides::default() };
    prepare_with(dir.path(), file, &doc, None, &HashMap::new(), overrides).unwrap()
}

#[test]
fn ef_ws_01_preparing_resolves_the_address_the_headers_and_every_message() {
    let dir = collection();
    write(dir.path(), "echo.yml", SINGLE);
    write(dir.path(), "chat.yml", LIST);

    let echo = prepared(&dir, "echo.yml");
    assert_eq!(echo.request.url, "ws://demo.test:9000/echo");
    assert_eq!(echo.request.headers, [("X-Demo".to_owned(), "Ada".to_owned())]);
    assert_eq!(echo.messages.len(), 1);
    assert_eq!(echo.messages[0].data, "{\"user\":\"Ada\"}");
    assert!(echo.request.body.is_none() && echo.unresolved.is_empty());

    let chat = prepared(&dir, "chat.yml");
    let titles: Vec<(&str, bool, &str)> =
        chat.messages.iter().map(|m| (m.title.as_str(), m.selected, m.data.as_str())).collect();
    assert_eq!(titles, [("Salut", true, "hello Ada\nbye"), ("Ping", false, "ping")]);
}

#[test]
fn ef_ws_01_a_bearer_token_becomes_the_authorization_header_and_a_bare_host_gets_the_ws_scheme() {
    let dir = collection();
    write(
        dir.path(),
        "auth.yml",
        "info:\n  name: A\n  type: websocket\n  seq: 1\n\nwebsocket:\n  url: demo.test/live?x={{user}}\n  auth:\n    type: bearer\n    token: \"{{user}}-token\"\n",
    );

    let prepared = prepared(&dir, "auth.yml");

    assert_eq!(prepared.request.url, "ws://demo.test/live?x=Ada");
    assert_eq!(prepared.request.headers, [("Authorization".to_owned(), "Bearer Ada-token".to_owned())]);
}

#[test]
fn ef_ws_01_an_undefined_variable_in_a_message_is_reported() {
    let dir = collection();
    write(
        dir.path(),
        "m.yml",
        "info:\n  name: M\n  type: websocket\n  seq: 1\n\nwebsocket:\n  url: ws://x.test\n  message:\n    type: text\n    data: \"{{absente}}\"\n",
    );
    assert_eq!(prepared(&dir, "m.yml").unresolved, ["absente"]);
}
