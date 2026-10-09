//! Ouvrir une requête WebSocket du fichier : variables, en-têtes, auth, réglages réseau et cookies de la session, puis la
//! connexion. Les scripts ne tournent pas sur une connexion WebSocket (Bruno non plus).

use std::path::Path;
use std::time::Duration;

use xc_core::{prepare_with, PreparedMessage, RequestDoc, SendAuth};
use xc_engine::{connect_ws, WsConnection, WsOpened, WsRequest};

use crate::session::Session;

/// Délai pour joindre l'hôte quand le fichier n'en donne pas.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Une connexion ouverte, avec les messages du fichier prêts à partir.
#[derive(Debug)]
pub struct WsStart {
    pub connection: WsConnection,
    /// Les messages du fichier, variables résolues, dans l'ordre ; ceux dont `selected` est vrai partent à « Envoyer ».
    pub messages: Vec<PreparedMessage>,
    /// Les variables sans valeur rencontrées dans l'adresse, les en-têtes et les messages.
    pub unresolved: Vec<String>,
}

impl WsStart {
    pub fn selected(&self) -> impl Iterator<Item = &PreparedMessage> {
        self.messages.iter().filter(|m| m.selected)
    }
}

fn auth_name(auth: &SendAuth) -> Option<&'static str> {
    match auth {
        SendAuth::None => None,
        SendAuth::Digest { .. } => Some("Digest"),
        SendAuth::Aws(_) => Some("AWS Signature V4"),
        SendAuth::Oauth2(_) => Some("OAuth 2.0"),
    }
}

/// Prépare `doc` avec `session` (variables du fichier et de l'environnement `env`, réglages réseau, cookies) et ouvre la
/// connexion. Les erreurs sont dites en français ; une authentification qui demande un échange avec le serveur (Digest,
/// AWS, OAuth 2.0) n'est pas prise en charge sur WebSocket : elle est refusée plutôt qu'ignorée.
pub async fn open_websocket(
    root: &Path,
    path: &str,
    doc: &RequestDoc,
    env: Option<&str>,
    session: &Session,
) -> Result<WsStart, String> {
    if doc.request_type != "websocket" {
        return Err(format!("« {} » n'est pas une requête WebSocket (type : {})", doc.name, doc.request_type));
    }
    let overrides = session.request_overrides(None, env);
    let prepared =
        prepare_with(root, path, doc, env, &session.runtime_strings(), overrides).map_err(|e| e.to_string())?;
    if let Some(name) = auth_name(&prepared.auth) {
        return Err(format!("l'authentification {name} n'est pas prise en charge sur une connexion WebSocket"));
    }
    let request = WsRequest {
        url: prepared.request.url.clone(),
        headers: prepared.request.headers.clone(),
        timeout: doc.timeout_ms.map_or(CONNECT_TIMEOUT, Duration::from_millis),
        keep_alive: doc.keep_alive_ms.map(Duration::from_millis),
        max_message: None,
        network: prepared.request.network.clone(),
    };
    let connection = connect_ws(request).await.map_err(|e| e.to_string())?;
    Ok(WsStart { connection, messages: prepared.messages, unresolved: prepared.unresolved })
}

/// Un résumé de `opened` pour l'affichage : « 101, sous-protocole chat ».
pub fn describe_opened(opened: &WsOpened) -> String {
    match &opened.protocol {
        Some(protocol) => format!("{}, sous-protocole {protocol}", opened.status),
        None => opened.status.to_string(),
    }
}
