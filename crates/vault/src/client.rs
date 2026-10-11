use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use xc_engine::{send, HttpRequest, Network};

use crate::reference::VaultRef;
use crate::{Result, VaultError};

const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BODY: u64 = 4 * 1024 * 1024;
/// Un secret lu est gardé ce temps : plusieurs requêtes d'un run n'interrogent Vault qu'une fois, un secret changé est relu.
const CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone, PartialEq, Eq)]
pub enum VaultAuth {
    Token(String),
    AppRole { role_id: String, secret_id: String, mount: String },
}

impl std::fmt::Debug for VaultAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Token(_) => f.write_str("Token(…)"),
            Self::AppRole { role_id, mount, .. } => {
                write!(f, "AppRole {{ role_id: {role_id:?}, mount: {mount:?}, secret_id: … }}")
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultConfig {
    pub addr: String,
    pub namespace: Option<String>,
    pub auth: VaultAuth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultInfo {
    pub display_name: String,
    pub policies: Vec<String>,
    /// Durée de vie restante en secondes ; `None` pour un jeton sans expiration.
    pub ttl: Option<u64>,
}

#[derive(Debug)]
pub struct VaultClient {
    config: VaultConfig,
    network: Network,
    token: Mutex<Option<String>>,
    cache: Mutex<HashMap<String, (Instant, Value)>>,
}

fn request_error(error: impl std::fmt::Display) -> VaultError {
    VaultError::Request(format!("Vault injoignable : {error}"))
}

fn messages(body: &[u8]) -> String {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let errors: Vec<&str> =
        value["errors"].as_array().map(|list| list.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    if errors.is_empty() {
        String::from_utf8_lossy(&body[..body.len().min(200)]).trim().to_owned()
    } else {
        errors.join(" ; ")
    }
}

impl VaultClient {
    pub fn new(config: VaultConfig, network: Network) -> Self {
        Self { config, network, token: Mutex::new(None), cache: Mutex::new(HashMap::new()) }
    }

    fn url(&self, route: &str) -> String {
        format!("{}/v1/{}", self.config.addr.trim_end_matches('/'), route.trim_start_matches('/'))
    }

    async fn call(
        &self,
        method: &str,
        route: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Result<(u16, Value, Vec<u8>)> {
        let mut headers = vec![("Accept".to_owned(), "application/json".to_owned())];
        if let Some(token) = token {
            headers.push(("X-Vault-Token".into(), token.to_owned()));
        }
        if let Some(namespace) = self.config.namespace.as_ref().filter(|n| !n.is_empty()) {
            headers.push(("X-Vault-Namespace".into(), namespace.clone()));
        }
        if body.is_some() {
            headers.push(("Content-Type".into(), "application/json".into()));
        }
        let mut network = self.network.clone();
        network.redirects.follow = false;
        let response = send(HttpRequest {
            method: method.to_owned(),
            url: self.url(route),
            headers,
            body: body.map(|value| value.to_string().into_bytes()),
            timeout: TIMEOUT,
            max_response_body: Some(MAX_BODY),
            network,
        })
        .await
        .map_err(request_error)?;
        let json = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        Ok((response.status, json, response.body))
    }

    /// Le jeton à présenter : celui donné, ou celui qu'un AppRole obtient (gardé pour les appels suivants).
    async fn token(&self) -> Result<String> {
        if let Some(token) = self.token.lock().map_err(request_error)?.clone() {
            return Ok(token);
        }
        let token = match &self.config.auth {
            VaultAuth::Token(token) if token.trim().is_empty() => return Err(VaultError::NoToken),
            VaultAuth::Token(token) => token.trim().to_owned(),
            VaultAuth::AppRole { role_id, secret_id, mount } => {
                let mount = if mount.is_empty() { "approle" } else { mount };
                let body = serde_json::json!({ "role_id": role_id, "secret_id": secret_id });
                let (status, json, raw) = self.call("POST", &format!("auth/{mount}/login"), None, Some(body)).await?;
                if status != 200 {
                    return Err(VaultError::Refused { status, message: messages(&raw) });
                }
                json["auth"]["client_token"].as_str().map(str::to_owned).ok_or(VaultError::NoToken)?
            }
        };
        *self.token.lock().map_err(request_error)? = Some(token.clone());
        Ok(token)
    }

    /// Vérifie la connexion et le jeton (`auth/token/lookup-self`).
    pub async fn check(&self) -> Result<VaultInfo> {
        if self.config.addr.trim().is_empty() {
            return Err(VaultError::NotConfigured);
        }
        let token = self.token().await?;
        let (status, json, raw) = self.call("GET", "auth/token/lookup-self", Some(&token), None).await?;
        if status != 200 {
            return Err(VaultError::Refused { status, message: messages(&raw) });
        }
        let data = &json["data"];
        Ok(VaultInfo {
            display_name: data["display_name"].as_str().unwrap_or_default().to_owned(),
            policies: data["policies"]
                .as_array()
                .map(|list| list.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default(),
            ttl: data["ttl"].as_u64().filter(|ttl| *ttl > 0),
        })
    }

    /// Le secret `montage/chemin` : moteur KV version 2 d'abord, version 1 quand le chemin de la version 2 n'existe pas.
    async fn secret(&self, reference: &VaultRef) -> Result<Value> {
        let key = reference.secret();
        if let Some((at, value)) = self.cache.lock().map_err(request_error)?.get(&key) {
            if at.elapsed() < CACHE_TTL {
                return Ok(value.clone());
            }
        }
        let token = self.token().await?;
        let (status, json, raw) =
            self.call("GET", &format!("{}/data/{}", reference.mount, reference.path), Some(&token), None).await?;
        let data = match status {
            200 => json["data"]["data"].clone(),
            404 => {
                let (status, json, raw) =
                    self.call("GET", &format!("{}/{}", reference.mount, reference.path), Some(&token), None).await?;
                match status {
                    200 => json["data"].clone(),
                    404 => return Err(VaultError::NotFound { path: key }),
                    status => return Err(VaultError::Refused { status, message: messages(&raw) }),
                }
            }
            status => return Err(VaultError::Refused { status, message: messages(&raw) }),
        };
        self.cache.lock().map_err(request_error)?.insert(key, (Instant::now(), data.clone()));
        Ok(data)
    }

    /// La valeur du champ ; un champ qui n'est pas du texte est rendu en JSON.
    pub async fn read(&self, reference: &VaultRef) -> Result<String> {
        if self.config.addr.trim().is_empty() {
            return Err(VaultError::NotConfigured);
        }
        let data = self.secret(reference).await?;
        match data.get(&reference.field) {
            Some(Value::String(text)) => Ok(text.clone()),
            Some(other) => Ok(other.to_string()),
            None => {
                let mut fields: Vec<&str> =
                    data.as_object().map(|map| map.keys().map(String::as_str).collect()).unwrap_or_default();
                fields.sort_unstable();
                Err(VaultError::NoField {
                    path: reference.secret(),
                    field: reference.field.clone(),
                    fields: fields.join(", "),
                })
            }
        }
    }
}
