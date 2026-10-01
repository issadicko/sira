use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use xc_engine::{send, EngineError, HttpRequest};

async fn serve_once(response: &'static str) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0; 8192];
        let n = socket.read(&mut buf).await.unwrap();
        socket.write_all(response.as_bytes()).await.unwrap();
        socket.shutdown().await.ok();
        String::from_utf8_lossy(&buf[..n]).into_owned()
    });
    (format!("http://{addr}"), handle)
}

fn request(method: &str, url: String) -> HttpRequest {
    HttpRequest { method: method.into(), url, headers: vec![], body: None, timeout: Duration::from_secs(5) }
}

#[tokio::test]
async fn ef_res_01_status_headers_and_body() {
    let (base, server) =
        serve_once("HTTP/1.1 201 Created\r\ncontent-type: application/json\r\ncontent-length: 11\r\n\r\n{\"ok\":true}")
            .await;
    let mut req = request("POST", format!("{base}/transactions?expand=client"));
    req.headers.push(("X-Canal".into(), "USSD".into()));
    req.body = Some(br#"{"montant":15000}"#.to_vec());

    let res = send(&req).await.unwrap();
    let raw = server.await.unwrap();

    assert_eq!(res.status, 201);
    assert_eq!(res.reason, "Created");
    assert_eq!(res.body, br#"{"ok":true}"#);
    assert!(res.headers.iter().any(|(k, v)| k == "content-type" && v == "application/json"));
    assert!(raw.starts_with("POST /transactions?expand=client HTTP/1.1"));
    assert!(raw.to_lowercase().contains("x-canal: ussd"));
    assert!(raw.contains("content-length: 17"));
    assert!(raw.ends_with(r#"{"montant":15000}"#));
}

#[tokio::test]
async fn ef_res_02_timings_are_measured() {
    let (base, _server) = serve_once("HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok").await;
    let res = send(&request("GET", base)).await.unwrap();
    let t = res.timings;
    assert_eq!(t.dns_ms, 0.0, "une IP littérale ne passe pas par le DNS");
    assert_eq!(t.tls_ms, 0.0);
    assert!(t.tcp_ms > 0.0 && t.ttfb_ms > 0.0);
    assert!(t.total_ms >= t.tcp_ms + t.ttfb_ms + t.download_ms);
}

#[tokio::test]
async fn ef_req_03_request_can_be_cancelled() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _hold = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let task = tokio::spawn(async move { send(&request("GET", format!("http://{addr}"))).await });
    tokio::time::sleep(Duration::from_millis(50)).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn timeout_is_reported() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let _hold = tokio::spawn(async move {
        let (_socket, _) = listener.accept().await.unwrap();
        tokio::time::sleep(Duration::from_secs(30)).await;
    });
    let mut req = request("GET", format!("http://{addr}"));
    req.timeout = Duration::from_millis(100);
    assert!(matches!(send(&req).await, Err(EngineError::Timeout(100))));
}

#[tokio::test]
async fn connection_refused_is_reported() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let err = send(&request("GET", format!("http://{addr}"))).await.unwrap_err();
    assert!(matches!(err, EngineError::Connect { .. }));
}

#[tokio::test]
async fn unsupported_scheme_is_rejected() {
    let err = send(&request("GET", "ftp://example.test".into())).await.unwrap_err();
    assert!(matches!(err, EngineError::UnsupportedScheme(_)));
}
