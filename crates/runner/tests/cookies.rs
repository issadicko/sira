use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::json;
use xc_core::read_request;
use xc_runner::{run_request, Outcome, Request, Session};

type Seen = Arc<Mutex<Vec<String>>>;

/// Répond 200 à toute requête en posant le cookie `sid=abc`, et garde la ligne `Cookie` reçue (ou « - »).
fn serve() -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let cookie = String::from_utf8_lossy(&buf[..n])
                .lines()
                .find(|l| l.to_ascii_lowercase().starts_with("cookie:"))
                .map_or_else(|| "-".to_owned(), str::to_ascii_lowercase);
            log.lock().unwrap().push(cookie);
            let reply =
                "HTTP/1.1 200 OK\r\nSet-Cookie: sid=abc; Path=/\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok";
            let _ = stream.write_all(reply.as_bytes());
        }
    });
    (base, seen)
}

fn indented(script: &str) -> String {
    script.lines().map(|line| format!("        {line}\n")).collect()
}

async fn run(base: &str, kind: &str, script: &str, session: &mut Session) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: S\n").unwrap();
    fs::write(
        dir.path().join("r.yml"),
        format!(
            "info:\n  name: R\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: {base}/ping\n\nruntime:\n  scripts:\n    - type: {kind}\n      code: |-\n{}",
            indented(script)
        ),
    )
    .unwrap();
    let doc = read_request(dir.path(), "r.yml").unwrap();
    let req = Request {
        root: dir.path(),
        path: "r.yml",
        doc: &doc,
        env: None,
        collection_name: "S",
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    run_request(req, session).await
}

#[tokio::test]
async fn ef_scr_02_a_post_response_script_reads_and_edits_the_cookies_of_the_request_url() {
    let (base, _) = serve();
    let script = format!(
        "const jar = bru.cookies.jar();\n\
         bru.setVar('read', [bru.cookies.get('sid'), bru.cookies.count(), bru.cookies.toObject(), bru.cookies.has('sid', 'abc'), bru.cookies.has('sid', 'x'), bru.cookies.toString(), bru.cookies.one('sid').path]);\n\
         const one = await jar.getCookie('{base}/ping', 'sid');\n\
         bru.setVar('one', [one.key, one.value, one.path, one.expires, one.secure, one.httpOnly]);\n\
         await jar.setCookie('{base}/', 'theme', 'dark');\n\
         await bru.cookies.upsert({{ key: 'lang', value: 'fr', path: '/' }});\n\
         bru.setVar('all', bru.cookies.map((c) => c.key + '=' + c.value));\n\
         await bru.cookies.remove('sid');\n\
         bru.setVar('after', bru.cookies.toObject());\n\
         bru.setVar('has', [await jar.hasCookie('{base}/', 'theme'), await jar.hasCookie('{base}/', 'sid')]);\n\
         await bru.cookies.clear();\n\
         bru.setVar('cleared', bru.cookies.count());\n"
    );
    let mut session = Session::default();

    let out = run(&base, "after-response", &script, &mut session).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(session.runtime["read"], json!(["abc", 1, { "sid": "abc" }, true, false, "sid=abc", "/"]));
    assert_eq!(session.runtime["one"], json!(["sid", "abc", "/", "Infinity", false, false]));
    assert_eq!(session.runtime["all"], json!(["sid=abc", "theme=dark", "lang=fr"]));
    assert_eq!(session.runtime["after"], json!({ "theme": "dark", "lang": "fr" }));
    assert_eq!(session.runtime["has"], json!([true, false]));
    assert_eq!(session.runtime["cleared"], json!(0));
    assert!(session.cookies.list().is_empty());
}

#[tokio::test]
async fn ef_scr_02_a_cookie_set_before_the_request_is_sent_with_it() {
    let (base, seen) = serve();
    let script = format!("await bru.cookies.jar().setCookie('{base}/', {{ key: 'pre', value: 'set', path: '/' }});\n");

    let out = run(&base, "before-request", &script, &mut Session::default()).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(seen.lock().unwrap()[0], "cookie: pre=set");
}

#[tokio::test]
async fn ef_scr_02_jar_methods_accept_a_callback_and_report_bad_arguments() {
    let (base, _) = serve();
    let script = format!(
        "const jar = bru.cookies.jar();\n\
         let viaCallback;\n\
         await jar.setCookie('{base}/', 'a', '1', (err) => {{ viaCallback = [err]; }});\n\
         let read;\n\
         await jar.getCookie('{base}/', 'a', (err, cookie) => {{ read = [err, cookie.value]; }});\n\
         let bad;\n\
         try {{ await jar.setCookie('{base}/', 42); }} catch (e) {{ bad = e.message; }}\n\
         let noUrl;\n\
         try {{ await jar.getCookies(''); }} catch (e) {{ noUrl = e.message; }}\n\
         let failed;\n\
         await jar.setCookies('{base}/', 'pas une liste', (err) => {{ failed = err && err.message; }});\n\
         bru.setVar('seen', [viaCallback, read, bad, noUrl, failed]);\n"
    );
    let mut session = Session::default();

    let out = run(&base, "after-response", &script, &mut session).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(
        session.runtime["seen"],
        json!([
            [null],
            [null, "1"],
            "Invalid arguments passed to setCookie",
            "URL is required",
            "setCookies expects an array of cookie objects"
        ])
    );
}

#[tokio::test]
async fn ef_scr_02_jar_urls_are_interpolated_with_the_variables() {
    let (base, _) = serve();
    let script = format!(
        "bru.setVar('host', '{base}');\n\
         await bru.cookies.jar().setCookie('{{{{host}}}}/', 'viaVar', '1');\n\
         bru.setVar('seen', await bru.cookies.jar().hasCookie('{base}/', 'viaVar'));\n"
    );
    let mut session = Session::default();

    let out = run(&base, "after-response", &script, &mut session).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(session.runtime["seen"], json!(true));
}
