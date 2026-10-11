//! Les variables `{{$vault.montage/chemin#champ}}` : l'interpolation ne sait pas aller chercher une valeur sur le réseau,
//! elle rend donc ces noms comme « sans valeur ». On les lit alors dans Vault, on les donne comme variables et on prépare
//! de nouveau, jusqu'à ce que plus aucun nom Vault ne manque (une valeur peut elle-même contenir une autre référence).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use xc_core::vars::Context;
use xc_core::{prepare_with, NetworkPrefs, Overrides, Prepared, RequestDoc, VaultPrefs};
use xc_engine::{Network, Redirects};
use xc_vault::{references, VaultAuth, VaultClient, VaultConfig, VaultError, VaultRef};

use crate::session::Session;

/// Le jeton ou le `secret_id` qu'un hôte a lus (trousseau de l'application) ; vides, ce sont `VAULT_TOKEN`,
/// `~/.vault-token` et `VAULT_SECRET_ID` qui servent.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct VaultCredentials {
    pub token: Option<String>,
    pub secret_id: Option<String>,
}

impl std::fmt::Debug for VaultCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultCredentials")
            .field("token", &self.token.as_ref().map(|_| "…"))
            .field("secret_id", &self.secret_id.as_ref().map(|_| "…"))
            .finish()
    }
}

/// Le client en cours de la session, gardé tant que les réglages ne changent pas : il garde le jeton obtenu et les
/// secrets lus pendant quelques secondes.
#[derive(Debug, Clone, Default)]
pub struct VaultSlot(Arc<Mutex<Option<Current>>>);

type Current = (VaultConfig, Arc<VaultClient>);

/// Tout ce que `VaultConfig` demande, lu dans les réglages, les identifiants de l'hôte puis l'environnement du processus.
pub fn config_of(
    prefs: &VaultPrefs,
    credentials: &VaultCredentials,
    env: &dyn Fn(&str) -> Option<String>,
    home: Option<&Path>,
) -> Option<VaultConfig> {
    let pick = |own: &str, name: &str| {
        Some(own.trim().to_owned()).filter(|v| !v.is_empty()).or_else(|| env(name).filter(|v| !v.trim().is_empty()))
    };
    let addr = pick(&prefs.addr, "VAULT_ADDR")?;
    let namespace = pick(&prefs.namespace, "VAULT_NAMESPACE");
    let auth = if prefs.auth == "approle" {
        VaultAuth::AppRole {
            role_id: pick(&prefs.role_id, "VAULT_ROLE_ID").unwrap_or_default(),
            secret_id: credentials.secret_id.clone().or_else(|| env("VAULT_SECRET_ID")).unwrap_or_default(),
            mount: prefs.approle_mount.trim().to_owned(),
        }
    } else {
        let from_file =
            || home.and_then(|dir| std::fs::read_to_string(dir.join(".vault-token")).ok()).map(|t| t.trim().to_owned());
        VaultAuth::Token(
            credentials
                .token
                .clone()
                .filter(|t| !t.trim().is_empty())
                .or_else(|| env("VAULT_TOKEN").filter(|t| !t.trim().is_empty()))
                .or_else(from_file)
                .unwrap_or_default(),
        )
    };
    Some(VaultConfig { addr, namespace, auth })
}

fn process_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

impl Session {
    fn vault_config(&self) -> Option<VaultConfig> {
        config_of(&self.network.vault, &self.vault_credentials, &process_env, home().as_deref())
    }

    fn vault_client(&self, config: VaultConfig, network: Network) -> Arc<VaultClient> {
        let mut slot = self.vault.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match slot.as_ref() {
            Some((known, client)) if *known == config => client.clone(),
            _ => {
                let client = Arc::new(VaultClient::new(config.clone(), network));
                *slot = Some((config, client.clone()));
                client
            }
        }
    }

    /// Vérifie la connexion à Vault avec les réglages de la session.
    pub async fn check_vault(&self, root: Option<&Path>) -> Result<xc_vault::VaultInfo, String> {
        let config = self.vault_config().ok_or_else(|| VaultError::NotConfigured.to_string())?;
        let network = vault_network(root, &self.network, &config.addr).map_err(|e| e.to_string())?;
        let client = self.vault_client(config, network);
        client.check().await.map_err(|e| e.to_string())
    }
}

fn vault_network(root: Option<&Path>, prefs: &NetworkPrefs, addr: &str) -> Result<Network, xc_core::CoreError> {
    let (root, collection) = match root {
        Some(root) => (root, Context::load(root, "x.yml").map(|c| c.collection).unwrap_or_default()),
        None => (Path::new("."), Default::default()),
    };
    xc_core::network::resolve(root, &collection, prefs, addr, Redirects::default(), &mut |s| s.to_owned(), &|name| {
        std::env::var(name).ok()
    })
}

const ROUNDS: usize = 4;

/// [`prepare_with`] qui lit dans Vault les variables `{{$vault.…}}` que la préparation n'a pas pu remplir. Une référence
/// mal écrite ou une lecture refusée fait échouer la préparation avec la raison, au lieu d'envoyer le nom tel quel.
pub async fn prepare_with_vault(
    root: &Path,
    path: &str,
    doc: &RequestDoc,
    env: Option<&str>,
    session: &Session,
    overrides: Overrides,
) -> Result<Prepared, String> {
    let mut fetched: HashMap<String, String> = HashMap::new();
    for _ in 0..ROUNDS {
        let mut runtime = session.runtime_strings();
        runtime.extend(fetched.iter().map(|(k, v)| (k.clone(), v.clone())));
        let prepared = prepare_with(root, path, doc, env, &runtime, overrides.clone()).map_err(|e| e.to_string())?;
        let wanted: Vec<VaultRef> = references(&prepared.unresolved)
            .into_iter()
            .collect::<Result<_, _>>()
            .map_err(|e: VaultError| e.to_string())?;
        if wanted.is_empty() {
            return Ok(prepared);
        }
        let config = session.vault_config().ok_or_else(|| VaultError::NotConfigured.to_string())?;
        let network = vault_network(Some(root), &session.network, &config.addr).map_err(|e| e.to_string())?;
        let client = session.vault_client(config, network);
        for reference in wanted {
            let value = client.read(&reference).await.map_err(|e| format!("{} : {e}", reference.variable()))?;
            fetched.insert(reference.variable(), value);
        }
    }
    Err("trop de références Vault imbriquées".into())
}
