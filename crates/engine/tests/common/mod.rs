#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;
use xc_engine::{HttpRequest, Network};

pub const OK: &str = "HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok";

pub fn request(url: String, network: Network) -> HttpRequest {
    HttpRequest {
        method: "GET".into(),
        url,
        headers: vec![],
        body: None,
        timeout: Duration::from_secs(5),
        max_response_body: None,
        network,
    }
}

pub struct Authority {
    pub cert_pem: String,
    issuer: Issuer<'static, KeyPair>,
}

pub struct Identity {
    pub cert_pem: String,
    pub key_pem: String,
}

pub fn authority(name: &str) -> Authority {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.distinguished_name.push(DnType::CommonName, name);
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let key = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    Authority { cert_pem: cert.pem(), issuer: Issuer::new(params, key) }
}

pub fn issue(authority: &Authority, names: &[&str]) -> Identity {
    let params = CertificateParams::new(names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>()).unwrap();
    let key = KeyPair::generate().unwrap();
    let cert = params.signed_by(&key, &authority.issuer).unwrap();
    Identity { cert_pem: cert.pem(), key_pem: key.serialize_pem() }
}

fn der(pem: &str) -> Vec<CertificateDer<'static>> {
    CertificateDer::pem_slice_iter(pem.as_bytes()).collect::<Result<_, _>>().unwrap()
}

/// Ce qu'un serveur TLS a vu d'une connexion : l'échange a-t-il abouti, et le client a-t-il présenté un certificat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub handshake_ok: bool,
    pub client_certificate: bool,
    pub request: String,
}

/// Un serveur HTTPS sur 127.0.0.1 qui traite `connections` connexions ; `client_ca` exige un certificat client.
pub async fn serve_tls(
    identity: &Identity,
    client_ca: Option<&Authority>,
    connections: usize,
) -> (u16, JoinHandle<Vec<Seen>>) {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let builder = ServerConfig::builder_with_provider(provider).with_safe_default_protocol_versions().unwrap();
    let builder = match client_ca {
        None => builder.with_no_client_auth(),
        Some(ca) => {
            let mut roots = RootCertStore::empty();
            for cert in der(&ca.cert_pem) {
                roots.add(cert).unwrap();
            }
            builder.with_client_cert_verifier(WebPkiClientVerifier::builder(Arc::new(roots)).build().unwrap())
        }
    };
    let key = PrivateKeyDer::from_pem_slice(identity.key_pem.as_bytes()).unwrap();
    let config = builder.with_single_cert(der(&identity.cert_pem), key).unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = tokio::spawn(async move {
        let mut seen = Vec::new();
        for _ in 0..connections {
            let (socket, _) = listener.accept().await.unwrap();
            let Ok(mut stream) = acceptor.accept(socket).await else {
                seen.push(Seen { handshake_ok: false, client_certificate: false, request: String::new() });
                continue;
            };
            let client_certificate = stream.get_ref().1.peer_certificates().is_some();
            let mut buf = vec![0; 8192];
            let Ok(n) = stream.read(&mut buf).await else {
                seen.push(Seen { handshake_ok: false, client_certificate, request: String::new() });
                continue;
            };
            let request = String::from_utf8_lossy(&buf[..n]).into_owned();
            stream.write_all(OK.as_bytes()).await.ok();
            stream.shutdown().await.ok();
            seen.push(Seen { handshake_ok: true, client_certificate, request });
        }
        seen
    });
    (port, handle)
}
