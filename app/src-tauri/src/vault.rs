//! Les réglages HashiCorp Vault de l'application : l'adresse, l'espace de noms et la méthode de connexion vivent avec les
//! réglages réseau ; le jeton et le `secret_id` d'un AppRole vivent dans le trousseau du système, jamais dans un fichier
//! ni dans l'interface (elle sait seulement qu'une valeur existe).

use std::path::Path;

use serde::Serialize;
use tauri::State;
use xc_core::VaultPrefs;
use xc_runner::{Session, VaultCredentials};
use xc_secrets::{SecretKey, SecretStore};

use crate::{err, AppState, Reply};

fn key(name: &str) -> SecretKey {
    SecretKey::new("", "vault", name)
}

/// Le jeton et le `secret_id` gardés dans le trousseau ; un trousseau indisponible n'en donne aucun.
pub fn credentials(store: &dyn SecretStore) -> VaultCredentials {
    VaultCredentials {
        token: store.get(&key("token")).ok().flatten(),
        secret_id: store.get(&key("secret-id")).ok().flatten(),
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultView {
    prefs: VaultPrefs,
    token_set: bool,
    secret_id_set: bool,
}

fn view(state: &AppState) -> Reply<VaultView> {
    let prefs = state.network.lock().map_err(err)?.vault.clone();
    let found = credentials(&*state.secrets.0);
    Ok(VaultView { prefs, token_set: found.token.is_some(), secret_id_set: found.secret_id.is_some() })
}

#[tauri::command]
pub fn vault_get(state: State<'_, AppState>) -> Reply<VaultView> {
    view(&state)
}

fn keep(store: &dyn SecretStore, name: &str, value: Option<&str>) -> Reply<()> {
    match value {
        None => Ok(()),
        Some("") => store.delete(&key(name)).map_err(err),
        Some(text) => store.set(&key(name), text.trim()).map_err(err),
    }
}

/// Enregistre les réglages Vault. `token` et `secret_id` : `None` garde la valeur en place, une chaîne vide l'oublie, un
/// texte la range dans le trousseau.
#[tauri::command]
pub async fn vault_save(
    state: State<'_, AppState>,
    prefs: VaultPrefs,
    token: Option<String>,
    secret_id: Option<String>,
) -> Reply<VaultView> {
    let clean = VaultPrefs {
        addr: prefs.addr.trim().trim_end_matches('/').to_owned(),
        namespace: prefs.namespace.trim().to_owned(),
        auth: if prefs.auth == "approle" { "approle".into() } else { "token".into() },
        role_id: prefs.role_id.trim().to_owned(),
        approle_mount: prefs.approle_mount.trim().trim_matches('/').to_owned(),
    };
    let has_scheme = clean.addr.starts_with("http://") || clean.addr.starts_with("https://");
    if !clean.addr.is_empty() && !has_scheme {
        return Err("L'adresse de Vault commence par http:// ou https://.".into());
    }
    keep(&*state.secrets.0, "token", token.as_deref())?;
    keep(&*state.secrets.0, "secret-id", secret_id.as_deref())?;
    let mut network = state.network.lock().map_err(err)?.clone();
    network.vault = clean;
    let data = state.data.lock().map_err(err)?.clone();
    if let Some(dir) = data {
        let to_save = network.clone();
        crate::blocking(move || to_save.save(&dir)).await?;
    }
    *state.network.lock().map_err(err)? = network;
    view(&state)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultCheck {
    display_name: String,
    policies: Vec<String>,
    ttl: Option<u64>,
}

/// Vérifie la connexion avec les réglages enregistrés ; `root` donne le réseau de la collection (proxy, autorités).
#[tauri::command]
pub async fn vault_check(state: State<'_, AppState>, root: Option<String>) -> Reply<VaultCheck> {
    let session = Session {
        network: crate::network::current(&state)?,
        vault_credentials: credentials(&*state.secrets.0),
        ..Session::default()
    };
    let info = session.check_vault(root.as_deref().map(Path::new)).await?;
    Ok(VaultCheck { display_name: info.display_name, policies: info.policies, ttl: info.ttl })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tauri::Manager;

    use super::*;
    use crate::secrets::Secrets;

    fn app(store: Arc<xc_secrets::MemoryStore>, data: &Path) -> tauri::App<tauri::test::MockRuntime> {
        let app = tauri::test::mock_app();
        app.manage(AppState {
            secrets: Secrets(store),
            data: Mutex::new(Some(data.to_path_buf())),
            ..AppState::default()
        });
        app
    }

    fn prefs(addr: &str) -> VaultPrefs {
        VaultPrefs { addr: addr.into(), namespace: " equipe ".into(), ..VaultPrefs::default() }
    }

    #[tokio::test]
    async fn ef_var_03_the_vault_token_lives_in_the_keychain_and_never_in_the_preferences_file() {
        let store = Arc::new(xc_secrets::MemoryStore::default());
        let data = tempfile::tempdir().unwrap();
        let app = app(store.clone(), data.path());
        let state = || app.state::<AppState>();

        let saved =
            vault_save(state(), prefs("https://vault.test/"), Some("s.tres-secret".into()), None).await.unwrap();

        assert!(saved.token_set && !saved.secret_id_set);
        assert_eq!((saved.prefs.addr.as_str(), saved.prefs.namespace.as_str()), ("https://vault.test", "equipe"));
        let on_disk = std::fs::read_to_string(data.path().join("network.json")).unwrap();
        assert!(on_disk.contains("vault.test") && !on_disk.contains("tres-secret"), "{on_disk}");
        assert_eq!(credentials(&*state().secrets.0).token.as_deref(), Some("s.tres-secret"));
        let seen = serde_json::to_string(&vault_get(state()).unwrap()).unwrap();
        assert!(!seen.contains("tres-secret"), "l'interface ne le reçoit jamais : {seen}");

        vault_save(state(), prefs("https://vault.test"), None, Some("sid".into())).await.unwrap();
        let kept = vault_get(state()).unwrap();
        assert!(kept.token_set && kept.secret_id_set, "None : les valeurs restent");
        let cleared = vault_save(state(), prefs("https://vault.test"), Some(String::new()), None).await.unwrap();
        assert!(!cleared.token_set && cleared.secret_id_set);
    }

    #[tokio::test]
    async fn ef_var_03_a_vault_address_without_a_scheme_is_refused_and_network_saves_keep_the_vault_settings() {
        let store = Arc::new(xc_secrets::MemoryStore::default());
        let data = tempfile::tempdir().unwrap();
        let app = app(store, data.path());
        let state = || app.state::<AppState>();

        let error = vault_save(state(), prefs("vault.test:8200"), None, None).await.unwrap_err();
        assert!(error.contains("http://"), "{error}");

        vault_save(state(), prefs("https://vault.test"), None, None).await.unwrap();
        crate::network::network_save(state(), xc_core::NetworkPrefs::default(), None).await.unwrap();
        assert_eq!(vault_get(state()).unwrap().prefs.addr, "https://vault.test");
    }

    #[tokio::test]
    async fn ef_var_03_checking_without_an_address_says_vault_is_not_configured() {
        let store = Arc::new(xc_secrets::MemoryStore::default());
        let data = tempfile::tempdir().unwrap();
        let app = app(store, data.path());

        let error = vault_check(app.state::<AppState>(), None).await.unwrap_err();

        assert!(error.contains("Vault n'est pas configuré"), "{error}");
    }
}
