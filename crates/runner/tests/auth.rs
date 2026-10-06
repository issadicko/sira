use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use xc_core::{read_request, RequestDoc};
use xc_engine::digest;
use xc_engine::{aws, millis, HttpRequest};
use xc_runner::{run_request, Outcome, Request, Session};

type Seen = Arc<Mutex<Vec<String>>>;

/// Un serveur qui garde chaque requête reçue en texte brut et répond avec `reply(requête)`.
fn serve(reply: impl Fn(&str) -> (u16, Vec<(String, String)>) + Send + 'static) -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            let (status, headers) = reply(&request);
            log.lock().unwrap().push(request);
            let extra: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
            let body = r#"{"ok":true}"#;
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base, seen)
}

fn header(request: &str, name: &str) -> Option<String> {
    request.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
    })
}

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("opencollection.yml"),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: Secure\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n    - name: secret\n      value: Circle Of Life\n"),
    )
    .unwrap();
    dir
}

fn request(root: &Path, auth: &str) -> (String, RequestDoc) {
    let text = format!("info:\n  name: Secure\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/dir/index.html?b=2&a=1\"\n  auth:\n{auth}\n");
    fs::write(root.join("secure.yml"), text).unwrap();
    ("secure.yml".to_owned(), read_request(root, "secure.yml").unwrap())
}

async fn run(root: &Path, path: &str, doc: &RequestDoc) -> Outcome {
    let req = Request {
        root,
        path,
        doc,
        env: None,
        collection_name: "Secure",
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    run_request(req, &mut Session::default()).await
}

fn field(header: &str, name: &str) -> String {
    let start = header.find(&format!("{name}=")).map(|i| i + name.len() + 1).unwrap();
    let rest = &header[start..];
    rest.strip_prefix('"').map_or_else(
        || rest.split([',', ' ']).next().unwrap_or_default().to_owned(),
        |quoted| quoted.split('"').next().unwrap_or_default().to_owned(),
    )
}

#[tokio::test]
async fn ef_aut_01_digest_answers_the_servers_challenge_and_the_retry_is_accepted() {
    let challenge = r#"Digest realm="shop", qop="auth", nonce="abc123", opaque="op""#;
    let (base, seen) = serve(move |request| {
        let Some(auth) = header(request, "authorization") else {
            return (401, vec![("WWW-Authenticate".into(), challenge.into())]);
        };
        let parsed = digest::parse_challenge(challenge).unwrap();
        let uri = field(&auth, "uri");
        let nc = u32::from_str_radix(&field(&auth, "nc"), 16).unwrap();
        let expected =
            digest::authorization(&parsed, "Mufasa", "Circle Of Life", "GET", &uri, nc, &field(&auth, "cnonce"));
        (if field(&expected, "response") == field(&auth, "response") { 200 } else { 403 }, vec![])
    });
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), "    type: digest\n    username: Mufasa\n    password: \"{{secret}}\"");
    let out = run(dir.path(), &path, &doc).await;

    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.response.as_ref().unwrap().status, 200);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2, "une requête sans identifiants, puis la réponse au défi");
    assert!(header(&seen[0], "authorization").is_none());
    assert!(seen[1].starts_with("GET /dir/index.html?b=2&a=1 "), "{}", seen[1]);
    assert!(
        out.sent_headers.iter().any(|(k, v)| k == "Authorization" && v.starts_with("Digest username=\"Mufasa\"")),
        "{:?}",
        out.sent_headers
    );
}

#[tokio::test]
async fn ef_aut_01_digest_with_a_wrong_password_ends_on_the_servers_refusal() {
    let (base, seen) = serve(|request| match header(request, "authorization") {
        None => (401, vec![("WWW-Authenticate".into(), r#"Digest realm="shop", nonce="n""#.into())]),
        Some(_) => (401, vec![("WWW-Authenticate".into(), r#"Digest realm="shop", nonce="n2", stale=false"#.into())]),
    });
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), "    type: digest\n    username: u\n    password: bad");
    let out = run(dir.path(), &path, &doc).await;

    assert_eq!(out.response.as_ref().unwrap().status, 401, "un seul essai : pas de boucle sur les défis");
    assert_eq!(seen.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn ef_aut_01_a_server_that_does_not_ask_for_digest_gets_a_single_request() {
    let (base, seen) = serve(|_| (200, vec![]));
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), "    type: digest\n    username: u\n    password: p");
    let out = run(dir.path(), &path, &doc).await;

    assert_eq!(out.response.as_ref().unwrap().status, 200);
    assert_eq!(seen.lock().unwrap().len(), 1);
}

fn parse_amz_date(stamp: &str) -> u64 {
    let n = |range: std::ops::Range<usize>| stamp[range].parse::<u64>().unwrap();
    millis(n(0..4) as i64, n(4..6) as u32, n(6..8) as u32, n(9..11), n(11..13), n(13..15))
}

#[tokio::test]
async fn ef_aut_01_aws_signs_the_request_that_is_sent() {
    let (base, seen) = serve(|_| (200, vec![]));
    let dir = collection(&base);
    let auth = "    type: awsv4\n    accessKeyId: AKIDEXAMPLE\n    secretAccessKey: wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY\n    sessionToken: TOKEN\n    service: service\n    region: eu-west-1";
    let (path, doc) = request(dir.path(), auth);
    let out = run(dir.path(), &path, &doc).await;

    assert_eq!(out.response.as_ref().unwrap().status, 200);
    let received = seen.lock().unwrap()[0].clone();
    let authorization = header(&received, "authorization").unwrap();
    assert!(authorization.starts_with("AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/"), "{authorization}");
    assert!(
        authorization.contains("/eu-west-1/service/aws4_request, SignedHeaders=host;x-amz-date;x-amz-security-token,"),
        "{authorization}"
    );
    assert_eq!(header(&received, "x-amz-security-token").as_deref(), Some("TOKEN"));

    let stamp = header(&received, "x-amz-date").unwrap();
    let mut again = HttpRequest {
        method: "GET".into(),
        url: format!("{base}/dir/index.html?b=2&a=1"),
        headers: vec![],
        body: None,
        timeout: std::time::Duration::from_secs(1),
        max_response_body: None,
        network: xc_engine::Network::default(),
    };
    let creds = aws::AwsCredentials {
        access_key_id: "AKIDEXAMPLE".into(),
        secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
        session_token: Some("TOKEN".into()),
        region: "eu-west-1".into(),
        service: "service".into(),
    };
    aws::sign(&mut again, &creds, parse_amz_date(&stamp)).unwrap();
    let expected = again.headers.iter().find(|(k, _)| k == "Authorization").unwrap().1.clone();
    assert_eq!(authorization, expected, "le serveur retrouve la signature avec ce qu'il a reçu");
    assert!(out.sent_headers.iter().any(|(k, _)| k == "Authorization"), "{:?}", out.sent_headers);
}

#[tokio::test]
async fn ef_aut_01_aws_without_keys_fails_before_sending() {
    let (base, seen) = serve(|_| (200, vec![]));
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), "    type: awsv4\n    service: s3");
    let out = run(dir.path(), &path, &doc).await;

    assert!(out.response.is_none());
    let message = out.error.unwrap().message;
    assert!(message.contains("requis"), "{message}");
    assert!(seen.lock().unwrap().is_empty());
}

#[tokio::test]
async fn ef_aut_01_aws_reads_keys_from_a_named_profile() {
    let (base, seen) = serve(|_| (200, vec![]));
    let dir = collection(&base);
    let file = dir.path().join("aws-credentials");
    fs::write(&file, "[sira-test]\naws_access_key_id = AKPROFILE\naws_secret_access_key = secretprofile\n").unwrap();
    std::env::set_var("AWS_SHARED_CREDENTIALS_FILE", &file);
    let (path, doc) =
        request(dir.path(), "    type: awsv4\n    service: service\n    region: us-east-1\n    profileName: sira-test");
    let out = run(dir.path(), &path, &doc).await;
    let missing = {
        let (path, doc) = request(dir.path(), "    type: awsv4\n    service: service\n    profileName: introuvable");
        run(dir.path(), &path, &doc).await
    };
    std::env::remove_var("AWS_SHARED_CREDENTIALS_FILE");

    assert_eq!(out.response.as_ref().unwrap().status, 200, "{:?}", out.error);
    assert!(header(&seen.lock().unwrap()[0], "authorization").unwrap().contains("Credential=AKPROFILE/"));
    assert!(missing.error.unwrap().message.contains("profil AWS « introuvable »"));
}
