#![allow(clippy::result_large_err)]

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use common::{authority, issue};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpListener;
use xc_engine::{connect_grpc, EngineError, GrpcCall, GrpcEvent, GrpcRequest, Network, Tls};

#[derive(Debug, Default, Clone)]
struct Seen {
    path: String,
    headers: Vec<(String, String)>,
}

type Log = Arc<Mutex<Vec<Seen>>>;

fn header(seen: &Seen, name: &str) -> Option<String> {
    seen.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone())
}

fn frame(message: &[u8]) -> Bytes {
    let mut out = BytesMut::new();
    out.extend_from_slice(&[0]);
    out.extend_from_slice(&(message.len() as u32).to_be_bytes());
    out.extend_from_slice(message);
    out.freeze()
}

fn trailers(code: u32, message: &str) -> http::HeaderMap {
    let mut map = http::HeaderMap::new();
    map.insert("grpc-status", code.to_string().parse().unwrap());
    if !message.is_empty() {
        map.insert("grpc-message", message.parse().unwrap());
    }
    map
}

async fn read_frames(body: &mut h2::RecvStream, until_end: bool) -> Vec<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut messages = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.unwrap();
        body.flow_control().release_capacity(chunk.len()).ok();
        buffer.extend_from_slice(&chunk);
        while buffer.len() >= 5 {
            let length = u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]) as usize;
            if buffer.len() < 5 + length {
                break;
            }
            messages.push(buffer[5..5 + length].to_vec());
            buffer.drain(..5 + length);
            if !until_end {
                return messages;
            }
        }
    }
    messages
}

/// Sert gRPC sur `stream` selon la méthode : `/t.S/Echo` (unaire), `/t.S/Stream` (trois messages), `/t.S/Collect`
/// (compte les messages reçus), `/t.S/Bidi` (renvoie chaque message), `/t.S/Fail` (statut 5 sans corps), `/t.S/Meta`
/// (renvoie les en-têtes reçus), `/t.S/Slow` (ne répond jamais), `/t.S/Plain` (HTTP 200 qui n'est pas gRPC).
async fn serve<S>(stream: S, log: Log)
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let Ok(mut connection) = h2::server::handshake(stream).await else { return };
    while let Some(Ok((request, mut respond))) = connection.accept().await {
        let log = log.clone();
        tokio::spawn(async move {
            let (parts, mut body) = request.into_parts();
            let seen = Seen {
                path: parts.uri.path().to_owned(),
                headers: parts
                    .headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
                    .collect(),
            };
            log.lock().unwrap().push(seen.clone());
            let ok =
                || http::Response::builder().status(200).header("content-type", "application/grpc").body(()).unwrap();
            match parts.uri.path() {
                "/t.S/Echo" => {
                    let messages = read_frames(&mut body, false).await;
                    let mut send = respond.send_response(ok(), false).unwrap();
                    send.send_data(frame(&messages[0]), false).unwrap();
                    send.send_trailers(trailers(0, "")).unwrap();
                }
                "/t.S/Stream" => {
                    read_frames(&mut body, false).await;
                    let mut send = respond.send_response(ok(), false).unwrap();
                    for n in 1..=3 {
                        send.send_data(frame(format!("m{n}").as_bytes()), false).unwrap();
                    }
                    send.send_trailers(trailers(0, "")).unwrap();
                }
                "/t.S/Collect" => {
                    let messages = read_frames(&mut body, true).await;
                    let mut send = respond.send_response(ok(), false).unwrap();
                    send.send_data(frame(format!("{} reçus", messages.len()).as_bytes()), false).unwrap();
                    send.send_trailers(trailers(0, "")).unwrap();
                }
                "/t.S/Bidi" => {
                    let mut send = respond.send_response(ok(), false).unwrap();
                    loop {
                        let messages = read_frames(&mut body, false).await;
                        let Some(message) = messages.first() else { break };
                        send.send_data(frame(&[b"re:".as_slice(), message].concat()), false).unwrap();
                    }
                    send.send_trailers(trailers(0, "")).unwrap();
                }
                "/t.S/Fail" => {
                    let response = http::Response::builder()
                        .status(200)
                        .header("content-type", "application/grpc")
                        .header("grpc-status", "5")
                        .header("grpc-message", "utilisateur%20introuvable")
                        .body(())
                        .unwrap();
                    respond.send_response(response, true).unwrap();
                }
                "/t.S/Meta" => {
                    read_frames(&mut body, false).await;
                    let text = seen.headers.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("\n");
                    let mut send = respond.send_response(ok(), false).unwrap();
                    send.send_data(frame(text.as_bytes()), false).unwrap();
                    send.send_trailers(trailers(0, "")).unwrap();
                }
                "/t.S/Slow" => {
                    read_frames(&mut body, false).await;
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
                _ => {
                    let response =
                        http::Response::builder().status(200).header("content-type", "text/plain").body(()).unwrap();
                    let mut send = respond.send_response(response, false).unwrap();
                    send.send_data(Bytes::from_static(b"bonjour"), true).unwrap();
                }
            }
        });
    }
}

async fn server() -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let log: Log = Arc::default();
    let shared = log.clone();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve(stream, shared.clone()));
        }
    });
    (format!("grpc://127.0.0.1:{port}"), log)
}

fn request(url: &str, method: &str) -> GrpcRequest {
    GrpcRequest {
        url: url.to_owned(),
        method: method.to_owned(),
        metadata: vec![],
        timeout: Duration::from_secs(5),
        deadline: None,
        network: Network::default(),
    }
}

async fn collect(call: &mut GrpcCall) -> Vec<GrpcEvent> {
    let mut events = Vec::new();
    while let Some(event) =
        tokio::time::timeout(Duration::from_secs(5), call.events.recv()).await.expect("événement attendu")
    {
        let last = matches!(event, GrpcEvent::Status { .. } | GrpcEvent::Error { .. });
        events.push(event);
        if last {
            break;
        }
    }
    events
}

fn messages(events: &[GrpcEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            GrpcEvent::Message { data } => Some(String::from_utf8_lossy(data).into_owned()),
            _ => None,
        })
        .collect()
}

fn status(events: &[GrpcEvent]) -> (u32, String) {
    match events.last() {
        Some(GrpcEvent::Status { code, message, .. }) => (*code, message.clone()),
        other => panic!("statut attendu, reçu {other:?}"),
    }
}

#[tokio::test]
async fn ef_grpc_01_a_unary_call_sends_one_message_and_gets_one_message_and_ok() {
    let (url, log) = server().await;
    let mut call = connect_grpc(request(&url, "t.S/Echo")).await.unwrap();
    call.sender.send(b"salut".to_vec()).unwrap();
    call.sender.finish().unwrap();
    let events = collect(&mut call).await;
    assert_eq!(messages(&events), ["salut"]);
    assert_eq!(status(&events), (0, String::new()));
    assert!(matches!(events[0], GrpcEvent::Headers { .. }));
    let seen = log.lock().unwrap()[0].clone();
    assert_eq!(seen.path, "/t.S/Echo");
    assert_eq!(header(&seen, "content-type").as_deref(), Some("application/grpc"));
    assert_eq!(header(&seen, "te").as_deref(), Some("trailers"));
}

#[tokio::test]
async fn ef_grpc_01_a_server_streaming_call_delivers_every_message_in_order() {
    let (url, _) = server().await;
    let mut call = connect_grpc(request(&url, "/t.S/Stream")).await.unwrap();
    call.sender.send(b"go".to_vec()).unwrap();
    call.sender.finish().unwrap();
    let events = collect(&mut call).await;
    assert_eq!(messages(&events), ["m1", "m2", "m3"]);
    assert_eq!(status(&events).0, 0);
}

#[tokio::test]
async fn ef_grpc_01_a_client_streaming_call_sends_several_messages_then_half_closes() {
    let (url, _) = server().await;
    let mut call = connect_grpc(request(&url, "t.S/Collect")).await.unwrap();
    for n in 0..4 {
        call.sender.send(format!("{n}").into_bytes()).unwrap();
    }
    call.sender.finish().unwrap();
    let events = collect(&mut call).await;
    assert_eq!(messages(&events), ["4 reçus"]);
}

#[tokio::test]
async fn ef_grpc_01_a_bidirectional_call_answers_each_message_while_the_call_stays_open() {
    let (url, _) = server().await;
    let mut call = connect_grpc(request(&url, "t.S/Bidi")).await.unwrap();
    call.sender.send(b"a".to_vec()).unwrap();
    let first = loop {
        match call.events.recv().await.unwrap() {
            GrpcEvent::Message { data } => break data,
            _ => continue,
        }
    };
    assert_eq!(first, b"re:a");
    call.sender.send(b"b".to_vec()).unwrap();
    call.sender.finish().unwrap();
    let events = collect(&mut call).await;
    assert_eq!(messages(&events), ["re:b"]);
    assert_eq!(status(&events).0, 0);
}

#[tokio::test]
async fn ef_grpc_01_an_error_status_without_body_is_reported_with_its_decoded_message() {
    let (url, _) = server().await;
    let mut call = connect_grpc(request(&url, "t.S/Fail")).await.unwrap();
    call.sender.send(b"x".to_vec()).unwrap();
    call.sender.finish().unwrap();
    let events = collect(&mut call).await;
    assert!(messages(&events).is_empty());
    assert_eq!(status(&events), (5, "utilisateur introuvable".into()));
}

#[tokio::test]
async fn ef_grpc_01_metadata_goes_out_as_lowercase_headers_and_reserved_ones_are_not_overridden() {
    let (url, _) = server().await;
    let mut req = request(&url, "t.S/Meta");
    req.metadata = vec![
        ("X-Demo".into(), "un".into()),
        ("authorization".into(), "Bearer t".into()),
        ("content-type".into(), "text/plain".into()),
    ];
    let mut call = connect_grpc(req).await.unwrap();
    call.sender.send(b"x".to_vec()).unwrap();
    call.sender.finish().unwrap();
    let events = collect(&mut call).await;
    let text = messages(&events).remove(0);
    assert!(text.contains("x-demo=un"), "{text}");
    assert!(text.contains("authorization=Bearer t"), "{text}");
    assert!(text.contains("content-type=application/grpc"), "{text}");
    assert!(!text.contains("text/plain"), "{text}");
}

#[tokio::test]
async fn ef_grpc_01_cancelling_a_call_ends_it_with_status_cancelled() {
    let (url, _) = server().await;
    let mut call = connect_grpc(request(&url, "t.S/Slow")).await.unwrap();
    call.sender.send(b"x".to_vec()).unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    call.sender.cancel();
    let events = collect(&mut call).await;
    assert_eq!(status(&events).0, 1);
}

#[tokio::test]
async fn ef_grpc_01_a_deadline_is_announced_and_ends_the_call_with_deadline_exceeded() {
    let (url, log) = server().await;
    let mut req = request(&url, "t.S/Slow");
    req.deadline = Some(Duration::from_millis(300));
    let mut call = connect_grpc(req).await.unwrap();
    call.sender.send(b"x".to_vec()).unwrap();
    let events = collect(&mut call).await;
    assert_eq!(status(&events).0, 4);
    assert_eq!(header(&log.lock().unwrap()[0], "grpc-timeout").as_deref(), Some("300m"));
}

#[tokio::test]
async fn ef_grpc_01_a_server_that_does_not_speak_grpc_is_said_so() {
    let (url, _) = server().await;
    let mut call = connect_grpc(request(&url, "t.S/Plain")).await.unwrap();
    call.sender.send(b"x".to_vec()).unwrap();
    let events = collect(&mut call).await;
    match events.last() {
        Some(GrpcEvent::Error { message }) => assert!(message.contains("text/plain"), "{message}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn ef_grpc_01_a_secure_call_negotiates_h2_and_trusts_the_given_authority() {
    let ca = authority("Autorité de test");
    let identity = issue(&ca, &["localhost"]);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            CertificateDer::pem_slice_iter(identity.cert_pem.as_bytes()).collect::<Result<_, _>>().unwrap(),
            PrivateKeyDer::from_pem_slice(identity.key_pem.as_bytes()).unwrap(),
        )
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
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
                    serve(tls, log).await;
                }
            });
        }
    });
    let mut req = request(&format!("grpcs://localhost:{port}"), "t.S/Echo");
    req.network = Network {
        tls: Tls { extra_roots: vec![ca.cert_pem.clone().into_bytes()], keep_default_roots: false, ..Tls::default() },
        ..Network::default()
    };
    let mut call = connect_grpc(req).await.unwrap();
    call.sender.send("chiffré".as_bytes().to_vec()).unwrap();
    call.sender.finish().unwrap();
    let events = collect(&mut call).await;
    assert_eq!(messages(&events), ["chiffré"]);
    assert_eq!(log.lock().unwrap()[0].path, "/t.S/Echo");
}

#[tokio::test]
async fn ef_grpc_01_a_secure_call_refuses_an_unknown_authority() {
    let ca = authority("Autre");
    let identity = issue(&ca, &["localhost"]);
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            CertificateDer::pem_slice_iter(identity.cert_pem.as_bytes()).collect::<Result<_, _>>().unwrap(),
            PrivateKeyDer::from_pem_slice(identity.key_pem.as_bytes()).unwrap(),
        )
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                if let Ok(tls) = acceptor.accept(stream).await {
                    serve(tls, Arc::default()).await;
                }
            });
        }
    });
    let error = connect_grpc(request(&format!("grpcs://localhost:{port}"), "t.S/Echo")).await.unwrap_err();
    assert!(matches!(error, EngineError::Tls(_)), "{error:?}");
}

#[tokio::test]
async fn ef_grpc_01_a_bad_method_a_bad_scheme_and_a_closed_port_are_errors() {
    let (url, _) = server().await;
    assert!(matches!(connect_grpc(request(&url, "Echo")).await.unwrap_err(), EngineError::Grpc(_)));
    assert!(matches!(
        connect_grpc(request("ftp://x", "t.S/Echo")).await.unwrap_err(),
        EngineError::UnsupportedScheme(_)
    ));
    let port = {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    };
    assert!(matches!(
        connect_grpc(request(&format!("127.0.0.1:{port}"), "t.S/Echo")).await.unwrap_err(),
        EngineError::Connect { .. }
    ));
}
