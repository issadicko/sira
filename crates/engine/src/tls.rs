use std::sync::{Arc, OnceLock};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use crate::EngineError;

fn config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            for cert in rustls_native_certs::load_native_certs().certs {
                roots.add(cert).ok();
            }
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            let config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("versions TLS par défaut")
                .with_root_certificates(roots)
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

pub async fn handshake(stream: TcpStream, host: &str) -> Result<TlsStream<TcpStream>, EngineError> {
    let name = ServerName::try_from(host.to_owned()).map_err(|e| EngineError::Tls(e.to_string()))?;
    TlsConnector::from(config()).connect(name, stream).await.map_err(|e| EngineError::Tls(e.to_string()))
}
