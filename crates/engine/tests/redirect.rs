use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use xc_engine::{send, HttpRequest, Network, RedirectStep, Redirects};

/// Un serveur qui répond une réponse par connexion, dans l'ordre, et rend les requêtes reçues.
async fn serve(responses: Vec<String>) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let mut seen = Vec::new();
        for response in responses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = vec![0; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            seen.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.shutdown().await.ok();
        }
        seen
    });
    (base, handle)
}

fn redirect(status: u16, location: &str) -> String {
    format!("HTTP/1.1 {status} Moved\r\nlocation: {location}\r\ncontent-length: 0\r\n\r\n")
}

const OK: &str = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok";

fn request(method: &str, url: String) -> HttpRequest {
    HttpRequest {
        method: method.into(),
        url,
        headers: vec![],
        body: None,
        timeout: Duration::from_secs(5),
        max_response_body: None,
        network: Network::default(),
    }
}

fn lines(raw: &str) -> Vec<String> {
    raw.lines().map(str::to_lowercase).collect()
}

#[tokio::test]
async fn ef_req_04_a_302_after_a_post_becomes_a_get_without_body_or_content_headers() {
    let (base, server) = serve(vec![redirect(302, "/done"), OK.into()]).await;
    let mut req = request("POST", format!("{base}/start"));
    req.headers = vec![("Content-Type".into(), "application/json".into()), ("X-Keep".into(), "1".into())];
    req.body = Some(br#"{"a":1}"#.to_vec());

    let res = send(req).await.unwrap();
    let seen = server.await.unwrap();

    assert_eq!((res.status, res.body.as_slice()), (200, b"ok".as_slice()));
    assert_eq!(res.url, format!("{base}/done"));
    assert_eq!(res.redirects, vec![RedirectStep { url: format!("{base}/start"), status: 302 }]);
    assert!(seen[0].starts_with("POST /start ") && seen[0].contains(r#"{"a":1}"#), "{}", seen[0]);
    assert!(seen[1].starts_with("GET /done "), "{}", seen[1]);
    let second = lines(&seen[1]);
    assert!(!second.iter().any(|l| l.starts_with("content-type") || l.starts_with("content-length")), "{second:?}");
    assert!(second.contains(&"x-keep: 1".to_owned()), "les autres en-têtes restent : {second:?}");
    assert!(!seen[1].contains(r#"{"a":1}"#));
}

#[tokio::test]
async fn ef_req_04_307_and_308_keep_the_method_and_the_body() {
    for status in [307, 308] {
        let (base, server) = serve(vec![redirect(status, "/again"), OK.into()]).await;
        let mut req = request("PUT", format!("{base}/start"));
        req.body = Some(b"payload".to_vec());
        let res = send(req).await.unwrap();
        let seen = server.await.unwrap();
        assert_eq!(res.status, 200, "{status}");
        assert!(seen[1].starts_with("PUT /again ") && seen[1].ends_with("payload"), "{status} : {}", seen[1]);
    }
}

#[tokio::test]
async fn ef_req_04_head_stays_head_on_a_301() {
    let (base, server) =
        serve(vec![redirect(301, "/there"), "HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n".into()]).await;
    let res = send(request("HEAD", format!("{base}/here"))).await.unwrap();
    assert_eq!(res.status, 200);
    assert!(server.await.unwrap()[1].starts_with("HEAD /there "));
}

#[tokio::test]
async fn ef_req_04_relative_and_absolute_locations_are_resolved() {
    let (base, server) = serve(vec![redirect(302, "../up?x=1"), redirect(302, "//127.0.0.1:1/never")]).await;
    let mut req = request("GET", format!("{base}/a/b/c"));
    req.network.redirects.max = 1;
    let res = send(req).await.unwrap();
    let seen = server.await.unwrap();
    assert!(seen[0].starts_with("GET /a/b/c "), "{}", seen[0]);
    assert!(seen[1].starts_with("GET /a/up?x=1 "), "{}", seen[1]);
    assert_eq!(res.status, 302, "la limite est atteinte : la 3xx est rendue telle quelle");
}

#[tokio::test]
async fn ef_req_04_the_redirect_past_the_limit_is_returned_as_the_response() {
    let (base, server) = serve(vec![redirect(302, "/1"), redirect(302, "/2"), redirect(302, "/3")]).await;
    let mut req = request("GET", format!("{base}/0"));
    req.network.redirects.max = 2;
    let res = send(req).await.unwrap();
    assert_eq!(server.await.unwrap().len(), 3, "2 redirections suivies, donc 3 requêtes");
    assert_eq!(res.status, 302);
    assert_eq!(res.url, format!("{base}/2"));
    assert_eq!(res.redirects.len(), 2);
    assert!(res.headers.iter().any(|(k, v)| k == "location" && v == "/3"));
}

#[tokio::test]
async fn ef_req_04_redirects_can_be_switched_off_and_a_missing_location_ends_them() {
    let (base, server) = serve(vec![redirect(302, "/next")]).await;
    let mut req = request("GET", format!("{base}/start"));
    req.network.redirects = Redirects::none();
    let res = send(req).await.unwrap();
    assert_eq!((res.status, res.redirects.len(), server.await.unwrap().len()), (302, 0, 1));

    let (base, _server) = serve(vec!["HTTP/1.1 302 Found\r\ncontent-length: 0\r\n\r\n".into()]).await;
    let res = send(request("GET", format!("{base}/start"))).await.unwrap();
    assert_eq!(res.status, 302, "sans Location, la 3xx est la réponse");
}

#[tokio::test]
async fn ef_req_04_only_http_locations_are_followed_and_other_3xx_codes_are_not() {
    let (base, _server) = serve(vec![redirect(302, "ftp://example.com/file")]).await;
    assert_eq!(send(request("GET", format!("{base}/a"))).await.unwrap().status, 302);
    for status in [300, 304, 305] {
        let (base, server) = serve(vec![redirect(status, "/b")]).await;
        let res = send(request("GET", format!("{base}/a"))).await.unwrap();
        assert_eq!((res.status, server.await.unwrap().len()), (status, 1));
    }
}

async fn across_origins(forward: bool) -> (Vec<String>, Vec<String>) {
    let (target, target_log) = serve(vec![OK.into()]).await;
    let (origin, origin_log) = serve(vec![redirect(302, &format!("{target}/landing"))]).await;
    let mut req = request("GET", format!("{origin}/start"));
    req.headers = vec![
        ("Authorization".into(), "Bearer s3cr3t".into()),
        ("Proxy-Authorization".into(), "Basic abc".into()),
        ("X-Amz-Date".into(), "20260101T000000Z".into()),
        ("X-Other".into(), "1".into()),
    ];
    req.network.redirects.forward_authorization = forward;
    assert_eq!(send(req).await.unwrap().status, 200);
    (lines(&origin_log.await.unwrap()[0]), lines(&target_log.await.unwrap()[0]))
}

#[tokio::test]
async fn ef_req_04_a_cross_origin_redirect_always_drops_aws_headers() {
    for forward in [true, false] {
        let (origin, target) = across_origins(forward).await;
        assert!(origin.contains(&"x-amz-date: 20260101t000000z".to_owned()));
        assert!(!target.iter().any(|l| l.starts_with("x-amz-")), "{forward} : {target:?}");
        assert!(target.contains(&"x-other: 1".to_owned()));
    }
}

#[tokio::test]
async fn ef_req_04_authorization_follows_a_cross_origin_redirect_unless_it_is_switched_off() {
    let (_, kept) = across_origins(true).await;
    assert!(kept.contains(&"authorization: bearer s3cr3t".to_owned()), "{kept:?}");
    let (_, dropped) = across_origins(false).await;
    assert!(
        !dropped.iter().any(|l| l.starts_with("authorization") || l.starts_with("proxy-authorization")),
        "{dropped:?}"
    );
}

#[tokio::test]
async fn ef_req_04_authorization_stays_on_the_same_origin_even_when_not_forwarded() {
    let (base, server) = serve(vec![redirect(302, "/inside"), OK.into()]).await;
    let mut req = request("GET", format!("{base}/start"));
    req.headers = vec![("Authorization".into(), "Bearer s3cr3t".into())];
    req.network.redirects.forward_authorization = false;
    send(req).await.unwrap();
    assert!(lines(&server.await.unwrap()[1]).contains(&"authorization: bearer s3cr3t".to_owned()));
}

#[tokio::test]
async fn ef_req_04_timings_add_up_over_the_hops() {
    let (base, _server) = serve(vec![redirect(302, "/b"), OK.into()]).await;
    let res = send(request("GET", format!("{base}/a"))).await.unwrap();
    let t = &res.timings;
    assert!(t.total_ms > 0.0 && t.total_ms >= t.ttfb_ms, "{t:?}");
}
