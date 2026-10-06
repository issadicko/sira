use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use xc_engine::{send, CookieJar, Cookies, HttpRequest, Network};

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

fn reply(status: &str, extra: &[&str]) -> String {
    let headers: String = extra.iter().map(|h| format!("{h}\r\n")).collect();
    format!("HTTP/1.1 {status}\r\n{headers}content-length: 0\r\nconnection: close\r\n\r\n")
}

fn request(url: String, jar: &CookieJar, send_cookies: bool, store_cookies: bool) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![],
        body: None,
        timeout: Duration::from_secs(5),
        max_response_body: None,
        network: Network {
            cookies: Some(Cookies { jar: jar.clone(), send: send_cookies, store: store_cookies }),
            ..Network::default()
        },
    }
}

fn cookie_line(raw: &str) -> Option<String> {
    raw.lines().find(|l| l.to_ascii_lowercase().starts_with("cookie:")).map(|l| l.to_ascii_lowercase())
}

#[tokio::test]
async fn ef_ux_02_a_cookie_set_by_a_response_comes_back_on_the_next_request() {
    let jar = CookieJar::default();
    let (base, seen) = serve(vec![reply("200 OK", &["set-cookie: sid=abc123; Path=/"]), reply("200 OK", &[])]).await;

    send(request(format!("{base}/login"), &jar, true, true)).await.unwrap();
    send(request(format!("{base}/profile"), &jar, true, true)).await.unwrap();

    let seen = seen.await.unwrap();
    assert_eq!(cookie_line(&seen[0]), None, "rien à envoyer au premier envoi");
    assert_eq!(cookie_line(&seen[1]).as_deref(), Some("cookie: sid=abc123"));
}

#[tokio::test]
async fn ef_ux_02_cookies_set_by_a_redirect_response_are_kept_and_sent_on_the_next_hop() {
    let jar = CookieJar::default();
    let (base, seen) =
        serve(vec![reply("302 Found", &["location: /next", "set-cookie: step=1; Path=/"]), reply("200 OK", &[])]).await;

    send(request(format!("{base}/start"), &jar, true, true)).await.unwrap();

    let seen = seen.await.unwrap();
    assert_eq!(cookie_line(&seen[1]).as_deref(), Some("cookie: step=1"));
    assert_eq!(jar.list().len(), 1);
}

#[tokio::test]
async fn ef_ux_02_a_cookie_written_by_hand_is_merged_with_the_jar_which_wins_on_a_name_clash() {
    let jar = CookieJar::default();
    let (base, seen) = serve(vec![reply("200 OK", &["set-cookie: a=jar; Path=/"]), reply("200 OK", &[])]).await;
    send(request(format!("{base}/"), &jar, true, true)).await.unwrap();

    let mut second = request(format!("{base}/"), &jar, true, true);
    second.headers = vec![("Cookie".into(), "b=hand; a=hand".into())];
    send(second).await.unwrap();

    assert_eq!(cookie_line(&seen.await.unwrap()[1]).as_deref(), Some("cookie: b=hand; a=jar"));
}

#[tokio::test]
async fn ef_ux_02_a_cookie_written_by_hand_does_not_follow_a_redirect_to_another_origin() {
    let jar = CookieJar::default();
    let (other, other_seen) = serve(vec![reply("200 OK", &[])]).await;
    let (base, seen) = serve(vec![reply("302 Found", &[&format!("location: {other}/landing")])]).await;

    let mut start = request(format!("{base}/start"), &jar, true, true);
    start.headers = vec![("Cookie".into(), "session=secret".into())];
    send(start).await.unwrap();

    assert_eq!(cookie_line(&seen.await.unwrap()[0]).as_deref(), Some("cookie: session=secret"));
    assert_eq!(cookie_line(&other_seen.await.unwrap()[0]), None);
}

#[tokio::test]
async fn ef_ux_02_with_sending_off_the_jar_is_not_read_and_with_storing_off_it_is_not_written() {
    let jar = CookieJar::default();
    let (base, seen) = serve(vec![
        reply("200 OK", &["set-cookie: kept=1; Path=/"]),
        reply("200 OK", &["set-cookie: ignored=1; Path=/"]),
        reply("200 OK", &[]),
    ])
    .await;

    send(request(format!("{base}/a"), &jar, true, true)).await.unwrap();
    send(request(format!("{base}/b"), &jar, false, true)).await.unwrap();
    send(request(format!("{base}/c"), &jar, true, false)).await.unwrap();

    let seen = seen.await.unwrap();
    assert_eq!(cookie_line(&seen[1]), None, "envoi coupé : le cookie reste dans le pot");
    assert_eq!(cookie_line(&seen[2]).as_deref(), Some("cookie: kept=1; ignored=1"), "le plus ancien d'abord");
    let again = jar.list().iter().map(|c| c.key.clone()).collect::<Vec<_>>();
    assert_eq!(again, vec!["ignored", "kept"], "le pot garde ce que le deuxième envoi a reçu, puis rien du troisième");
}

#[tokio::test]
async fn ef_ux_02_without_a_jar_nothing_is_sent_or_kept() {
    let (base, seen) = serve(vec![reply("200 OK", &["set-cookie: a=1; Path=/"])]).await;
    let mut plain = request(format!("{base}/"), &CookieJar::default(), true, true);
    plain.network.cookies = None;

    send(plain).await.unwrap();

    assert_eq!(cookie_line(&seen.await.unwrap()[0]), None);
}

#[tokio::test]
async fn ef_ux_02_secure_cookies_are_sent_over_http_only_to_a_local_address() {
    let jar = CookieJar::default();
    let (base, seen) = serve(vec![reply("200 OK", &["set-cookie: s=1; Path=/; Secure"]), reply("200 OK", &[])]).await;

    send(request(format!("{base}/"), &jar, true, true)).await.unwrap();
    send(request(format!("{base}/"), &jar, true, true)).await.unwrap();

    assert_eq!(
        cookie_line(&seen.await.unwrap()[1]).as_deref(),
        Some("cookie: s=1"),
        "127.0.0.1 est une origine de confiance"
    );
}
