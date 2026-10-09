//! Client gRPC : la connexion (DNS, proxy, TLS avec ALPN `h2`) est celle des requêtes HTTP, le transport est HTTP/2
//! (`h2`). Le moteur ne connaît pas protobuf : il envoie et reçoit des messages déjà encodés. Deux tâches se partagent
//! le flux : l'une écrit les messages de l'appelant, l'autre lit les en-têtes, les messages et le statut final.

use std::future::poll_fn;
use std::time::Duration;

use bytes::{Buf, Bytes, BytesMut};
use h2::{Reason, RecvStream, SendStream};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio::time::Instant;
use url::Url;

use crate::{dial_alpn, tls, EngineError, Network, ProxyScheme, Timings};

/// Taille maximale d'un message reçu : celle de la plupart des clients gRPC (4 Mio) serait trop juste pour un outil
/// de test, on garde la limite du WebSocket.
const MAX_MESSAGE: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct GrpcRequest {
    /// `grpc://`, `grpcs://`, `http://`, `https://` ; sans schéma, `grpc://` (en clair), comme Bruno.
    pub url: String,
    /// `paquet.Service/Méthode`, avec ou sans barre oblique initiale.
    pub method: String,
    pub metadata: Vec<(String, String)>,
    /// Délai pour joindre le serveur et ouvrir le flux.
    pub timeout: Duration,
    /// Durée maximale de l'appel entier, annoncée au serveur (`grpc-timeout`) et appliquée ici ; `None` : aucune.
    pub deadline: Option<Duration>,
    pub network: Network,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GrpcOpened {
    pub remote_addr: String,
    pub url: String,
    pub timings: Timings,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GrpcEvent {
    /// Les en-têtes de la réponse (le serveur peut ne les envoyer qu'avec le premier message).
    Headers { headers: Vec<(String, String)> },
    /// Un message reçu, encore encodé.
    Message { data: Vec<u8> },
    /// La fin de l'appel : code gRPC (0 : OK), message et métadonnées finales. Toujours le dernier événement, sauf erreur.
    Status { code: u32, message: String, metadata: Vec<(String, String)> },
    /// Le transport a cassé ; rien ne suivra.
    Error { message: String },
}

enum Command {
    Send(Vec<u8>),
    Finish,
    Cancel,
}

#[derive(Debug, Clone)]
pub struct GrpcSender(mpsc::UnboundedSender<Command>);

impl GrpcSender {
    pub fn send(&self, message: Vec<u8>) -> Result<(), EngineError> {
        self.0.send(Command::Send(message)).map_err(|_| closed())
    }

    /// Annonce qu'aucun autre message ne partira (demi-fermeture).
    pub fn finish(&self) -> Result<(), EngineError> {
        self.0.send(Command::Finish).map_err(|_| closed())
    }

    /// Abandonne l'appel : le serveur reçoit une réinitialisation du flux, le statut final est `CANCELLED` (1).
    pub fn cancel(&self) {
        self.0.send(Command::Cancel).ok();
    }
}

fn closed() -> EngineError {
    EngineError::Grpc("l'appel est terminé".into())
}

#[derive(Debug)]
pub struct GrpcCall {
    pub opened: GrpcOpened,
    pub sender: GrpcSender,
    pub events: mpsc::UnboundedReceiver<GrpcEvent>,
}

fn target(url: &str) -> Result<(Url, bool), EngineError> {
    let text = if url.contains("://") { url.to_owned() } else { format!("grpc://{url}") };
    let parsed = Url::parse(&text).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    let secure = match parsed.scheme() {
        "grpc" | "http" => false,
        "grpcs" | "https" => true,
        other => return Err(EngineError::UnsupportedScheme(other.to_owned())),
    };
    let scheme = if secure { "https" } else { "http" };
    let host = parsed.host_str().ok_or_else(|| EngineError::InvalidUrl("hôte manquant".into()))?;
    let port = parsed.port().map(|port| format!(":{port}")).unwrap_or_default();
    let url = Url::parse(&format!("{scheme}://{host}{port}/")).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    Ok((url, secure))
}

fn authority(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    url.port().map_or_else(|| host.to_owned(), |port| format!("{host}:{port}"))
}

fn transport(error: h2::Error) -> EngineError {
    EngineError::Grpc(format!("transport HTTP/2 : {error}"))
}

fn timeout_header(deadline: Duration) -> String {
    format!("{}m", deadline.as_millis().max(1))
}

pub async fn connect_grpc(request: GrpcRequest) -> Result<GrpcCall, EngineError> {
    let (url, secure) = target(&request.url)?;
    if secure || request.network.proxy.as_ref().is_some_and(|proxy| proxy.scheme == ProxyScheme::Https) {
        tls::warm_up().await;
    }
    let path = format!("/{}", request.method.trim_start_matches('/'));
    if path.len() < 4 || !path[1..].contains('/') {
        return Err(EngineError::Grpc(format!(
            "méthode invalide (attendu paquet.Service/Méthode) : {}",
            request.method
        )));
    }
    let uri = format!("{}://{}{path}", url.scheme(), authority(&url));
    let mut builder = http::Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .header("user-agent", concat!("sira-grpc/", env!("CARGO_PKG_VERSION")))
        .header("grpc-accept-encoding", "identity");
    if let Some(deadline) = request.deadline {
        builder = builder.header("grpc-timeout", timeout_header(deadline));
    }
    for (name, value) in &request.metadata {
        let name = name.to_ascii_lowercase();
        if matches!(name.as_str(), "content-type" | "te" | "host" | ":authority") {
            continue;
        }
        builder = builder.header(name, value);
    }
    let call = builder.body(()).map_err(|e| EngineError::Grpc(format!("métadonnée invalide : {e}")))?;

    let timeout = request.timeout;
    let (response, send, addr, timings) = tokio::time::timeout(timeout, async {
        let started = Instant::now();
        let dialed = dial_alpn(&url, secure, true, &request.network, &[b"h2"]).await?;
        let (client, connection) = h2::client::handshake(dialed.stream).await.map_err(transport)?;
        tokio::spawn(async move {
            connection.await.ok();
        });
        let mut client = client.ready().await.map_err(transport)?;
        let (response, send) = client.send_request(call, false).map_err(transport)?;
        let mut timings = dialed.timings;
        timings.total_ms = started.elapsed().as_secs_f64() * 1000.0;
        Ok::<_, EngineError>((response, send, dialed.addr, timings))
    })
    .await
    .map_err(|_| EngineError::Timeout(timeout.as_millis()))??;

    let (commands, outgoing) = mpsc::unbounded_channel();
    let (emit, events) = mpsc::unbounded_channel();
    tokio::spawn(write_loop(send, outgoing, emit.clone()));
    let deadline = request.deadline;
    tokio::spawn(async move {
        let read = read_loop(response, &emit);
        match deadline {
            Some(limit) => {
                if tokio::time::timeout(limit, read).await.is_err() {
                    emit.send(GrpcEvent::Status { code: 4, message: "Deadline Exceeded".into(), metadata: Vec::new() })
                        .ok();
                }
            }
            None => read.await,
        }
    });
    let opened = GrpcOpened { remote_addr: addr.to_string(), url: url.to_string(), timings };
    Ok(GrpcCall { opened, sender: GrpcSender(commands), events })
}

fn frame(message: &[u8]) -> Bytes {
    let mut out = BytesMut::with_capacity(5 + message.len());
    out.extend_from_slice(&[0]);
    out.extend_from_slice(&u32::try_from(message.len()).unwrap_or(u32::MAX).to_be_bytes());
    out.extend_from_slice(message);
    out.freeze()
}

async fn write_all(stream: &mut SendStream<Bytes>, mut data: Bytes) -> Result<(), h2::Error> {
    while !data.is_empty() {
        stream.reserve_capacity(data.len());
        let capacity = match poll_fn(|cx| stream.poll_capacity(cx)).await {
            Some(capacity) => capacity?,
            None => return Err(h2::Error::from(Reason::CANCEL)),
        };
        if capacity == 0 {
            continue;
        }
        let chunk = data.split_to(capacity.min(data.len()));
        stream.send_data(chunk, false)?;
    }
    Ok(())
}

async fn write_loop(
    mut stream: SendStream<Bytes>,
    mut commands: mpsc::UnboundedReceiver<Command>,
    emit: mpsc::UnboundedSender<GrpcEvent>,
) {
    loop {
        match commands.recv().await {
            Some(Command::Send(message)) => {
                if let Err(error) = write_all(&mut stream, frame(&message)).await {
                    if error.reason() != Some(Reason::CANCEL) {
                        emit.send(GrpcEvent::Error { message: format!("écriture : {error}") }).ok();
                    }
                    return;
                }
            }
            Some(Command::Finish) => {
                stream.send_data(Bytes::new(), true).ok();
                return;
            }
            Some(Command::Cancel) | None => {
                stream.send_reset(Reason::CANCEL);
                return;
            }
        }
    }
}

fn pairs(headers: &http::HeaderMap) -> Vec<(String, String)> {
    headers.iter().map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned())).collect()
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn status_of(headers: &http::HeaderMap) -> Option<GrpcEvent> {
    let code = headers.get("grpc-status")?.to_str().ok()?.trim().parse::<u32>().ok()?;
    let message = headers.get("grpc-message").and_then(|v| v.to_str().ok()).map(percent_decode).unwrap_or_default();
    let metadata =
        pairs(headers).into_iter().filter(|(name, _)| name != "grpc-status" && name != "grpc-message").collect();
    Some(GrpcEvent::Status { code, message, metadata })
}

fn failure(error: &h2::Error) -> GrpcEvent {
    if error.reason() == Some(Reason::CANCEL) && error.is_reset() {
        return GrpcEvent::Status { code: 1, message: "Cancelled".into(), metadata: Vec::new() };
    }
    GrpcEvent::Error { message: format!("transport HTTP/2 : {error}") }
}

fn drain(buffer: &mut BytesMut, emit: &mpsc::UnboundedSender<GrpcEvent>) -> Result<(), String> {
    while buffer.len() >= 5 {
        let compressed = buffer[0] != 0;
        let length = u32::from_be_bytes([buffer[1], buffer[2], buffer[3], buffer[4]]) as usize;
        if length > MAX_MESSAGE {
            return Err(format!("message reçu trop gros ({length} octets)"));
        }
        if buffer.len() < 5 + length {
            return Ok(());
        }
        buffer.advance(5);
        let data = buffer.split_to(length);
        if compressed {
            return Err("message compressé : la compression gRPC n'est pas prise en charge".into());
        }
        emit.send(GrpcEvent::Message { data: data.to_vec() }).ok();
    }
    Ok(())
}

async fn read_loop(response: h2::client::ResponseFuture, emit: &mpsc::UnboundedSender<GrpcEvent>) {
    let response = match response.await {
        Ok(response) => response,
        Err(error) => {
            emit.send(failure(&error)).ok();
            return;
        }
    };
    let (parts, mut body): (_, RecvStream) = response.into_parts();
    let headers = pairs(&parts.headers);
    emit.send(GrpcEvent::Headers { headers }).ok();
    if let Some(status) = status_of(&parts.headers) {
        emit.send(status).ok();
        return;
    }
    let content_type = parts.headers.get("content-type").and_then(|v| v.to_str().ok()).unwrap_or_default();
    if !parts.status.is_success() || !content_type.starts_with("application/grpc") {
        let shown = if content_type.is_empty() { "aucun" } else { content_type };
        emit.send(GrpcEvent::Error {
            message: format!(
                "le serveur ne répond pas en gRPC (HTTP {}, content-type : {shown})",
                parts.status.as_u16()
            ),
        })
        .ok();
        return;
    }
    let mut buffer = BytesMut::new();
    while let Some(chunk) = body.data().await {
        match chunk {
            Ok(bytes) => {
                body.flow_control().release_capacity(bytes.len()).ok();
                buffer.extend_from_slice(&bytes);
                if let Err(message) = drain(&mut buffer, emit) {
                    emit.send(GrpcEvent::Error { message }).ok();
                    return;
                }
            }
            Err(error) => {
                emit.send(failure(&error)).ok();
                return;
            }
        }
    }
    match body.trailers().await {
        Ok(Some(trailers)) => match status_of(&trailers) {
            Some(status) => emit.send(status).ok(),
            None => emit.send(GrpcEvent::Error { message: "réponse terminée sans grpc-status".into() }).ok(),
        },
        Ok(None) => emit.send(GrpcEvent::Error { message: "réponse terminée sans statut gRPC".into() }).ok(),
        Err(error) => emit.send(failure(&error)).ok(),
    };
}
