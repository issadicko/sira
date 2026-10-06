//! Les réglages réseau de l'application : TLS, autorité de certification et proxy. Ils vivent dans `network.json` du
//! dossier de données ; le mot de passe du proxy n'y est jamais écrit, il reste dans le trousseau du système.

use serde::Serialize;
use tauri::State;
use xc_core::{NetworkPrefs, ProxyMode};
use xc_secrets::{SecretKey, SecretStore};

use crate::{err, AppState, Reply};

fn password_key() -> SecretKey {
    SecretKey::new("@application", "reseau", "mot-de-passe-du-proxy")
}

/// Les préférences telles que l'envoi les utilise : le mot de passe du proxy manuel, lu dans le trousseau, en plus. Un
/// trousseau indisponible laisse le mot de passe vide : la requête part sans, et le proxy la refusera.
pub fn with_password(store: &dyn SecretStore, mut prefs: NetworkPrefs) -> NetworkPrefs {
    if prefs.proxy.mode == ProxyMode::Manual && !prefs.proxy.config.auth_disabled {
        prefs.proxy.config.password = store.get(&password_key()).ok().flatten().unwrap_or_default();
    }
    prefs
}

/// Les préférences à donner à une session, mot de passe du proxy compris.
pub fn current(state: &AppState) -> Reply<NetworkPrefs> {
    let prefs = state.network.lock().map_err(err)?.clone();
    Ok(with_password(&*state.secrets.0, prefs))
}

/// Les préférences vues par l'interface : elle sait seulement qu'un mot de passe de proxy existe.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkView {
    prefs: NetworkPrefs,
    proxy_password_set: bool,
}

fn view(state: &AppState) -> Reply<NetworkView> {
    let prefs = state.network.lock().map_err(err)?.clone();
    let proxy_password_set = state.secrets.0.get(&password_key()).ok().flatten().is_some();
    Ok(NetworkView { prefs, proxy_password_set })
}

#[tauri::command]
pub fn network_get(state: State<'_, AppState>) -> Reply<NetworkView> {
    view(&state)
}

/// Enregistre les préférences. `proxy_password` : `None` garde le mot de passe en place, une chaîne vide l'oublie, un
/// texte le range dans le trousseau.
#[tauri::command]
pub async fn network_save(
    state: State<'_, AppState>,
    prefs: NetworkPrefs,
    proxy_password: Option<String>,
) -> Reply<NetworkView> {
    let mut prefs = prefs;
    prefs.ca_file = prefs.ca_file.map(|file| file.trim().to_owned()).filter(|file| !file.is_empty());
    prefs.validate()?;
    match proxy_password.as_deref() {
        None => {}
        Some("") => state.secrets.0.delete(&password_key()).map_err(err)?,
        Some(password) => state.secrets.0.set(&password_key(), password).map_err(err)?,
    }
    let data = state.data.lock().map_err(err)?.clone();
    if let Some(dir) = data {
        let to_save = prefs.clone();
        crate::blocking(move || to_save.save(&dir)).await?;
    }
    *state.network.lock().map_err(err)? = prefs;
    view(&state)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tauri::Manager;
    use xc_core::{ProxyConfig, ProxyPref};

    use super::*;
    use crate::secrets::Secrets;

    fn manual(username: &str) -> NetworkPrefs {
        NetworkPrefs {
            proxy: ProxyPref {
                mode: ProxyMode::Manual,
                config: ProxyConfig { hostname: "p.test".into(), username: username.into(), ..ProxyConfig::default() },
            },
            ..NetworkPrefs::default()
        }
    }

    #[tokio::test]
    async fn ef_req_04_the_proxy_password_lives_in_the_keychain_and_never_in_the_preferences_file() {
        let store = Arc::new(xc_secrets::MemoryStore::default());
        let data = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        app.manage(AppState {
            secrets: Secrets(store.clone()),
            data: Mutex::new(Some(data.path().to_path_buf())),
            ..AppState::default()
        });
        let state = || app.state::<AppState>();

        let saved = network_save(state(), manual("alice"), Some("s3cr3t".into())).await.unwrap();

        assert!(saved.proxy_password_set);
        let on_disk = std::fs::read_to_string(data.path().join("network.json")).unwrap();
        assert!(on_disk.contains("alice") && !on_disk.contains("s3cr3t"), "{on_disk}");
        assert_eq!(current(&state()).unwrap().proxy.config.password, "s3cr3t", "l'envoi reçoit le mot de passe");
        let seen = serde_json::to_string(&network_get(state()).unwrap()).unwrap();
        assert!(!seen.contains("s3cr3t"), "l'interface ne le reçoit jamais : {seen}");

        network_save(state(), manual("alice"), None).await.unwrap();
        assert!(network_get(state()).unwrap().proxy_password_set, "None : le mot de passe reste");
        let cleared = network_save(state(), manual("alice"), Some(String::new())).await.unwrap();
        assert!(!cleared.proxy_password_set);
        assert_eq!(current(&state()).unwrap().proxy.config.password, "");
    }

    #[tokio::test]
    async fn ef_req_04_invalid_preferences_are_refused_and_nothing_is_written() {
        let data = tempfile::tempdir().unwrap();
        let app = tauri::test::mock_app();
        app.manage(AppState { data: Mutex::new(Some(data.path().to_path_buf())), ..AppState::default() });
        let mut bad = manual("");
        bad.proxy.config.port = "99999".into();

        let error = network_save(app.state::<AppState>(), bad, None).await.unwrap_err();

        assert!(error.contains("port de proxy invalide"), "{error}");
        assert!(!data.path().join("network.json").exists());
        assert_eq!(network_get(app.state::<AppState>()).unwrap().prefs, NetworkPrefs::default());
    }

    #[tokio::test]
    async fn ef_req_04_the_authority_path_is_trimmed_and_a_blank_one_is_dropped() {
        let app = tauri::test::mock_app();
        app.manage(AppState::default());
        let with = |file: &str| NetworkPrefs { ca_file: Some(file.into()), ..NetworkPrefs::default() };

        let kept = network_save(app.state::<AppState>(), with("  /certs/ca.pem \n"), None).await.unwrap();
        assert_eq!(kept.prefs.ca_file.as_deref(), Some("/certs/ca.pem"));
        let dropped = network_save(app.state::<AppState>(), with("   "), None).await.unwrap();
        assert_eq!(dropped.prefs.ca_file, None);
    }

    #[tokio::test]
    async fn ef_req_04_a_send_goes_through_the_saved_proxy_with_the_keychain_password() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(Mutex::new(String::new()));
        let log = Arc::clone(&seen);
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap();
            *log.lock().unwrap() = String::from_utf8_lossy(&buf[..n]).to_lowercase();
            write!(stream, "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok").unwrap();
        });
        let store = Arc::new(xc_secrets::MemoryStore::default());
        let (data, collection) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let app = tauri::test::mock_app();
        app.manage(AppState {
            secrets: Secrets(store),
            data: Mutex::new(Some(data.path().to_path_buf())),
            ..AppState::default()
        });
        let state = || app.state::<AppState>();
        let root = collection.path();
        std::fs::write(root.join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: S\n").unwrap();
        std::fs::write(
            root.join("req.yml"),
            "info:\n  name: R\n  type: http\n\nhttp:\n  method: GET\n  url: http://cible.test/x\n",
        )
        .unwrap();
        let mut prefs = manual("alice");
        prefs.proxy.config.port = port.to_string();
        prefs.proxy.config.hostname = "127.0.0.1".into();
        network_save(state(), prefs, Some("s3cr3t".into())).await.unwrap();
        let doc = xc_core::read_request(root, "req.yml").unwrap();

        let args = crate::SendArgs {
            id: "1".into(),
            root: root.display().to_string(),
            path: "req.yml".into(),
            doc,
            env: None,
        };
        let sent = crate::send_request(app.handle().clone(), state(), args).await.unwrap();

        assert_eq!(sent.response.as_ref().map(|r| r.status), Some(200), "{:?}", sent.error);
        let head = seen.lock().unwrap().clone();
        assert!(head.starts_with("get http://cible.test/x http/1.1"), "{head}");
        assert!(head.contains("proxy-authorization: basic ywxpy2u6cznjcjn0"), "{head}");
    }

    #[test]
    fn ef_req_04_only_a_manual_proxy_with_authentication_reads_the_password() {
        let store = xc_secrets::MemoryStore::default();
        store.set(&password_key(), "pw").unwrap();
        assert_eq!(with_password(&store, manual("a")).proxy.config.password, "pw");
        assert_eq!(with_password(&store, NetworkPrefs::default()).proxy.config.password, "");
        let mut no_auth = manual("a");
        no_auth.proxy.config.auth_disabled = true;
        assert_eq!(with_password(&store, no_auth).proxy.config.password, "");
    }
}
