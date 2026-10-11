use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use xc_engine::ntlm::{self, Challenge};
use xc_engine::{HttpRequest, Network, NtlmCredentials};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;
const SERVER_CHALLENGE: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];

#[derive(Debug, Default, Clone)]
struct Seen {
    connection: usize,
    request_line: String,
    authorization: Option<String>,
    body: String,
}

type Log = Arc<Mutex<Vec<Seen>>>;

fn type2() -> Vec<u8> {
    let mut message = Vec::new();
    message.extend_from_slice(b"NTLMSSP\0");
    message.extend_from_slice(&2u32.to_le_bytes());
    message.extend_from_slice(&[0; 8]);
    message.extend_from_slice(&0xe288_8235u32.to_le_bytes());
    message.extend_from_slice(&SERVER_CHALLENGE);
    message.extend_from_slice(&[0; 8]);
    message.extend_from_slice(&[0; 4]);
    message.extend_from_slice(&56u32.to_le_bytes());
    message.extend_from_slice(&[0; 8]);
    message
}

fn field(message: &[u8], at: usize) -> Vec<u8> {
    let len = usize::from(u16::from_le_bytes([message[at], message[at + 1]]));
    let offset = u32::from_le_bytes(message[at + 4..at + 8].try_into().unwrap()) as usize;
    message[offset..offset + len].to_vec()
}

/// Vrai quand le message de type 3 prouve la connaissance du mot de passe `password` pour `user` et `domain`.
fn proves(message: &[u8], password: &str, user: &str, domain: &str) -> bool {
    if message.len() < 64
        || &message[..8] != b"NTLMSSP\0"
        || u32::from_le_bytes(message[8..12].try_into().unwrap()) != 3
    {
        return false;
    }
    let nt = field(message, 20);
    let (proof, blob) = nt.split_at(16);
    let (timestamp, client): ([u8; 8], [u8; 8]) = (blob[8..16].try_into().unwrap(), blob[16..24].try_into().unwrap());
    let challenge =
        Challenge { flags: 0, server_challenge: SERVER_CHALLENGE, target_info: Vec::new(), timestamp: None };
    let (_, expected) = ntlm::responses_v2(&challenge, password, user, domain, client, timestamp);
    expected[..16] == *proof
}

async fn read_request(stream: &mut TcpStream) -> Option<(String, Option<String>, String)> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&data).into_owned();
        if let Some(at) = text.find("\r\n\r\n") {
            let head = &text[..at];
            let length = head
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                })
                .unwrap_or(0);
            if text.len() >= at + 4 + length {
                let authorization = head.lines().find_map(|l| {
                    l.to_ascii_lowercase().starts_with("authorization:").then(|| l[14..].trim().to_owned())
                });
                return Some((
                    head.lines().next().unwrap_or_default().to_owned(),
                    authorization,
                    text[at + 4..at + 4 + length].to_owned(),
                ));
            }
        }
    }
}

/// Un serveur NTLM : 401 avec défi à la négociation, 200 si la preuve du type 3 est bonne pour `password`, 401 sinon.
async fn serve(password: &'static str, asks: bool) -> (String, Log, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::default();
    let connections = Arc::new(AtomicUsize::new(0));
    let (shared, count) = (log.clone(), connections.clone());
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let (log, id) = (shared.clone(), count.fetch_add(1, Ordering::SeqCst) + 1);
            tokio::spawn(async move {
                while let Some((line, authorization, body)) = read_request(&mut stream).await {
                    log.lock().unwrap().push(Seen {
                        connection: id,
                        request_line: line,
                        authorization: authorization.clone(),
                        body,
                    });
                    let token =
                        authorization.as_deref().and_then(|a| ntlm::token_of(a)).and_then(|t| B64.decode(t).ok());
                    let response = match (&token, asks) {
                        (_, false) => "HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\npublic".to_owned(),
                        (None, true) => {
                            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM\r\nContent-Length: 0\r\n\r\n"
                                .to_owned()
                        }
                        (Some(bytes), true) if bytes.get(8) == Some(&1) => format!(
                            "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM {}\r\nContent-Length: 4\r\n\r\nnope",
                            B64.encode(type2())
                        ),
                        (Some(bytes), true) if proves(bytes, password, "ada", "CORP") => {
                            "HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nbienvenue".to_owned()
                        }
                        _ => "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM\r\nContent-Length: 6\r\n\r\nrefusé"
                            .to_owned(),
                    };
                    if stream.write_all(response.as_bytes()).await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (base, log, connections)
}

fn credentials(password: &str) -> NtlmCredentials {
    NtlmCredentials {
        username: "ada".into(),
        password: password.into(),
        domain: "CORP".into(),
        workstation: "PC1".into(),
    }
}

fn request(url: String, method: &str, body: Option<&str>) -> HttpRequest {
    HttpRequest {
        method: method.into(),
        url,
        headers: vec![("Content-Type".into(), "text/plain".into())],
        body: body.map(|b| b.as_bytes().to_vec()),
        timeout: Duration::from_secs(5),
        max_response_body: None,
        network: Network::default(),
    }
}

#[tokio::test]
async fn ef_aut_03_ntlm_negotiates_on_one_connection_and_the_real_request_goes_with_the_proof() {
    let (base, log, connections) = serve("secret", true).await;

    let response =
        xc_engine::send_ntlm(request(format!("{base}/rapport"), "POST", Some("données")), &credentials("secret"))
            .await
            .unwrap();

    assert_eq!((response.status, String::from_utf8_lossy(&response.body).as_ref()), (200, "bienvenue"));
    assert_eq!(connections.load(Ordering::SeqCst), 1, "NTLM lie l'échange à une connexion");
    let seen = log.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert!(seen.iter().all(|s| s.connection == 1 && s.request_line.starts_with("POST /rapport")));
    assert_eq!(seen[0].body, "", "la négociation part sans corps");
    assert_eq!(seen[1].body, "données", "la requête entière part avec la réponse au défi");
    assert!(seen[0].authorization.as_deref().unwrap().starts_with("NTLM TlRMTVNTUAAB"));
}

#[tokio::test]
async fn ef_aut_03_a_wrong_password_is_refused_by_the_server_and_the_401_is_returned() {
    let (base, _, _) = serve("secret", true).await;

    let response =
        xc_engine::send_ntlm(request(format!("{base}/x"), "GET", None), &credentials("mauvais")).await.unwrap();

    assert_eq!(response.status, 401);
}

#[tokio::test]
async fn ef_aut_03_a_server_that_does_not_ask_for_ntlm_answers_the_first_request_as_is() {
    let (base, log, _) = serve("secret", false).await;

    let response =
        xc_engine::send_ntlm(request(format!("{base}/x"), "GET", None), &credentials("secret")).await.unwrap();

    assert_eq!((response.status, response.body.as_slice()), (200, b"public".as_slice()));
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn ef_aut_03_a_challenge_that_cannot_be_read_is_an_error() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                while read_request(&mut stream).await.is_some() {
                    let _ = stream.write_all(b"HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: NTLM bm9wZQ==\r\nContent-Length: 0\r\n\r\n").await;
                }
            });
        }
    });

    let error =
        xc_engine::send_ntlm(request(format!("{base}/x"), "GET", None), &credentials("secret")).await.unwrap_err();

    assert!(error.to_string().contains("défi NTLM illisible"), "{error}");
}

#[test]
fn ef_aut_03_the_debug_output_of_ntlm_credentials_hides_the_password() {
    let shown = format!("{:?}", credentials("très-secret"));
    assert!(!shown.contains("très-secret") && shown.contains("ada"), "{shown}");
}
