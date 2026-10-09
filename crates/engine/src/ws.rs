//! Client WebSocket : la connexion (DNS, proxy, TLS) est celle des requêtes HTTP, la mise à niveau et les trames sont
//! celles de `tungstenite`. Une tâche possède la connexion et parle au reste du programme par deux canaux : des
//! événements reçus d'un côté, des envois à faire de l'autre.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, WebSocketConfig};
use tokio_tungstenite::tungstenite::{self, Message};
use url::Url;

use crate::{cookies, dial, tls, EngineError, Network, ProxyScheme, Timings};

/// Combien de temps on attend la réponse du serveur à notre trame de fermeture avant de conclure seul.
const CLOSE_GRACE: Duration = Duration::from_secs(3);

/// En-têtes que la mise à niveau pose elle-même : les répéter la casserait.
const OWN_HEADERS: [&str; 6] =
    ["host", "connection", "upgrade", "sec-websocket-key", "sec-websocket-version", "sec-websocket-extensions"];

#[derive(Debug, Clone)]
pub struct WsRequest {
    /// `ws://`, `wss://`, ou `http://` et `https://` qui valent la même chose (comme dans un navigateur).
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Délai pour joindre l'hôte et achever la mise à niveau.
    pub timeout: Duration,
    /// Une trame ping à cet intervalle ; `None` : aucune.
    pub keep_alive: Option<Duration>,
    /// Taille maximale d'un message reçu, en octets ; `None` : celle de `tungstenite` (64 Mio).
    pub max_message: Option<usize>,
    pub network: Network,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WsOpened {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    /// Le sous-protocole que le serveur a choisi (`Sec-WebSocket-Protocol`).
    pub protocol: Option<String>,
    pub remote_addr: String,
    pub url: String,
    pub timings: Timings,
}

/// Ce qui arrive sur la connexion. Un texte est du texte ; un message binaire ou une charge de ping garde ses octets.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WsEvent {
    Text {
        data: String,
    },
    Binary {
        data: Vec<u8>,
    },
    Ping {
        data: Vec<u8>,
    },
    Pong {
        data: Vec<u8>,
    },
    /// La connexion est fermée ; `code` est absent quand la fermeture ne s'est pas faite par une trame.
    Close {
        code: Option<u16>,
        reason: String,
    },
    /// Une erreur de lecture ou d'écriture ; la connexion se ferme ensuite.
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum WsOutgoing {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Close { code: u16, reason: String },
}

/// Pour envoyer sur la connexion depuis n'importe où ; se clone.
#[derive(Debug, Clone)]
pub struct WsSender(mpsc::UnboundedSender<WsOutgoing>);

impl WsSender {
    pub fn send(&self, message: WsOutgoing) -> Result<(), EngineError> {
        self.0.send(message).map_err(|_| EngineError::WebSocket("la connexion est fermée".into()))
    }

    pub fn text(&self, data: impl Into<String>) -> Result<(), EngineError> {
        self.send(WsOutgoing::Text(data.into()))
    }

    /// Demande la fermeture normale (1000).
    pub fn close(&self) -> Result<(), EngineError> {
        self.send(WsOutgoing::Close { code: 1000, reason: String::new() })
    }
}

#[derive(Debug)]
pub struct WsConnection {
    pub opened: WsOpened,
    pub sender: WsSender,
    /// Se vide jusqu'à `Close` ou `Error`, puis se ferme.
    pub events: mpsc::UnboundedReceiver<WsEvent>,
}

impl WsConnection {
    pub async fn next(&mut self) -> Option<WsEvent> {
        self.events.recv().await
    }
}

fn rejected(error: tungstenite::Error) -> EngineError {
    match error {
        tungstenite::Error::Http(response) => {
            let status = response.status();
            let body = response
                .body()
                .as_deref()
                .map(|bytes| String::from_utf8_lossy(&bytes[..bytes.len().min(300)]).trim().to_owned())
                .filter(|text| !text.is_empty());
            let reason = status.canonical_reason().unwrap_or_default();
            EngineError::WebSocket(format!(
                "mise à niveau refusée par le serveur (HTTP {} {reason}){}",
                status.as_u16(),
                body.map(|b| format!(" : {b}")).unwrap_or_default()
            ))
        }
        other => EngineError::WebSocket(other.to_string()),
    }
}

fn config(request: &WsRequest) -> WebSocketConfig {
    WebSocketConfig::default().max_message_size(request.max_message.or(WebSocketConfig::default().max_message_size))
}

/// Ouvre la connexion et attend la fin de la mise à niveau (HTTP 101).
pub async fn connect_ws(request: WsRequest) -> Result<WsConnection, EngineError> {
    let mut url = Url::parse(&request.url).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    let (secure, scheme, jar_scheme) = match url.scheme() {
        "ws" | "http" => (false, "ws", "http"),
        "wss" | "https" => (true, "wss", "https"),
        other => return Err(EngineError::UnsupportedScheme(other.to_owned())),
    };
    url.set_scheme(scheme).map_err(|()| EngineError::InvalidUrl(request.url.clone()))?;
    let mut jar_url = url.clone();
    jar_url.set_scheme(jar_scheme).ok();
    if secure || request.network.proxy.as_ref().is_some_and(|proxy| proxy.scheme == ProxyScheme::Https) {
        tls::warm_up().await;
    }

    let cookies = request.network.cookies.clone();
    let jar_line = cookies.as_ref().filter(|c| c.send).and_then(|c| c.jar.header(&jar_url));
    let headers = cookies::with_jar_cookies(&request.headers, jar_line.as_deref());

    let timeout = request.timeout;
    let (socket, response, addr, timings) = tokio::time::timeout(timeout, async {
        let started = Instant::now();
        let dialed = dial(&url, secure, true, &request.network).await?;
        let mut upgrade = url.as_str().into_client_request().map_err(rejected)?;
        for (name, value) in headers.iter().filter(|(k, _)| !OWN_HEADERS.contains(&k.to_ascii_lowercase().as_str())) {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| EngineError::WebSocket(format!("nom d'en-tête invalide : {name}")))?;
            let value = HeaderValue::from_str(value)
                .map_err(|_| EngineError::WebSocket(format!("valeur d'en-tête invalide pour {name}")))?;
            upgrade.headers_mut().append(name, value);
        }
        let (socket, response) =
            tokio_tungstenite::client_async_with_config(upgrade, dialed.stream, Some(config(&request)))
                .await
                .map_err(rejected)?;
        let mut timings = dialed.timings;
        timings.total_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok::<_, EngineError>((socket, response, dialed.addr, timings))
    })
    .await
    .map_err(|_| EngineError::Timeout(timeout.as_millis()))??;

    let response_headers: Vec<(String, String)> = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();
    if let Some(cookies) = cookies.as_ref().filter(|c| c.store) {
        let received: Vec<&str> = response_headers
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
            .map(|(_, value)| value.as_str())
            .collect();
        cookies.jar.store(&jar_url, &received);
    }
    let opened = WsOpened {
        status: response.status().as_u16(),
        protocol: response_headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("sec-websocket-protocol"))
            .map(|(_, v)| v.clone()),
        headers: response_headers,
        remote_addr: addr.to_string(),
        url: url.to_string(),
        timings,
    };

    let (commands, outgoing) = mpsc::unbounded_channel();
    let (emit, events) = mpsc::unbounded_channel();
    tokio::spawn(run(socket, outgoing, emit, request.keep_alive));
    Ok(WsConnection { opened, sender: WsSender(commands), events })
}

fn close_frame(code: u16, reason: String) -> CloseFrame {
    CloseFrame { code: CloseCode::from(code), reason: reason.into() }
}

async fn run<S>(
    socket: tokio_tungstenite::WebSocketStream<S>,
    mut outgoing: mpsc::UnboundedReceiver<WsOutgoing>,
    emit: mpsc::UnboundedSender<WsEvent>,
    keep_alive: Option<Duration>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (mut sink, mut stream) = socket.split();
    let mut ticker = keep_alive.filter(|d| !d.is_zero()).map(|d| tokio::time::interval_at(Instant::now() + d, d));
    let mut deadline: Option<Instant> = None;
    let mut commands_open = true;
    let finish = |event: WsEvent| {
        let _ = emit.send(event);
    };
    loop {
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => finish(WsEvent::Text { data: text.as_str().to_owned() }),
                Some(Ok(Message::Binary(data))) => finish(WsEvent::Binary { data: data.to_vec() }),
                Some(Ok(Message::Ping(data))) => finish(WsEvent::Ping { data: data.to_vec() }),
                Some(Ok(Message::Pong(data))) => finish(WsEvent::Pong { data: data.to_vec() }),
                Some(Ok(Message::Close(frame))) => {
                    finish(WsEvent::Close {
                        code: frame.as_ref().map(|f| u16::from(f.code)),
                        reason: frame.map(|f| f.reason.as_str().to_owned()).unwrap_or_default(),
                    });
                    return;
                }
                Some(Ok(Message::Frame(_))) => {}
                Some(Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed)) | None => {
                    finish(WsEvent::Close { code: None, reason: String::new() });
                    return;
                }
                Some(Err(error)) => {
                    finish(WsEvent::Error { message: error.to_string() });
                    return;
                }
            },
            command = outgoing.recv(), if commands_open => {
                let message = match command {
                    Some(WsOutgoing::Text(text)) => Message::text(text),
                    Some(WsOutgoing::Binary(data)) => Message::binary(data),
                    Some(WsOutgoing::Ping(data)) => Message::Ping(data.into()),
                    Some(WsOutgoing::Close { code, reason }) => Message::Close(Some(close_frame(code, reason))),
                    None => {
                        commands_open = false;
                        Message::Close(Some(close_frame(1000, String::new())))
                    }
                };
                let closing = matches!(message, Message::Close(_));
                if let Err(error) = sink.send(message).await {
                    finish(WsEvent::Error { message: error.to_string() });
                    return;
                }
                if closing {
                    deadline.get_or_insert(Instant::now() + CLOSE_GRACE);
                }
            }
            () = async { ticker.as_mut().unwrap().tick().await; }, if ticker.is_some() && deadline.is_none() => {
                if let Err(error) = sink.send(Message::Ping(Vec::new().into())).await {
                    finish(WsEvent::Error { message: error.to_string() });
                    return;
                }
            }
            () = async { tokio::time::sleep_until(deadline.unwrap()).await; }, if deadline.is_some() => {
                finish(WsEvent::Close { code: None, reason: String::new() });
                return;
            }
        }
    }
}
