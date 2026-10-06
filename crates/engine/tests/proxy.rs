mod common;

use common::{authority, issue, request, serve_tls, OK};
use tokio::io::{copy_bidirectional, AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use xc_engine::{send, EngineError, Network, Proxy, ProxyScheme, Tls};

fn proxy(scheme: ProxyScheme, port: u16) -> Proxy {
    Proxy { scheme, host: "127.0.0.1".into(), port, auth: None, bypass: String::new() }
}

fn through(proxy: Proxy) -> Network {
    Network { proxy: Some(proxy), ..Network::default() }
}

async fn read_head(socket: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        socket.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }
    String::from_utf8(head).unwrap()
}

/// Un « proxy » HTTP qui répond une réponse fixe et rend ce qu'il a reçu.
async fn forward_proxy(answer: &'static str) -> (u16, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let head = read_head(&mut socket).await;
        socket.write_all(answer.as_bytes()).await.unwrap();
        head
    });
    (port, handle)
}

/// Un proxy `CONNECT` : vérifie la demande, ouvre la connexion vers la cible et relaie les octets.
async fn connect_proxy(status_line: &'static str) -> (u16, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let head = read_head(&mut socket).await;
        let target = head.split_whitespace().nth(1).unwrap().to_owned();
        if !status_line.contains(" 200 ") {
            socket.write_all(format!("{status_line}\r\ncontent-length: 0\r\n\r\n").as_bytes()).await.unwrap();
            return head;
        }
        let mut upstream = TcpStream::connect(target.replace("localhost", "127.0.0.1")).await.unwrap();
        socket.write_all(format!("{status_line}\r\n\r\n").as_bytes()).await.unwrap();
        copy_bidirectional(&mut socket, &mut upstream).await.ok();
        head
    });
    (port, handle)
}

async fn http_origin() -> (u16, JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let head = read_head(&mut socket).await;
        socket.write_all(OK.as_bytes()).await.unwrap();
        head
    });
    (port, handle)
}

#[tokio::test]
async fn ef_req_04_an_http_request_goes_to_the_proxy_in_absolute_form() {
    let (port, proxy_seen) = forward_proxy(OK).await;

    let res =
        send(request("http://cible.test:8080/a/b?x=1".into(), through(proxy(ProxyScheme::Http, port)))).await.unwrap();

    assert_eq!((res.status, res.body.as_slice()), (200, b"ok".as_slice()));
    let head = proxy_seen.await.unwrap().to_lowercase();
    assert!(head.starts_with("get http://cible.test:8080/a/b?x=1 http/1.1"), "{head}");
    assert!(head.contains("host: cible.test:8080"), "{head}");
}

#[tokio::test]
async fn ef_req_04_proxy_credentials_are_sent_as_basic_authorization() {
    let (port, proxy_seen) = forward_proxy(OK).await;
    let mut p = proxy(ProxyScheme::Http, port);
    p.auth = Some(("alice".into(), "s3cret".into()));

    send(request("http://cible.test/".into(), through(p))).await.unwrap();

    let head = proxy_seen.await.unwrap().to_lowercase();
    assert!(head.contains("proxy-authorization: basic ywxpy2u6cznjcmv0"), "{head}");
}

#[tokio::test]
async fn ef_req_04_a_bypassed_host_is_reached_directly_in_origin_form() {
    let (origin, origin_seen) = http_origin().await;
    let (proxy_port, proxy_seen) = forward_proxy(OK).await;
    let mut p = proxy(ProxyScheme::Http, proxy_port);
    p.bypass = "localhost".into();

    let res = send(request(format!("http://localhost:{origin}/direct"), through(p))).await.unwrap();

    assert_eq!(res.status, 200);
    assert!(origin_seen.await.unwrap().starts_with("GET /direct "));
    assert!(!proxy_seen.is_finished(), "le proxy n'a rien reçu");
    proxy_seen.abort();
}

#[tokio::test]
async fn ef_req_04_an_https_request_opens_a_connect_tunnel_then_verifies_the_server_itself() {
    let ca = authority("Sira test CA");
    let (origin, origin_seen) = serve_tls(&issue(&ca, &["localhost"]), None, 1).await;
    let (port, proxy_seen) = connect_proxy("HTTP/1.1 200 Connection Established").await;
    let network = Network {
        proxy: Some(Proxy { auth: Some(("alice".into(), "s3cret".into())), ..proxy(ProxyScheme::Http, port) }),
        tls: Tls { extra_roots: vec![ca.cert_pem.clone().into_bytes()], ..Tls::default() },
        ..Network::default()
    };

    let res = send(request(format!("https://localhost:{origin}/secure"), network)).await.unwrap();

    assert_eq!(res.status, 200);
    let head = proxy_seen.await.unwrap().to_lowercase();
    assert!(head.starts_with(&format!("connect localhost:{origin} http/1.1")), "{head}");
    assert!(head.contains("proxy-authorization: basic ywxpy2u6cznjcmv0"), "{head}");
    let seen = origin_seen.await.unwrap();
    assert!(seen[0].request.starts_with("GET /secure "), "{}", seen[0].request);
}

#[tokio::test]
async fn ef_req_04_a_proxy_that_refuses_the_tunnel_is_reported_with_its_answer() {
    let (port, _proxy_seen) = connect_proxy("HTTP/1.1 407 Proxy Authentication Required").await;

    let error =
        send(request("https://localhost:9/".into(), through(proxy(ProxyScheme::Http, port)))).await.unwrap_err();

    assert!(matches!(error, EngineError::Connect { .. }), "{error}");
    assert!(error.to_string().contains("407"), "{error}");
}

#[tokio::test]
async fn ef_req_04_an_unreachable_proxy_is_a_connection_error_naming_the_proxy() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let error = send(request("http://cible.test/".into(), through(proxy(ProxyScheme::Http, port)))).await.unwrap_err();

    assert!(matches!(&error, EngineError::Connect { addr, .. } if addr.contains(&port.to_string())), "{error}");
}

#[tokio::test]
async fn ef_req_04_an_https_proxy_is_reached_over_tls() {
    let ca = authority("Sira test CA");
    let (port, proxy_seen) = serve_tls(&issue(&ca, &["127.0.0.1"]), None, 1).await;
    let network = Network {
        proxy: Some(proxy(ProxyScheme::Https, port)),
        tls: Tls { extra_roots: vec![ca.cert_pem.clone().into_bytes()], ..Tls::default() },
        ..Network::default()
    };

    let res = send(request("http://cible.test/via-tls".into(), network)).await.unwrap();

    assert_eq!(res.status, 200);
    let seen = proxy_seen.await.unwrap();
    assert!(seen[0].request.to_lowercase().starts_with("get http://cible.test/via-tls "), "{}", seen[0].request);
}

/// Un serveur SOCKS5 : annonce la méthode choisie, lit la demande, relaie vers `origin` et rend le nom demandé.
async fn socks5_proxy(origin: u16, password_auth: bool) -> (u16, JoinHandle<(String, Option<(String, String)>)>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut greeting = [0u8; 2];
        socket.read_exact(&mut greeting).await.unwrap();
        let mut methods = vec![0u8; usize::from(greeting[1])];
        socket.read_exact(&mut methods).await.unwrap();
        let mut credentials = None;
        if password_auth {
            assert!(methods.contains(&2), "{methods:?}");
            socket.write_all(&[5, 2]).await.unwrap();
            let mut head = [0u8; 2];
            socket.read_exact(&mut head).await.unwrap();
            let mut user = vec![0u8; usize::from(head[1])];
            socket.read_exact(&mut user).await.unwrap();
            let mut len = [0u8; 1];
            socket.read_exact(&mut len).await.unwrap();
            let mut password = vec![0u8; usize::from(len[0])];
            socket.read_exact(&mut password).await.unwrap();
            credentials = Some((String::from_utf8(user).unwrap(), String::from_utf8(password).unwrap()));
            socket.write_all(&[1, 0]).await.unwrap();
        } else {
            socket.write_all(&[5, 0]).await.unwrap();
        }
        let mut head = [0u8; 4];
        socket.read_exact(&mut head).await.unwrap();
        assert_eq!(head[3], 3, "le nom est confié au proxy");
        let mut len = [0u8; 1];
        socket.read_exact(&mut len).await.unwrap();
        let mut name = vec![0u8; usize::from(len[0])];
        socket.read_exact(&mut name).await.unwrap();
        let mut port = [0u8; 2];
        socket.read_exact(&mut port).await.unwrap();
        let mut upstream = TcpStream::connect(("127.0.0.1", origin)).await.unwrap();
        socket.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
        copy_bidirectional(&mut socket, &mut upstream).await.ok();
        (format!("{}:{}", String::from_utf8(name).unwrap(), u16::from_be_bytes(port)), credentials)
    });
    (port, handle)
}

#[tokio::test]
async fn ef_req_04_socks5_hands_the_hostname_to_the_proxy_and_relays_the_exchange() {
    let (origin, origin_seen) = http_origin().await;
    let (port, proxy_seen) = socks5_proxy(origin, false).await;

    let res =
        send(request("http://cible.test:8081/relay".into(), through(proxy(ProxyScheme::Socks5, port)))).await.unwrap();

    assert_eq!((res.status, res.body.as_slice()), (200, b"ok".as_slice()));
    let head = origin_seen.await.unwrap();
    assert!(head.starts_with("GET /relay "), "origine : pas de forme absolue derrière SOCKS : {head}");
    assert_eq!(proxy_seen.await.unwrap().0, "cible.test:8081");
}

#[tokio::test]
async fn ef_req_04_socks5_authenticates_with_the_proxy_credentials() {
    let (origin, _origin_seen) = http_origin().await;
    let (port, proxy_seen) = socks5_proxy(origin, true).await;
    let mut p = proxy(ProxyScheme::Socks5, port);
    p.auth = Some(("alice".into(), "s3cret".into()));

    let res = send(request("http://cible.test/".into(), through(p))).await.unwrap();

    assert_eq!(res.status, 200);
    assert_eq!(proxy_seen.await.unwrap().1, Some(("alice".into(), "s3cret".into())));
}

#[tokio::test]
async fn ef_req_04_socks5_reports_a_refused_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0u8; 3];
        socket.read_exact(&mut buf).await.unwrap();
        socket.write_all(&[5, 0]).await.unwrap();
        let mut request = [0u8; 4 + 1 + "cible.test".len() + 2];
        socket.read_exact(&mut request).await.unwrap();
        socket.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
    });

    let error =
        send(request("http://cible.test/".into(), through(proxy(ProxyScheme::Socks5, port)))).await.unwrap_err();

    assert!(error.to_string().contains("connexion refusée par l'hôte"), "{error}");
}

#[tokio::test]
async fn ef_req_04_socks4a_sends_the_hostname_after_the_user_id() {
    let (origin, _origin_seen) = http_origin().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let proxy_seen = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut head = [0u8; 8];
        socket.read_exact(&mut head).await.unwrap();
        let mut rest = Vec::new();
        let mut byte = [0u8; 1];
        let mut zeros = 0;
        while zeros < 2 {
            socket.read_exact(&mut byte).await.unwrap();
            rest.push(byte[0]);
            zeros += usize::from(byte[0] == 0);
        }
        let mut upstream = TcpStream::connect(("127.0.0.1", origin)).await.unwrap();
        socket.write_all(&[0, 0x5A, 0, 0, 0, 0, 0, 0]).await.unwrap();
        copy_bidirectional(&mut socket, &mut upstream).await.ok();
        (head, rest)
    });
    let mut p = proxy(ProxyScheme::Socks4, port);
    p.auth = Some(("alice".into(), String::new()));

    let res = send(request("http://cible.test:9000/".into(), through(p))).await.unwrap();

    assert_eq!(res.status, 200);
    let (head, rest) = proxy_seen.await.unwrap();
    assert_eq!(head, [4, 1, 0x23, 0x28, 0, 0, 0, 1]);
    assert_eq!(rest, b"alice\0cible.test\0");
}
