pub mod aws;
mod cookies;
pub mod digest;
mod network;
mod proxy;
mod redirect;
mod time;
mod tls;

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Body as _;
use hyper::Request;
use hyper_util::rt::TokioIo;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use url::Url;

use proxy::Stream;
use redirect::Hop;

pub use cookies::{CookieDraft, CookieJar, CookieView, Cookies, ScriptCookie};
pub use network::{ClientIdentity, Network, Proxy, ProxyScheme, Redirects, Tls};
pub use time::{amz_date, iso_from_millis, millis};

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    pub timeout: Duration,
    /// Taille maximale, en octets, du corps de la réponse ; `None` ne la borne pas.
    pub max_response_body: Option<u64>,
    pub network: Network,
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
    /// Les durées ; avec des redirections, la somme de celles de chaque saut.
    pub timings: Timings,
    /// L'adresse de la réponse : celle du dernier saut.
    pub url: String,
    /// Les redirections suivies pour y arriver : l'adresse demandée et le statut reçu, dans l'ordre.
    pub redirects: Vec<RedirectStep>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RedirectStep {
    pub url: String,
    pub status: u16,
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
    #[error("réponse trop volumineuse : plus de {0} octets")]
    ResponseTooLarge(u64),
}

const fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

impl Timings {
    fn add(&mut self, other: &Timings) {
        self.dns_ms += other.dns_ms;
        self.tcp_ms += other.tcp_ms;
        self.tls_ms += other.tls_ms;
        self.ttfb_ms += other.ttfb_ms;
        self.download_ms += other.download_ms;
        self.total_ms += other.total_ms;
    }
}

/// Envoie la requête et suit les redirections que `request.network` demande. Le délai s'applique à chaque saut.
pub async fn send(request: HttpRequest) -> Result<HttpResponse, EngineError> {
    let url = Url::parse(&request.url).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    let mut hop =
        Hop { method: request.method.clone(), url, headers: request.headers.clone(), body: request.body.clone() };
    let (mut timings, mut steps) = (Timings::default(), Vec::new());
    let cookies = request.network.cookies.clone();
    loop {
        let uses_tls = hop.url.scheme() == "https"
            || request.network.proxy.as_ref().is_some_and(|proxy| proxy.scheme == ProxyScheme::Https);
        if uses_tls {
            tls::warm_up().await;
        }
        let jar_line = cookies.as_ref().filter(|c| c.send).and_then(|c| c.jar.header(&hop.url));
        let sent = HttpRequest {
            method: hop.method.clone(),
            url: hop.url.to_string(),
            headers: cookies::with_jar_cookies(&hop.headers, jar_line.as_deref()),
            body: hop.body.clone(),
            ..request.clone()
        };
        let timeout = request.timeout;
        let mut response = tokio::time::timeout(timeout, send_hop(sent))
            .await
            .map_err(|_| EngineError::Timeout(timeout.as_millis()))??;
        timings.add(&response.timings);
        if let Some(cookies) = cookies.as_ref().filter(|c| c.store) {
            let received: Vec<&str> = response
                .headers
                .iter()
                .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
                .map(|(_, value)| value.as_str())
                .collect();
            cookies.jar.store(&hop.url, &received);
        }
        match redirect::next(&hop, &response, &request.network.redirects, steps.len() as u32) {
            Some(next) => {
                steps.push(RedirectStep { url: hop.url.to_string(), status: response.status });
                hop = next;
            }
            None => {
                response.timings = timings;
                response.url = hop.url.to_string();
                response.redirects = steps;
                return Ok(response);
            }
        }
    }
}

async fn send_hop(request: HttpRequest) -> Result<HttpResponse, EngineError> {
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

    let proxy = request.network.proxy.clone().filter(|proxy| proxy.applies_to(&url));
    let proxy = proxy.as_ref();
    let (dial_host, dial_port) = proxy.map_or((host.as_str(), port), |proxy| (proxy.host.as_str(), proxy.port));
    let (addrs, dns) = resolve(dial_host, dial_port).await?;

    let t = Instant::now();
    let (tcp_stream, addr) = connect(&addrs).await?;
    let mut stream: Stream = Box::new(tcp_stream);
    if let Some(proxy) = proxy {
        if proxy.scheme == ProxyScheme::Https {
            let own = Tls { client: None, ..request.network.tls.clone() };
            stream = Box::new(tls::handshake(stream, &proxy.host, &own).await?);
        }
        if secure || proxy.is_socks() {
            proxy::tunnel(&mut stream, proxy, &host, port).await?;
        }
    }
    let tcp = t.elapsed();

    let forward = proxy.filter(|proxy| !secure && !proxy.is_socks());
    let mut timings = Timings { dns_ms: ms(dns), tcp_ms: ms(tcp), ..Timings::default() };
    let response = if secure {
        let t = Instant::now();
        let tls = tls::handshake(stream, &host, &request.network.tls).await?;
        timings.tls_ms = ms(t.elapsed());
        exchange(tls, &url, forward, request, &mut timings).await?
    } else {
        exchange(stream, &url, forward, request, &mut timings).await?
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
    forward: Option<&Proxy>,
    request: HttpRequest,
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
    let target = match (forward, url.query()) {
        (Some(_), _) => url.as_str().to_owned(),
        (None, Some(q)) => format!("{}?{}", url.path(), q),
        (None, None) => url.path().to_owned(),
    };
    let mut builder = Request::builder().method(method).uri(target);
    if let Some(authorization) = forward.and_then(Proxy::authorization) {
        builder = builder.header("proxy-authorization", authorization);
    }
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
    let body = Full::new(Bytes::from(request.body.unwrap_or_default()));
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
    let body = read_body(response.into_body(), request.max_response_body).await?;
    timings.download_ms = ms(t.elapsed());

    Ok(HttpResponse {
        status: status.as_u16(),
        reason: status.canonical_reason().unwrap_or_default().to_owned(),
        http_version,
        remote_addr: String::new(),
        headers,
        body,
        timings: timings.clone(),
        url: String::new(),
        redirects: Vec::new(),
    })
}

/// Texte d'un corps : l'UTF-8 valide est repris sans copie, sinon les octets invalides sont remplacés.
pub fn lossy_text(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

const PREALLOCATED_MAX: u64 = 64 << 20;

/// Lit le corps morceau par morceau, en refusant dès que `Content-Length` ou les octets reçus dépassent `max`.
async fn read_body(mut body: hyper::body::Incoming, max: Option<u64>) -> Result<Vec<u8>, EngineError> {
    let too_large = |size: u64| max.filter(|max| size > *max).map(EngineError::ResponseTooLarge);
    let announced = body.size_hint().lower();
    if let Some(error) = too_large(announced) {
        return Err(error);
    }
    let mut bytes = Vec::with_capacity(usize::try_from(announced.min(PREALLOCATED_MAX)).unwrap_or(0));
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(|e| EngineError::Http(e.to_string()))?;
        let Ok(data) = frame.into_data() else { continue };
        if let Some(error) = too_large((bytes.len() + data.len()) as u64) {
            return Err(error);
        }
        bytes.extend_from_slice(&data);
    }
    Ok(bytes)
}
