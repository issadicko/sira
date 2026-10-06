mod common;

use common::{authority, issue, request, serve_tls};
use xc_engine::{send, ClientIdentity, EngineError, Network, Tls};

fn network(tls: Tls) -> Network {
    Network { tls, ..Network::default() }
}

fn bundle(pems: &[&str]) -> Vec<Vec<u8>> {
    pems.iter().map(|pem| pem.as_bytes().to_vec()).collect()
}

#[tokio::test]
async fn ef_req_04_a_certificate_signed_by_an_unknown_authority_is_refused_by_default() {
    let ca = authority("Sira test CA");
    let (port, server) = serve_tls(&issue(&ca, &["localhost"]), None, 1).await;

    let error = send(request(format!("https://localhost:{port}/"), Network::default())).await.unwrap_err();

    assert!(matches!(error, EngineError::Tls(_)), "{error}");
    assert!(!server.await.unwrap()[0].handshake_ok);
}

#[tokio::test]
async fn ef_req_04_turning_verification_off_accepts_an_unknown_authority_and_a_wrong_hostname() {
    let ca = authority("Sira test CA");
    let (port, server) = serve_tls(&issue(&ca, &["autre.test"]), None, 1).await;
    let tls = Tls { verify: false, ..Tls::default() };

    let res = send(request(format!("https://localhost:{port}/"), network(tls))).await.unwrap();

    assert_eq!((res.status, res.body.as_slice()), (200, b"ok".as_slice()));
    assert!(server.await.unwrap()[0].handshake_ok);
}

#[tokio::test]
async fn ef_req_04_a_custom_authority_makes_its_certificates_trusted() {
    let ca = authority("Sira test CA");
    let (port, server) = serve_tls(&issue(&ca, &["localhost"]), None, 1).await;
    let tls = Tls { extra_roots: bundle(&[&ca.cert_pem]), ..Tls::default() };

    let res = send(request(format!("https://localhost:{port}/"), network(tls))).await.unwrap();

    assert_eq!(res.status, 200);
    assert!(server.await.unwrap()[0].handshake_ok);
}

#[tokio::test]
async fn ef_req_04_a_custom_authority_does_not_excuse_a_wrong_hostname() {
    let ca = authority("Sira test CA");
    let (port, _server) = serve_tls(&issue(&ca, &["autre.test"]), None, 1).await;
    let tls = Tls { extra_roots: bundle(&[&ca.cert_pem]), ..Tls::default() };

    let error = send(request(format!("https://localhost:{port}/"), network(tls))).await.unwrap_err();

    assert!(matches!(error, EngineError::Tls(_)), "{error}");
}

#[tokio::test]
async fn ef_req_04_a_pem_bundle_trusts_every_authority_it_holds() {
    let (first, second) = (authority("première"), authority("seconde"));
    let (port, _server) = serve_tls(&issue(&second, &["localhost"]), None, 1).await;
    let tls = Tls { extra_roots: bundle(&[&format!("{}\n{}", first.cert_pem, second.cert_pem)]), ..Tls::default() };

    let res = send(request(format!("https://localhost:{port}/"), network(tls))).await.unwrap();

    assert_eq!(res.status, 200);
}

#[tokio::test]
async fn ef_req_04_without_the_default_roots_only_the_custom_authority_is_trusted() {
    let (trusted, other) = (authority("seule"), authority("autre"));
    let (port, _server) = serve_tls(&issue(&other, &["localhost"]), None, 1).await;
    let tls = Tls { extra_roots: bundle(&[&trusted.cert_pem]), keep_default_roots: false, ..Tls::default() };

    let error = send(request(format!("https://localhost:{port}/"), network(tls))).await.unwrap_err();

    assert!(matches!(error, EngineError::Tls(_)), "{error}");
}

#[tokio::test]
async fn ef_req_04_an_unreadable_authority_is_reported_not_ignored() {
    let ca = authority("Sira test CA");
    let (port, _server) = serve_tls(&issue(&ca, &["localhost"]), None, 1).await;
    let tls = Tls {
        extra_roots: bundle(&["-----BEGIN CERTIFICATE-----\n!!!\n-----END CERTIFICATE-----\n"]),
        ..Tls::default()
    };

    let error = send(request(format!("https://localhost:{port}/"), network(tls))).await.unwrap_err();

    assert!(error.to_string().contains("autorité de certification"), "{error}");
}

#[tokio::test]
async fn ef_req_04_a_client_certificate_is_presented_to_a_server_that_asks_for_it() {
    let ca = authority("Sira test CA");
    let server_identity = issue(&ca, &["localhost"]);
    let client = issue(&ca, &["client.test"]);
    let (port, server) = serve_tls(&server_identity, Some(&ca), 1).await;
    let tls = Tls {
        extra_roots: bundle(&[&ca.cert_pem]),
        client: Some(ClientIdentity { cert_pem: client.cert_pem.into(), key_pem: client.key_pem.into() }),
        ..Tls::default()
    };

    let res = send(request(format!("https://localhost:{port}/"), network(tls))).await.unwrap();

    assert_eq!(res.status, 200);
    let seen = server.await.unwrap();
    assert!(seen[0].handshake_ok && seen[0].client_certificate, "{seen:?}");
}

#[tokio::test]
async fn ef_req_04_without_a_client_certificate_a_server_that_requires_one_refuses_the_exchange() {
    let ca = authority("Sira test CA");
    let (port, _server) = serve_tls(&issue(&ca, &["localhost"]), Some(&ca), 1).await;
    let tls = Tls { extra_roots: bundle(&[&ca.cert_pem]), ..Tls::default() };

    let outcome = send(request(format!("https://localhost:{port}/"), network(tls))).await;

    assert!(outcome.is_err(), "le serveur exige un certificat client : {outcome:?}");
}

#[tokio::test]
async fn ef_req_04_an_unreadable_client_key_is_reported() {
    let ca = authority("Sira test CA");
    let client = issue(&ca, &["client.test"]);
    let (port, _server) = serve_tls(&issue(&ca, &["localhost"]), None, 1).await;
    let tls = Tls {
        extra_roots: bundle(&[&ca.cert_pem]),
        client: Some(ClientIdentity { cert_pem: client.cert_pem.into(), key_pem: "pas une clé".as_bytes().to_vec() }),
        ..Tls::default()
    };

    let error = send(request(format!("https://localhost:{port}/"), network(tls))).await.unwrap_err();

    assert!(error.to_string().contains("clé privée"), "{error}");
}
