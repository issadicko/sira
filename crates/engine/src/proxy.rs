//! Joindre un hôte à travers un proxy : tunnel `CONNECT` (HTTP, HTTPS) ou SOCKS (4a, 5).

use std::net::IpAddr;

use base64::Engine as _;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::network::{Proxy, ProxyScheme};
use crate::EngineError;

pub(crate) trait Conn: AsyncRead + AsyncWrite + Unpin + Send {}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> Conn for T {}

pub(crate) type Stream = Box<dyn Conn>;

const HEAD_MAX: usize = 16 * 1024;

impl Proxy {
    pub(crate) const fn is_socks(&self) -> bool {
        matches!(self.scheme, ProxyScheme::Socks4 | ProxyScheme::Socks5)
    }

    /// L'en-tête `Proxy-Authorization` d'un proxy HTTP qui demande des identifiants.
    pub(crate) fn authorization(&self) -> Option<String> {
        self.auth.as_ref().map(|(user, password)| {
            format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}")))
        })
    }

    fn refused(&self, reason: impl std::fmt::Display) -> EngineError {
        EngineError::Connect { addr: format!("{}:{}", self.host, self.port), reason: format!("proxy : {reason}") }
    }
}

/// Fait ouvrir par le proxy une connexion vers `host:port` ; ensuite `stream` va jusqu'à l'hôte.
pub(crate) async fn tunnel(stream: &mut Stream, proxy: &Proxy, host: &str, port: u16) -> Result<(), EngineError> {
    match proxy.scheme {
        ProxyScheme::Http | ProxyScheme::Https => connect(stream, proxy, host, port).await,
        ProxyScheme::Socks5 => socks5(stream, proxy, host, port).await,
        ProxyScheme::Socks4 => socks4a(stream, proxy, host, port).await,
    }
}

fn authority(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

async fn connect(stream: &mut Stream, proxy: &Proxy, host: &str, port: u16) -> Result<(), EngineError> {
    let authority = authority(host, port);
    let mut head = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some(authorization) = proxy.authorization() {
        head.push_str(&format!("Proxy-Authorization: {authorization}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await.map_err(|e| proxy.refused(e))?;

    let mut reply = Vec::new();
    let mut byte = [0u8; 1];
    while !reply.ends_with(b"\r\n\r\n") {
        if reply.len() >= HEAD_MAX {
            return Err(proxy.refused("réponse au tunnel trop longue"));
        }
        let read = stream.read(&mut byte).await.map_err(|e| proxy.refused(e))?;
        if read == 0 {
            return Err(proxy.refused("connexion fermée avant la réponse au tunnel"));
        }
        reply.push(byte[0]);
    }
    let reply = String::from_utf8_lossy(&reply);
    let status_line = reply.lines().next().unwrap_or_default();
    let code = status_line.split_whitespace().nth(1).and_then(|code| code.parse::<u16>().ok());
    match code {
        Some(200..=299) => Ok(()),
        _ => Err(proxy.refused(format!("tunnel refusé ({status_line})"))),
    }
}

async fn socks5(stream: &mut Stream, proxy: &Proxy, host: &str, port: u16) -> Result<(), EngineError> {
    let io = |e: std::io::Error| proxy.refused(e);
    let greeting: &[u8] = if proxy.auth.is_some() { &[5, 2, 0, 2] } else { &[5, 1, 0] };
    stream.write_all(greeting).await.map_err(io)?;
    let mut choice = [0u8; 2];
    stream.read_exact(&mut choice).await.map_err(io)?;
    match choice {
        [5, 0] => {}
        [5, 2] => authenticate(stream, proxy).await?,
        [5, 0xFF] => return Err(proxy.refused("SOCKS5 : aucune méthode d'authentification acceptée")),
        _ => return Err(proxy.refused("SOCKS5 : réponse inattendue")),
    }

    let mut request = vec![5, 1, 0];
    match host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            request.push(1);
            request.extend_from_slice(&ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            request.push(4);
            request.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            let name = u8::try_from(host.len()).map_err(|_| proxy.refused("SOCKS5 : nom d'hôte trop long"))?;
            request.extend_from_slice(&[3, name]);
            request.extend_from_slice(host.as_bytes());
        }
    }
    request.extend_from_slice(&port.to_be_bytes());
    stream.write_all(&request).await.map_err(io)?;

    let mut head = [0u8; 4];
    stream.read_exact(&mut head).await.map_err(io)?;
    if head[1] != 0 {
        return Err(proxy.refused(format!("SOCKS5 : connexion refusée ({})", socks5_reason(head[1]))));
    }
    let bound = match head[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut len = [0u8; 1];
            stream.read_exact(&mut len).await.map_err(io)?;
            usize::from(len[0])
        }
        _ => return Err(proxy.refused("SOCKS5 : adresse de réponse inconnue")),
    };
    let mut rest = vec![0u8; bound + 2];
    stream.read_exact(&mut rest).await.map_err(io)?;
    Ok(())
}

async fn authenticate(stream: &mut Stream, proxy: &Proxy) -> Result<(), EngineError> {
    let io = |e: std::io::Error| proxy.refused(e);
    let (user, password) = proxy.auth.as_ref().ok_or_else(|| proxy.refused("SOCKS5 : identifiants demandés"))?;
    let (user, password) = (user.as_bytes(), password.as_bytes());
    let (Ok(user_len), Ok(password_len)) = (u8::try_from(user.len()), u8::try_from(password.len())) else {
        return Err(proxy.refused("SOCKS5 : identifiants trop longs"));
    };
    let mut request = vec![1, user_len];
    request.extend_from_slice(user);
    request.push(password_len);
    request.extend_from_slice(password);
    stream.write_all(&request).await.map_err(io)?;
    let mut reply = [0u8; 2];
    stream.read_exact(&mut reply).await.map_err(io)?;
    if reply[1] != 0 {
        return Err(proxy.refused("SOCKS5 : identifiants refusés"));
    }
    Ok(())
}

const fn socks5_reason(code: u8) -> &'static str {
    match code {
        1 => "échec général",
        2 => "interdit par les règles du proxy",
        3 => "réseau inaccessible",
        4 => "hôte inaccessible",
        5 => "connexion refusée par l'hôte",
        6 => "durée de vie dépassée",
        7 => "commande non prise en charge",
        8 => "type d'adresse non pris en charge",
        _ => "raison inconnue",
    }
}

async fn socks4a(stream: &mut Stream, proxy: &Proxy, host: &str, port: u16) -> Result<(), EngineError> {
    let io = |e: std::io::Error| proxy.refused(e);
    let mut request = vec![4, 1];
    request.extend_from_slice(&port.to_be_bytes());
    let ip = host.parse::<std::net::Ipv4Addr>().ok();
    match ip {
        Some(ip) => request.extend_from_slice(&ip.octets()),
        None => request.extend_from_slice(&[0, 0, 0, 1]),
    }
    if let Some((user, _)) = &proxy.auth {
        request.extend_from_slice(user.as_bytes());
    }
    request.push(0);
    if ip.is_none() {
        request.extend_from_slice(host.as_bytes());
        request.push(0);
    }
    stream.write_all(&request).await.map_err(io)?;
    let mut reply = [0u8; 8];
    stream.read_exact(&mut reply).await.map_err(io)?;
    if reply[1] != 0x5A {
        return Err(proxy.refused(format!("SOCKS4 : connexion refusée (code {})", reply[1])));
    }
    Ok(())
}
