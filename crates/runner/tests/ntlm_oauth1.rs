use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use base64::Engine as _;
use xc_core::read_request;
use xc_engine::ntlm;
use xc_runner::{run_request, Request, Session};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

type Seen = Arc<Mutex<Vec<(String, Option<String>)>>>;

fn type2() -> Vec<u8> {
    let mut message = b"NTLMSSP\0".to_vec();
    message.extend_from_slice(&2u32.to_le_bytes());
    message.extend_from_slice(&[0; 8]);
    message.extend_from_slice(&0xe288_8235u32.to_le_bytes());
    message.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    message.extend_from_slice(&[0; 20]);
    message
}

/// Un serveur à une connexion à la fois : sur une même connexion, il répond 401 avec un défi NTLM au type 1, 200 au type 3.
fn serve(ntlm_only: bool) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let log = Arc::clone(&log);
            thread::spawn(move || loop {
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    return;
                }
                let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
                let line = raw.lines().next().unwrap_or_default().to_owned();
                let authorization = raw.lines().find_map(|l| {
                    l.to_ascii_lowercase().starts_with("authorization:").then(|| l[14..].trim().to_owned())
                });
                log.lock().unwrap().push((line, authorization.clone()));
                let reply = if ntlm_only {
                    match authorization.as_deref().and_then(ntlm::token_of).and_then(|t| B64.decode(t).ok()) {
                        Some(bytes) if bytes.get(8) == Some(&1) => {
                            format!(
                                "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM {}\r\nContent-Length: 0\r\n\r\n",
                                B64.encode(type2())
                            )
                        }
                        Some(bytes) if bytes.get(8) == Some(&3) => {
                            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_owned()
                        }
                        _ => "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM\r\nContent-Length: 0\r\n\r\n"
                            .to_owned(),
                    }
                } else {
                    "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok".to_owned()
                };
                if stream.write_all(reply.as_bytes()).is_err() {
                    return;
                }
            });
        }
    });
    (base, seen)
}

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("opencollection.yml"),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: A\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n    - name: user\n      value: ada\n"),
    )
    .unwrap();
    dir
}

async fn run(root: &Path, text: &str) -> xc_runner::Outcome {
    fs::write(root.join("r.yml"), text).unwrap();
    let doc = read_request(root, "r.yml").unwrap();
    run_request(
        Request {
            root,
            path: "r.yml",
            doc: &doc,
            env: None,
            collection_name: "A",
            execution_mode: "cli",
            cancel: Arc::new(AtomicBool::new(false)),
        },
        &mut Session::default(),
    )
    .await
}

const NTLM: &str = "info:\n  name: N\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/n\"\n  auth:\n    type: ntlm\n    username: \"{{user}}\"\n    password: pw\n    domain: CORP\n\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: \"200\"\n";

#[tokio::test]
async fn ef_aut_03_an_ntlm_request_runs_the_exchange_and_passes_its_assertions() {
    let (base, seen) = serve(true);
    let dir = collection(&base);

    let outcome = run(dir.path(), NTLM).await;

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(outcome.passed(), "{:?}", outcome.assertions);
    assert_eq!(outcome.response.as_ref().map(|r| r.status), Some(200));
    let log = seen.lock().unwrap();
    assert_eq!(log.len(), 2);
    assert!(log[0].1.as_deref().unwrap().starts_with("NTLM TlRMTVNTUAAB"), "type 1 : {:?}", log[0].1);
    assert!(log[1].1.as_deref().unwrap().starts_with("NTLM TlRMTVNTUAAD"), "type 3 : {:?}", log[1].1);
}

#[tokio::test]
async fn ef_aut_03_an_oauth1_request_goes_out_signed_in_the_authorization_header() {
    let (base, seen) = serve(false);
    let dir = collection(&base);
    let text = "info:\n  name: O\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/photos?file=vacation.jpg\"\n  auth:\n    type: oauth1\n    consumerKey: ck\n    consumerSecret: cs\n    accessToken: tok\n    accessTokenSecret: ts\n    realm: Photos\n";

    let outcome = run(dir.path(), text).await;

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let sent = seen.lock().unwrap()[0].1.clone().expect("en-tête Authorization");
    assert!(sent.starts_with("OAuth realm=\"Photos\", oauth_consumer_key=\"ck\""), "{sent}");
    for part in
        ["oauth_token=\"tok\"", "oauth_signature_method=\"HMAC-SHA1\"", "oauth_version=\"1.0\"", "oauth_signature=\""]
    {
        assert!(sent.contains(part), "{part} dans {sent}");
    }
    let shown = outcome.sent_headers.iter().find(|(k, _)| k == "Authorization").map(|(_, v)| v.clone());
    assert_eq!(shown.as_deref(), Some(sent.as_str()), "le rapport garde l'en-tête signé");
}

#[tokio::test]
async fn ef_aut_03_an_oauth1_placement_in_the_query_signs_the_url_and_a_bad_key_fails_the_request_with_the_reason() {
    let (base, seen) = serve(false);
    let dir = collection(&base);
    let query = "info:\n  name: O\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/p\"\n  auth:\n    type: oauth1\n    consumerKey: ck\n    consumerSecret: cs\n    placement: query\n";

    let outcome = run(dir.path(), query).await;

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    let line = seen.lock().unwrap()[0].0.clone();
    assert!(line.starts_with("GET /p?oauth_consumer_key=ck&") && line.contains("oauth_signature="), "{line}");
    assert!(seen.lock().unwrap()[0].1.is_none());

    let rsa = "info:\n  name: O\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/p\"\n  auth:\n    type: oauth1\n    consumerKey: ck\n    signatureMethod: RSA-SHA256\n";
    let failed = run(dir.path(), rsa).await;
    let error = failed.error.expect("erreur d'envoi");
    assert!(error.message.contains("clé privée"), "{}", error.message);
}
