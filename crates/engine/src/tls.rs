use std::sync::{Arc, OnceLock};

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use crate::network::Tls;
use crate::EngineError;

fn provider() -> Arc<CryptoProvider> {
    static PROVIDER: OnceLock<Arc<CryptoProvider>> = OnceLock::new();
    PROVIDER.get_or_init(|| Arc::new(rustls::crypto::ring::default_provider())).clone()
}

static ROOTS: OnceLock<RootCertStore> = OnceLock::new();

/// Charge les autorités du système avant le premier envoi sécurisé : sur macOS cela peut prendre plusieurs secondes,
/// qui ne doivent pas compter dans le délai de la requête.
pub(crate) async fn warm_up() {
    if ROOTS.get().is_none() {
        tokio::task::spawn_blocking(default_roots).await.ok();
    }
}

fn default_roots() -> RootCertStore {
    ROOTS
        .get_or_init(|| {
            let mut roots = RootCertStore::empty();
            roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            for cert in rustls_native_certs::load_native_certs().certs {
                roots.add(cert).ok();
            }
            roots
        })
        .clone()
}

fn tls_error(e: impl std::fmt::Display) -> EngineError {
    EngineError::Tls(e.to_string())
}

/// Accepte tout serveur : ni la chaîne ni le nom d'hôte ne sont vérifiés (réglage « vérification TLS » désactivé).
#[derive(Debug)]
struct AcceptAny(Arc<CryptoProvider>);

impl ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.0.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn roots(tls: &Tls) -> Result<RootCertStore, EngineError> {
    let mut roots =
        if tls.keep_default_roots || tls.extra_roots.is_empty() { default_roots() } else { RootCertStore::empty() };
    for bundle in &tls.extra_roots {
        for cert in CertificateDer::pem_slice_iter(bundle) {
            let cert = cert.map_err(|e| tls_error(format!("autorité de certification illisible : {e}")))?;
            roots.add(cert).map_err(|e| tls_error(format!("autorité de certification refusée : {e}")))?;
        }
    }
    Ok(roots)
}

fn build(tls: &Tls) -> Result<ClientConfig, EngineError> {
    let builder =
        ClientConfig::builder_with_provider(provider()).with_safe_default_protocol_versions().map_err(tls_error)?;
    let builder = if tls.verify {
        builder.with_root_certificates(roots(tls)?)
    } else {
        builder.dangerous().with_custom_certificate_verifier(Arc::new(AcceptAny(provider())))
    };
    match &tls.client {
        None => Ok(builder.with_no_client_auth()),
        Some(identity) => {
            let chain = CertificateDer::pem_slice_iter(&identity.cert_pem)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| tls_error(format!("certificat client illisible : {e}")))?;
            if chain.is_empty() {
                return Err(tls_error("certificat client illisible : aucun certificat PEM trouvé"));
            }
            let key = PrivateKeyDer::from_pem_slice(&identity.key_pem)
                .map_err(|e| tls_error(format!("clé privée du certificat client illisible : {e}")))?;
            builder.with_client_auth_cert(chain, key).map_err(|e| tls_error(format!("certificat client refusé : {e}")))
        }
    }
}

fn config(tls: &Tls) -> Result<Arc<ClientConfig>, EngineError> {
    static DEFAULT: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    if *tls == Tls::default() {
        if let Some(config) = DEFAULT.get() {
            return Ok(config.clone());
        }
        let config = Arc::new(build(tls)?);
        return Ok(DEFAULT.get_or_init(|| config).clone());
    }
    build(tls).map(Arc::new)
}

pub async fn handshake<S>(stream: S, host: &str, tls: &Tls) -> Result<TlsStream<S>, EngineError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    handshake_alpn(stream, host, tls, &[]).await
}

/// Comme `handshake`, en proposant ces protocoles ALPN (`h2` pour gRPC).
pub async fn handshake_alpn<S>(stream: S, host: &str, tls: &Tls, alpn: &[&[u8]]) -> Result<TlsStream<S>, EngineError>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let name = ServerName::try_from(host.to_owned()).map_err(tls_error)?;
    let mut config = config(tls)?;
    if !alpn.is_empty() {
        let mut own = (*config).clone();
        own.alpn_protocols = alpn.iter().map(|protocol| protocol.to_vec()).collect();
        config = Arc::new(own);
    }
    TlsConnector::from(config).connect(name, stream).await.map_err(tls_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ef_req_04_warming_up_loads_the_system_roots_once_before_any_handshake() {
        warm_up().await;
        let loaded = ROOTS.get().expect("racines chargées").len();
        assert!(loaded > 0);
        warm_up().await;
        assert_eq!(ROOTS.get().map(RootCertStore::len), Some(loaded));
        assert!(config(&Tls::default()).is_ok());
    }
}
