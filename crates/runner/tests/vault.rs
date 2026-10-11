use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use xc_core::{read_request, NetworkPrefs, VaultPrefs};
use xc_runner::{run_request, vault_config_of, Request, Session, Stage, VaultCredentials};

type Seen = Arc<Mutex<Vec<String>>>;

fn serve() -> (String, Seen) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen: Seen = Arc::default();
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let mut buf = [0u8; 16384];
            let n = stream.read(&mut buf).unwrap_or(0);
            let raw = String::from_utf8_lossy(&buf[..n]).into_owned();
            let path = raw.split(' ').nth(1).unwrap_or_default().to_owned();
            let token = raw
                .lines()
                .find_map(|l| l.to_ascii_lowercase().starts_with("x-vault-token:").then(|| l[14..].trim().to_owned()));
            log.lock().unwrap().push(raw);
            let (status, body) = match path.as_str() {
                "/v1/secret/data/app" if token.as_deref() == Some("s.jeton") => {
                    (200, r#"{"data":{"data":{"pass":"p@ss","api":"cle-123"}}}"#)
                }
                "/v1/secret/data/app" | "/v1/secret/data/interdit" => (403, r#"{"errors":["permission denied"]}"#),
                p if p.starts_with("/v1/") => (404, r#"{"errors":[]}"#),
                _ => (200, "ok"),
            };
            let _ = write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    (base, seen)
}

fn collection(base: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("opencollection.yml"),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: Vault\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: {base}\n    - name: dbPass\n      value: \"{{{{$vault.secret/app#pass}}}}\"\n"),
    )
    .unwrap();
    dir
}

fn request(root: &Path, body: &str) -> (String, xc_core::RequestDoc) {
    fs::write(root.join("r.yml"), format!("info:\n  name: R\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{{{baseUrl}}}}/echo\"\n  headers:\n{body}")).unwrap();
    ("r.yml".to_owned(), read_request(root, "r.yml").unwrap())
}

fn session(base: &str, token: Option<&str>) -> Session {
    Session {
        network: NetworkPrefs {
            vault: VaultPrefs { addr: base.to_owned(), ..VaultPrefs::default() },
            ..NetworkPrefs::default()
        },
        vault_credentials: VaultCredentials { token: token.map(str::to_owned), secret_id: None },
        ..Session::default()
    }
}

async fn run(root: &Path, path: &str, doc: &xc_core::RequestDoc, session: &mut Session) -> xc_runner::Outcome {
    run_request(
        Request {
            root,
            path,
            doc,
            env: None,
            collection_name: "Vault",
            execution_mode: "cli",
            cancel: Arc::new(AtomicBool::new(false)),
        },
        session,
    )
    .await
}

#[tokio::test]
async fn ef_var_03_a_vault_variable_in_a_header_and_one_inside_a_collection_variable_are_filled_before_sending() {
    let (base, seen) = serve();
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), "    - name: X-Pass\n      value: \"{{dbPass}}\"\n    - name: X-Api\n      value: \"{{$vault.secret/app#api}}\"\n");
    let mut session = session(&base, Some("s.jeton"));

    let outcome = run(dir.path(), &path, &doc, &mut session).await;

    assert!(outcome.error.is_none(), "{:?}", outcome.error);
    assert!(outcome.unresolved.is_empty(), "{:?}", outcome.unresolved);
    let header =
        |name: &str| outcome.sent_headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone());
    assert_eq!(header("x-pass").as_deref(), Some("p@ss"));
    assert_eq!(header("x-api").as_deref(), Some("cle-123"));
    let vault_reads = seen.lock().unwrap().iter().filter(|r| r.contains("/v1/secret/data/app")).count();
    assert_eq!(vault_reads, 1, "le secret n'est lu qu'une fois pour ses deux champs");
}

#[tokio::test]
async fn ef_var_03_a_refused_vault_read_fails_the_request_with_the_variable_and_the_reason_and_sends_nothing() {
    let (base, seen) = serve();
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), "    - name: X-Pass\n      value: \"{{$vault.secret/interdit#x}}\"\n");
    let mut session = session(&base, Some("s.jeton"));

    let outcome = run(dir.path(), &path, &doc, &mut session).await;

    let error = outcome.error.expect("une erreur de préparation");
    assert!(matches!(error.stage, Stage::Prepare));
    assert!(
        error.message.contains("$vault.secret/interdit#x") && error.message.contains("permission denied"),
        "{}",
        error.message
    );
    assert!(!seen.lock().unwrap().iter().any(|r| r.starts_with("GET /echo")), "rien n'est envoyé");
}

#[tokio::test]
async fn ef_var_03_without_a_vault_address_or_with_a_bad_reference_the_request_says_why() {
    let (base, _) = serve();
    let dir = collection(&base);
    let (path, doc) = request(dir.path(), "    - name: X-Pass\n      value: \"{{$vault.secret/app#pass}}\"\n");

    let outcome = run(dir.path(), &path, &doc, &mut Session::default()).await;
    assert!(outcome.error.unwrap().message.contains("Vault n'est pas configuré"));

    let (path, doc) = request(dir.path(), "    - name: X-Pass\n      value: \"{{$vault.mauvais}}\"\n");
    let outcome = run(dir.path(), &path, &doc, &mut session(&base, Some("s.jeton"))).await;
    assert!(outcome.error.unwrap().message.contains("référence Vault invalide"));
}

#[tokio::test]
async fn ef_var_03_checking_the_connection_uses_the_session_settings() {
    let (base, _) = serve();
    let error = session(&base, Some("s.jeton")).check_vault(None).await.unwrap_err();
    assert!(error.contains("404") || error.contains("refusé"), "{error}");
    assert!(Session::default().check_vault(None).await.unwrap_err().contains("n'est pas configuré"));
}

fn env_of<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
    move |name| pairs.iter().find(|(k, _)| *k == name).map(|(_, v)| (*v).to_owned())
}

#[test]
fn ef_var_03_the_settings_come_first_then_the_host_credentials_then_the_environment_then_the_token_file() {
    let home = tempfile::tempdir().unwrap();
    fs::write(home.path().join(".vault-token"), "s.fichier\n").unwrap();
    let none = VaultCredentials::default();

    assert!(
        vault_config_of(&VaultPrefs::default(), &none, &env_of(&[]), None).is_none(),
        "sans adresse : pas de Vault"
    );

    let prefs = VaultPrefs { addr: "https://vault.test".into(), namespace: "equipe".into(), ..VaultPrefs::default() };
    let env = env_of(&[("VAULT_ADDR", "https://autre.test"), ("VAULT_TOKEN", "s.env"), ("VAULT_NAMESPACE", "ignoré")]);
    let config = vault_config_of(&prefs, &none, &env, Some(home.path())).unwrap();
    assert_eq!((config.addr.as_str(), config.namespace.as_deref()), ("https://vault.test", Some("equipe")));
    assert_eq!(config.auth, xc_vault::VaultAuth::Token("s.env".into()));

    let own = VaultCredentials { token: Some("s.trousseau".into()), secret_id: None };
    assert_eq!(
        vault_config_of(&prefs, &own, &env, None).unwrap().auth,
        xc_vault::VaultAuth::Token("s.trousseau".into())
    );
    let from_env_only = vault_config_of(&VaultPrefs::default(), &none, &env, Some(home.path())).unwrap();
    assert_eq!(from_env_only.addr, "https://autre.test");
    let from_file = vault_config_of(&prefs, &none, &env_of(&[]), Some(home.path())).unwrap();
    assert_eq!(from_file.auth, xc_vault::VaultAuth::Token("s.fichier".into()));

    let approle = VaultPrefs { auth: "approle".into(), role_id: "r-1".into(), approle_mount: "ci".into(), ..prefs };
    let creds = VaultCredentials { token: None, secret_id: Some("sid".into()) };
    assert_eq!(
        vault_config_of(&approle, &creds, &env_of(&[]), None).unwrap().auth,
        xc_vault::VaultAuth::AppRole { role_id: "r-1".into(), secret_id: "sid".into(), mount: "ci".into() }
    );
}
