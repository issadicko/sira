use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use xc_engine::Network;
use xc_vault::{references, VaultAuth, VaultClient, VaultConfig, VaultError, VaultRef};

#[derive(Debug, Clone)]
struct Seen {
    method: String,
    path: String,
    token: Option<String>,
    namespace: Option<String>,
    body: String,
}

type Log = Arc<Mutex<Vec<Seen>>>;
type Route = fn(&Seen) -> (u16, String);

async fn serve(route: Route) -> (String, Log) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = format!("http://{}", listener.local_addr().unwrap());
    let log: Log = Arc::default();
    let shared = log.clone();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let log = shared.clone();
            tokio::spawn(async move {
                let mut data = Vec::new();
                let mut buf = [0u8; 4096];
                let (head, body) = loop {
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    data.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&data).into_owned();
                    if let Some(at) = text.find("\r\n\r\n") {
                        let head = text[..at].to_owned();
                        let length = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if text.len() >= at + 4 + length {
                            break (head, text[at + 4..at + 4 + length].to_owned());
                        }
                    }
                };
                let mut lines = head.lines();
                let mut first = lines.next().unwrap_or_default().split(' ');
                let (method, path) =
                    (first.next().unwrap_or_default().to_owned(), first.next().unwrap_or_default().to_owned());
                let header = |name: &str| {
                    head.lines().find_map(|l| {
                        l.to_ascii_lowercase()
                            .starts_with(&format!("{name}:"))
                            .then(|| l[name.len() + 1..].trim().to_owned())
                    })
                };
                let seen =
                    Seen { method, path, token: header("x-vault-token"), namespace: header("x-vault-namespace"), body };
                let (status, reply) = route(&seen);
                log.lock().unwrap().push(seen);
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                stream.write_all(response.as_bytes()).await.ok();
            });
        }
    });
    (addr, log)
}

fn client(addr: &str, auth: VaultAuth) -> VaultClient {
    VaultClient::new(VaultConfig { addr: addr.to_owned(), namespace: None, auth }, Network::default())
}

fn token(addr: &str) -> VaultClient {
    client(addr, VaultAuth::Token("s.jeton".into()))
}

fn kv(seen: &Seen) -> (u16, String) {
    match (seen.method.as_str(), seen.path.as_str()) {
        ("GET", "/v1/secret/data/app/db") => {
            (200, r#"{"data":{"data":{"password":"p@ss","port":5432,"user":"ada"},"metadata":{"version":3}}}"#.into())
        }
        ("GET", "/v1/kv1/app/db") => (200, r#"{"data":{"password":"ancien"}}"#.into()),
        ("GET", "/v1/secret/data/interdit") => {
            (403, r#"{"errors":["1 error occurred:\n\t* permission denied"]}"#.into())
        }
        ("GET", "/v1/auth/token/lookup-self") => {
            (200, r#"{"data":{"display_name":"token","policies":["default","app"],"ttl":3600}}"#.into())
        }
        _ => (404, r#"{"errors":[]}"#.into()),
    }
}

#[test]
fn ef_var_03_a_vault_reference_is_read_as_mount_path_and_field() {
    let r = VaultRef::parse("$vault.secret/app/db#password").unwrap();
    assert_eq!((r.mount.as_str(), r.path.as_str(), r.field.as_str()), ("secret", "app/db", "password"));
    assert_eq!(r.variable(), "$vault.secret/app/db#password");
    assert_eq!(r.secret(), "secret/app/db");
    assert_eq!(VaultRef::parse("/secret/app#k").unwrap().mount, "secret");
    assert_eq!(VaultRef::parse(" secret/app # k ").unwrap().field, "k");
    for bad in ["secret#k", "secret/app", "secret/app#", "secret//app#k", "#k", ""] {
        assert!(matches!(VaultRef::parse(bad), Err(VaultError::BadReference(_))), "{bad}");
    }
}

#[test]
fn ef_var_03_only_unresolved_vault_names_are_picked_out_with_their_errors() {
    let names =
        vec!["baseUrl".to_owned(), "$vault.secret/a#b".to_owned(), "$guid".to_owned(), "$vault.mauvais".to_owned()];
    let found = references(&names);
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].as_ref().unwrap().field, "b");
    assert!(found[1].is_err());
}

#[tokio::test]
async fn ef_var_03_a_field_of_a_kv_v2_secret_is_read_with_the_token_and_the_namespace() {
    let (addr, log) = serve(kv).await;
    let vault = VaultClient::new(
        VaultConfig { addr: addr.clone(), namespace: Some("equipe".into()), auth: VaultAuth::Token("s.jeton".into()) },
        Network::default(),
    );

    let password = vault.read(&VaultRef::parse("secret/app/db#password").unwrap()).await.unwrap();

    assert_eq!(password, "p@ss");
    let seen = log.lock().unwrap()[0].clone();
    assert_eq!((seen.method.as_str(), seen.path.as_str()), ("GET", "/v1/secret/data/app/db"));
    assert_eq!((seen.token.as_deref(), seen.namespace.as_deref()), (Some("s.jeton"), Some("equipe")));
}

#[tokio::test]
async fn ef_var_03_a_field_that_is_not_text_is_given_as_json() {
    let (addr, _) = serve(kv).await;
    assert_eq!(token(&addr).read(&VaultRef::parse("secret/app/db#port").unwrap()).await.unwrap(), "5432");
}

#[tokio::test]
async fn ef_var_03_a_kv_v1_mount_is_read_when_the_v2_path_does_not_exist() {
    let (addr, log) = serve(kv).await;

    let value = token(&addr).read(&VaultRef::parse("kv1/app/db#password").unwrap()).await.unwrap();

    assert_eq!(value, "ancien");
    let paths: Vec<String> = log.lock().unwrap().iter().map(|s| s.path.clone()).collect();
    assert_eq!(paths, ["/v1/kv1/data/app/db", "/v1/kv1/app/db"]);
}

#[tokio::test]
async fn ef_var_03_a_missing_secret_a_missing_field_and_a_refusal_are_told_apart() {
    let (addr, _) = serve(kv).await;
    let vault = token(&addr);

    let absent = vault.read(&VaultRef::parse("secret/rien#x").unwrap()).await.unwrap_err();
    assert_eq!(absent, VaultError::NotFound { path: "secret/rien".into() });

    let field = vault.read(&VaultRef::parse("secret/app/db#mot").unwrap()).await.unwrap_err();
    assert_eq!(field.to_string(), "le secret secret/app/db n'a pas de champ « mot » (champs : password, port, user)");

    let refused = vault.read(&VaultRef::parse("secret/interdit#x").unwrap()).await.unwrap_err();
    match refused {
        VaultError::Refused { status, message } => {
            assert_eq!(status, 403);
            assert!(message.contains("permission denied"), "{message}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn ef_var_03_a_secret_is_read_once_for_several_fields() {
    let (addr, log) = serve(kv).await;
    let vault = token(&addr);

    vault.read(&VaultRef::parse("secret/app/db#password").unwrap()).await.unwrap();
    vault.read(&VaultRef::parse("secret/app/db#user").unwrap()).await.unwrap();

    assert_eq!(log.lock().unwrap().len(), 1);
}

fn approle(seen: &Seen) -> (u16, String) {
    match (seen.method.as_str(), seen.path.as_str()) {
        ("POST", "/v1/auth/approle/login")
            if seen.body.contains("\"secret_id\":\"bon\"") && seen.body.contains("\"role_id\":\"r-1\"") =>
        {
            (200, r#"{"auth":{"client_token":"s.obtenu"}}"#.into())
        }
        ("POST", "/v1/auth/approle/login") => (400, r#"{"errors":["invalid secret id"]}"#.into()),
        ("GET", "/v1/secret/data/app") if seen.token.as_deref() == Some("s.obtenu") => {
            (200, r#"{"data":{"data":{"k":"v"}}}"#.into())
        }
        _ => (403, r#"{"errors":["permission denied"]}"#.into()),
    }
}

#[tokio::test]
async fn ef_var_03_an_approle_logs_in_once_and_uses_the_token_it_gets() {
    let (addr, log) = serve(approle).await;
    let vault =
        client(&addr, VaultAuth::AppRole { role_id: "r-1".into(), secret_id: "bon".into(), mount: String::new() });

    assert_eq!(vault.read(&VaultRef::parse("secret/app#k").unwrap()).await.unwrap(), "v");
    vault.check().await.ok();
    let logins = log.lock().unwrap().iter().filter(|s| s.path.ends_with("/login")).count();
    assert_eq!(logins, 1);

    let wrong = client(
        &addr,
        VaultAuth::AppRole { role_id: "r-1".into(), secret_id: "mauvais".into(), mount: "approle".into() },
    );
    match wrong.read(&VaultRef::parse("secret/app#k").unwrap()).await.unwrap_err() {
        VaultError::Refused { status, message } => assert_eq!((status, message.as_str()), (400, "invalid secret id")),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn ef_var_03_the_connection_check_reports_the_token_policies_and_ttl() {
    let (addr, _) = serve(kv).await;

    let info = token(&addr).check().await.unwrap();

    assert_eq!((info.display_name.as_str(), info.ttl), ("token", Some(3600)));
    assert_eq!(info.policies, ["default", "app"]);
}

#[tokio::test]
async fn ef_var_03_a_missing_address_a_missing_token_and_a_down_server_are_errors() {
    let reference = VaultRef::parse("secret/app/db#password").unwrap();
    assert_eq!(client("", VaultAuth::Token("x".into())).read(&reference).await.unwrap_err(), VaultError::NotConfigured);
    assert_eq!(client("", VaultAuth::Token("x".into())).check().await.unwrap_err(), VaultError::NotConfigured);
    let (addr, _) = serve(kv).await;
    assert_eq!(client(&addr, VaultAuth::Token("  ".into())).read(&reference).await.unwrap_err(), VaultError::NoToken);

    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let error = token(&format!("http://127.0.0.1:{port}")).read(&reference).await.unwrap_err();
    assert!(matches!(&error, VaultError::Request(m) if m.starts_with("Vault injoignable")), "{error:?}");
}

#[test]
fn ef_var_03_the_debug_output_never_shows_a_token_or_a_secret_id() {
    let shown = format!(
        "{:?} {:?}",
        VaultAuth::Token("s.très-secret".into()),
        VaultAuth::AppRole { role_id: "r-1".into(), secret_id: "tres-secret".into(), mount: "approle".into() }
    );
    assert!(!shown.contains("très-secret") && !shown.contains("tres-secret"), "{shown}");
    assert!(shown.contains("r-1"));
}
