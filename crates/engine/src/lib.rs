mod tls;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::Request;
use hyper_util::rt::TokioIo;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use url::Url;

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Timings {
    pub dns_ms: f64,
    pub tcp_ms: f64,
    pub tls_ms: f64,
    pub ttfb_ms: f64,
    pub download_ms: f64,
    pub total_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HttpResponse {
    pub status: u16,
    pub reason: String,
    pub http_version: String,
    pub remote_addr: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timings: Timings,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("URL invalide : {0}")]
    InvalidUrl(String),
    #[error("schéma non pris en charge : {0}")]
    UnsupportedScheme(String),
    #[error("méthode HTTP invalide : {0}")]
    InvalidMethod(String),
    #[error("résolution DNS impossible pour {host} : {reason}")]
    Dns { host: String, reason: String },
    #[error("connexion impossible à {addr} : {reason}")]
    Connect { addr: String, reason: String },
    #[error("négociation TLS échouée : {0}")]
    Tls(String),
    #[error("échange HTTP échoué : {0}")]
    Http(String),
    #[error("délai dépassé après {0} ms")]
    Timeout(u128),
}

const fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

pub async fn send(request: &HttpRequest) -> Result<HttpResponse, EngineError> {
    tokio::time::timeout(request.timeout, send_inner(request))
        .await
        .map_err(|_| EngineError::Timeout(request.timeout.as_millis()))?
}

async fn send_inner(request: &HttpRequest) -> Result<HttpResponse, EngineError> {
    let start = Instant::now();
    let url = Url::parse(&request.url).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    let secure = match url.scheme() {
        "https" => true,
        "http" => false,
        other => return Err(EngineError::UnsupportedScheme(other.to_owned())),
    };
    let host = url
        .host_str()
        .ok_or_else(|| EngineError::InvalidUrl("hôte manquant".into()))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned();
    let port = url.port_or_known_default().unwrap_or(if secure { 443 } else { 80 });

    let (addrs, dns) = resolve(&host, port).await?;

    let t = Instant::now();
    let (stream, addr) = connect(&addrs).await?;
    let tcp = t.elapsed();

    let mut timings = Timings { dns_ms: ms(dns), tcp_ms: ms(tcp), ..Timings::default() };
    let response = if secure {
        let t = Instant::now();
        let tls = tls::handshake(stream, &host).await?;
        timings.tls_ms = ms(t.elapsed());
        exchange(tls, &url, request, &mut timings).await?
    } else {
        exchange(stream, &url, request, &mut timings).await?
    };
    timings.total_ms = ms(start.elapsed());
    Ok(HttpResponse { remote_addr: addr.to_string(), timings, ..response })
}

async fn resolve(host: &str, port: u16) -> Result<(Vec<SocketAddr>, Duration), EngineError> {
    if let Ok(ip) = host.parse() {
        return Ok((vec![SocketAddr::new(ip, port)], Duration::ZERO));
    }
    let t = Instant::now();
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| EngineError::Dns { host: host.to_owned(), reason: e.to_string() })?
        .collect();
    if addrs.is_empty() {
        return Err(EngineError::Dns { host: host.to_owned(), reason: "aucune adresse".into() });
    }
    Ok((addrs, t.elapsed()))
}

async fn connect(addrs: &[SocketAddr]) -> Result<(TcpStream, SocketAddr), EngineError> {
    let mut last = String::new();
    for addr in addrs {
        match TcpStream::connect(addr).await {
            Ok(stream) => {
                stream.set_nodelay(true).ok();
                return Ok((stream, *addr));
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(EngineError::Connect { addr: addrs[0].to_string(), reason: last })
}

async fn exchange<S>(
    stream: S,
    url: &Url,
    request: &HttpRequest,
    timings: &mut Timings,
) -> Result<HttpResponse, EngineError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let http_err = |e: hyper::Error| EngineError::Http(e.to_string());
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await.map_err(http_err)?;
    tokio::spawn(connection);

    let method = hyper::Method::from_bytes(request.method.as_bytes())
        .map_err(|_| EngineError::InvalidMethod(request.method.clone()))?;
    let target = match url.query() {
        Some(q) => format!("{}?{}", url.path(), q),
        None => url.path().to_owned(),
    };
    let mut builder = Request::builder().method(method).uri(target);
    if !request.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("host")) {
        let host = match url.port() {
            Some(p) => format!("{}:{p}", url.host_str().unwrap_or_default()),
            None => url.host_str().unwrap_or_default().to_owned(),
        };
        builder = builder.header("host", host);
    }
    for (k, v) in &request.headers {
        builder = builder.header(k.as_str(), v.as_str());
    }
    let body = Full::new(Bytes::from(request.body.clone().unwrap_or_default()));
    let req = builder.body(body).map_err(|e| EngineError::Http(e.to_string()))?;

    let t = Instant::now();
    let response = sender.send_request(req).await.map_err(http_err)?;
    timings.ttfb_ms = ms(t.elapsed());

    let status = response.status();
    let http_version = format!("{:?}", response.version());
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
        .collect();

    let t = Instant::now();
    let body = response.into_body().collect().await.map_err(http_err)?.to_bytes().to_vec();
    timings.download_ms = ms(t.elapsed());

    Ok(HttpResponse {
        status: status.as_u16(),
        reason: status.canonical_reason().unwrap_or_default().to_owned(),
        http_version,
        remote_addr: String::new(),
        headers,
        body,
        timings: timings.clone(),
    })
}
