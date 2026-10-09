#![allow(clippy::result_large_err)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{authority, issue};
use futures_util::{SinkExt, StreamExt};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::http::StatusCode;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::Message;
use xc_engine::{connect_ws, Cookies, EngineError, Network, Proxy, ProxyScheme, Tls, WsEvent, WsOutgoing, WsRequest};

/// Ce que le serveur a vu d'une connexion.
#[derive(Debug, Default, Clone)]
struct Seen {
    path: String,
    headers: Vec<(String, String)>,
    pings: usize,
    closed_with: Option<u16>,
}

type Log = Arc<Mutex<Vec<Seen>>>;

fn header(seen: &Seen, name: &str) -> Option<String> {
    seen.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
}

fn reject(status: StatusCode, body: &str) -> ErrorResponse {
    let mut response = ErrorResponse::new(Some(body.to_owned()));
    *response.status_mut() = status;
    response
}

/// Sert le WebSocket sur `stream` selon le chemin demandé : `/echo`, `/greet` (salue d'abord), `/close` (ferme avec
/// 4000 « bye »), `/reject` (401), `/proto` (choisit le sous-protocole « chat »), `/big` (renvoie 2 Mio), `/silent`.
async fn serve_socket<S>(stream: S, log: Log)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let index = {
        let mut log = log.lock().unwrap();
        log.push(Seen::default());
        log.len() - 1
    };
    let remember = log.clone();
    let callback = move |request: &Request, mut response: Response| -> Result<Response, ErrorResponse> {
        let path = request.uri().path().to_owned();
        {
            let mut log = remember.lock().unwrap();
            log[index].path = path.clone();
            log[index].headers = request
                .headers()
                .iter()
                .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
                .collect();
        }
        if path == "/reject" {
            return Err(reject(StatusCode::UNAUTHORIZED, "jeton manquant"));
        }
        if path == "/proto" {
            response.headers_mut().insert("sec-websocket-protocol", "chat".parse().unwrap());
        }
        if path == "/cookie" {
            response.headers_mut().insert("set-cookie", "session=abc; Path=/".parse().unwrap());
        }
        Ok(response)
    };
    let Ok(mut socket) = tokio_tungstenite::accept_hdr_async(stream, callback).await else { return };
    let path = log.lock().unwrap()[index].path.clone();
    match path.as_str() {
        "/greet" => {
            let _ = socket.send(Message::text("bonjour")).await;
        }
        "/close" => {
            let frame = CloseFrame { code: CloseCode::from(4000), reason: "bye".into() };
            let _ = socket.send(Message::Close(Some(frame))).await;
        }
        "/big" => {
            let _ = socket.send(Message::binary(vec![7u8; 2 << 20])).await;
        }
        _ => {}
    }
    while let Some(Ok(message)) = socket.next().await {
        match message {
            Message::Text(_) | Message::Binary(_) if path != "/silent" => {
                let _ = socket.send(message).await;
            }
            Message::Ping(_) => log.lock().unwrap()[index].pings += 1,
            Message::Close(frame) => log.lock().unwrap()[index].closed_with = frame.map(|f| u16::from(f.code)),
            _ => {}
        }
    }
}

async fn serve() -> (u16, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log: Log = Arc::default();
    let shared = log.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve_socket(stream, shared.clone()));
        }
    });
    (port, log)
}

fn request(url: String) -> WsRequest {
    WsRequest {
        url,
        headers: vec![],
        timeout: Duration::from_secs(5),
        keep_alive: None,
        max_message: None,
        network: Network::default(),
    }
}

async fn next(connection: &mut xc_engine::WsConnection) -> WsEvent {
    tokio::time::timeout(Duration::from_secs(5), connection.next()).await.expect("un événement").expect("la connexion")
}

#[tokio::test]
async fn ef_ws_01_a_text_message_goes_to_the_server_and_comes_back() {
    let (port, _) = serve().await;
    let mut connection = connect_ws(request(format!("ws://127.0.0.1:{port}/echo"))).await.unwrap();

    assert_eq!(connection.opened.status, 101);
    assert_eq!(connection.opened.remote_addr, format!("127.0.0.1:{port}"));
    connection.sender.text("salut").unwrap();
    assert_eq!(next(&mut connection).await, WsEvent::Text { data: "salut".into() });
    connection.sender.send(WsOutgoing::Binary(vec![1, 2, 3])).unwrap();
    assert_eq!(next(&mut connection).await, WsEvent::Binary { data: vec![1, 2, 3] });
}

#[tokio::test]
async fn ef_ws_01_a_message_the_server_sends_first_is_received_without_sending_anything() {
    let (port, _) = serve().await;
    let mut connection = connect_ws(request(format!("ws://127.0.0.1:{port}/greet"))).await.unwrap();
    assert_eq!(next(&mut connection).await, WsEvent::Text { data: "bonjour".into() });
}

#[tokio::test]
async fn ef_ws_01_the_client_closes_with_a_code_and_the_events_end_with_the_close() {
    let (port, log) = serve().await;
    let mut connection = connect_ws(request(format!("ws://127.0.0.1:{port}/echo"))).await.unwrap();

    connection.sender.send(WsOutgoing::Close { code: 1001, reason: "je pars".into() }).unwrap();

    let closed = next(&mut connection).await;
    assert!(matches!(closed, WsEvent::Close { code: Some(1001), .. }), "{closed:?}");
    assert!(connection.next().await.is_none(), "plus rien après la fermeture");
    assert_eq!(log.lock().unwrap()[0].closed_with, Some(1001));
    assert!(connection.sender.text("trop tard").is_err());
}

#[tokio::test]
async fn ef_ws_01_a_close_from_the_server_gives_its_code_and_reason() {
    let (port, _) = serve().await;
    let mut connection = connect_ws(request(format!("ws://127.0.0.1:{port}/close"))).await.unwrap();
    assert_eq!(next(&mut connection).await, WsEvent::Close { code: Some(4000), reason: "bye".into() });
}

#[tokio::test]
async fn ef_ws_01_a_refused_upgrade_says_the_status_and_the_body() {
    let (port, _) = serve().await;
    let error = connect_ws(request(format!("ws://127.0.0.1:{port}/reject"))).await.unwrap_err();
    let message = error.to_string();
    assert!(matches!(error, EngineError::WebSocket(_)), "{message}");
    assert!(message.contains("401") && message.contains("jeton manquant"), "{message}");
}

#[tokio::test]
async fn ef_ws_01_the_request_headers_reach_the_server_and_the_chosen_subprotocol_comes_back() {
    let (port, log) = serve().await;
    let mut wanted = request(format!("ws://127.0.0.1:{port}/proto"));
    wanted.headers = vec![
        ("X-Demo-Client".into(), "sira".into()),
        ("Sec-WebSocket-Protocol".into(), "chat, superchat".into()),
        ("Host".into(), "ne-doit-pas-casser".into()),
    ];

    let connection = connect_ws(wanted).await.unwrap();

    assert_eq!(connection.opened.protocol.as_deref(), Some("chat"));
    let seen = log.lock().unwrap()[0].clone();
    assert_eq!(header(&seen, "x-demo-client").as_deref(), Some("sira"));
    assert_eq!(header(&seen, "sec-websocket-protocol").as_deref(), Some("chat, superchat"));
    assert_ne!(header(&seen, "host").as_deref(), Some("ne-doit-pas-casser"), "la mise à niveau garde son Host");
}

#[tokio::test]
async fn ef_ws_01_cookies_of_the_jar_are_sent_and_the_ones_the_server_sets_are_kept() {
    let (port, log) = serve().await;
    let jar = xc_engine::CookieJar::default();
    let url = url::Url::parse(&format!("http://127.0.0.1:{port}/")).unwrap();
    jar.store(&url, &["avant=1; Path=/"]);
    let mut wanted = request(format!("ws://127.0.0.1:{port}/cookie"));
    wanted.network.cookies = Some(Cookies { jar: jar.clone(), send: true, store: true });

    connect_ws(wanted).await.unwrap();

    assert_eq!(header(&log.lock().unwrap()[0], "cookie").as_deref(), Some("avant=1"));
    assert_eq!(jar.header(&url).as_deref(), Some("avant=1; session=abc"));
}

#[tokio::test]
async fn ef_ws_01_a_keep_alive_interval_sends_pings() {
    let (port, log) = serve().await;
    let mut wanted = request(format!("ws://127.0.0.1:{port}/echo"));
    wanted.keep_alive = Some(Duration::from_millis(40));
    let _connection = connect_ws(wanted).await.unwrap();

    tokio::time::sleep(Duration::from_millis(400)).await;

    assert!(log.lock().unwrap()[0].pings >= 3, "{:?}", log.lock().unwrap()[0]);
}

#[tokio::test]
async fn ef_ws_01_a_server_that_never_answers_the_upgrade_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let _held = listener.accept().await;
        tokio::time::sleep(Duration::from_secs(5)).await;
    });
    let mut wanted = request(format!("ws://127.0.0.1:{port}/echo"));
    wanted.timeout = Duration::from_millis(200);

    let error = connect_ws(wanted).await.unwrap_err();

    assert!(matches!(error, EngineError::Timeout(200)), "{error}");
}

#[tokio::test]
async fn ef_ws_01_http_schemes_stand_for_ws_and_other_schemes_are_refused() {
    let (port, _) = serve().await;
    let connection = connect_ws(request(format!("http://127.0.0.1:{port}/echo"))).await.unwrap();
    assert!(connection.opened.url.starts_with("ws://"), "{}", connection.opened.url);

    let error = connect_ws(request("ftp://127.0.0.1/".into())).await.unwrap_err();
    assert!(matches!(error, EngineError::UnsupportedScheme(_)), "{error}");
    assert!(matches!(connect_ws(request("pas une adresse".into())).await.unwrap_err(), EngineError::InvalidUrl(_)));
}

#[tokio::test]
async fn ef_ws_01_a_message_over_the_limit_ends_the_connection_with_an_error() {
    let (port, _) = serve().await;
    let mut wanted = request(format!("ws://127.0.0.1:{port}/big"));
    wanted.max_message = Some(1 << 20);
    let mut connection = connect_ws(wanted).await.unwrap();

    let event = next(&mut connection).await;

    assert!(matches!(&event, WsEvent::Error { message } if message.contains("Message too long")), "{event:?}");
    assert!(connection.next().await.is_none());
}

/// Un proxy `CONNECT` minimal : relaie vers l'hôte demandé et note les demandes reçues.
async fn serve_proxy() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let log = seen.clone();
    tokio::spawn(async move {
        while let Ok((mut client, _)) = listener.accept().await {
            let log = log.clone();
            tokio::spawn(async move {
                let mut head = Vec::new();
                let mut byte = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if client.read_exact(&mut byte).await.is_err() {
                        return;
                    }
                    head.push(byte[0]);
                }
                let line = String::from_utf8_lossy(&head).lines().next().unwrap_or_default().to_owned();
                log.lock().unwrap().push(line.clone());
                let target = line.split(' ').nth(1).unwrap_or_default().to_owned();
                let Ok(mut upstream) = TcpStream::connect(&target).await else { return };
                let _ = client.write_all(b"HTTP/1.1 200 Connection established\r\n\r\n").await;
                let _ = tokio::io::copy_bidirectional(&mut client, &mut upstream).await;
            });
        }
    });
    (port, seen)
}

#[tokio::test]
async fn ef_ws_01_the_upgrade_goes_through_an_http_proxy_by_a_connect_tunnel() {
    let (port, _) = serve().await;
    let (proxy_port, proxied) = serve_proxy().await;
    let mut wanted = request(format!("ws://127.0.0.1:{port}/echo"));
    wanted.network.proxy = Some(Proxy {
        scheme: ProxyScheme::Http,
        host: "127.0.0.1".into(),
        port: proxy_port,
        auth: None,
        bypass: String::new(),
    });

    let mut connection = connect_ws(wanted).await.unwrap();
    connection.sender.text("par le proxy").unwrap();

    assert_eq!(next(&mut connection).await, WsEvent::Text { data: "par le proxy".into() });
    assert_eq!(proxied.lock().unwrap().as_slice(), [format!("CONNECT 127.0.0.1:{port} HTTP/1.1")]);
}

#[tokio::test]
async fn ef_ws_01_wss_verifies_the_certificate_and_trusts_a_custom_authority() {
    let ca = authority("Sira test CA");
    let identity = issue(&ca, &["localhost"]);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            CertificateDer::pem_slice_iter(identity.cert_pem.as_bytes()).collect::<Result<_, _>>().unwrap(),
            PrivateKeyDer::from_pem_slice(identity.key_pem.as_bytes()).unwrap(),
        )
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log: Log = Arc::default();
    let shared = log.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (acceptor, log) = (acceptor.clone(), shared.clone());
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(stream).await {
                    serve_socket(tls, log).await;
                }
            });
        }
    });

    let untrusted = connect_ws(request(format!("wss://localhost:{port}/echo"))).await.unwrap_err();
    assert!(matches!(untrusted, EngineError::Tls(_)), "{untrusted}");

    let mut trusted = request(format!("wss://localhost:{port}/echo"));
    trusted.network =
        Network { tls: Tls { extra_roots: vec![ca.cert_pem.into_bytes()], ..Tls::default() }, ..Network::default() };
    let mut connection = connect_ws(trusted).await.unwrap();
    connection.sender.text("chiffré").unwrap();
    assert_eq!(next(&mut connection).await, WsEvent::Text { data: "chiffré".into() });
    assert!(connection.opened.timings.tls_ms > 0.0);
}
