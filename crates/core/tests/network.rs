use std::collections::HashMap;
use std::fs;
use std::path::Path;

use xc_core::{prepare_with, read_request, NetworkPrefs, Overrides, ProxyMode, ProxyPref};
use xc_engine::ProxyScheme;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn collection(config: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "opencollection.yml",
        &format!("opencollection: 1.0.0\n\ninfo:\n  name: Réseau\n\nrequest:\n  variables:\n    - name: host\n      value: api.test\n\nconfig:\n{config}"),
    );
    write(
        dir.path(),
        "r.yml",
        "info:\n  name: R\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"https://{{host}}/v1\"\n",
    );
    dir
}

fn prepared(dir: &tempfile::TempDir, network: Option<NetworkPrefs>) -> Result<xc_core::Prepared, xc_core::CoreError> {
    let doc = read_request(dir.path(), "r.yml").unwrap();
    prepare_with(dir.path(), "r.yml", &doc, None, &HashMap::new(), Overrides { network, ..Overrides::default() })
}

const PROXY: &str =
    "  proxy:\n    inherit: false\n    config:\n      protocol: socks5\n      hostname: corp.test\n      port: 1081\n";

#[test]
fn ef_req_04_the_collection_proxy_reaches_the_prepared_request() {
    let dir = collection(PROXY);

    let network = prepared(&dir, Some(NetworkPrefs::default())).unwrap().request.network;

    let proxy = network.proxy.expect("un proxy");
    assert_eq!((proxy.scheme, proxy.host.as_str(), proxy.port), (ProxyScheme::Socks5, "corp.test", 1081));
}

#[test]
fn ef_req_04_a_client_certificate_is_chosen_for_the_final_url_with_its_variables_resolved() {
    let dir = collection(
        "  clientCertificates:\n    - domain: '{{host}}'\n      type: pem\n      certificateFilePath: certs/c.pem\n      privateKeyFilePath: certs/k.pem\n",
    );
    write(dir.path(), "certs/c.pem", "CERT");
    write(dir.path(), "certs/k.pem", "KEY");

    let network = prepared(&dir, Some(NetworkPrefs::default())).unwrap().request.network;

    let identity = network.tls.client.expect("un certificat client");
    assert_eq!((identity.cert_pem, identity.key_pem), (b"CERT".to_vec(), b"KEY".to_vec()));
}

#[test]
fn ef_req_04_an_unreadable_certificate_fails_the_preparation_with_the_reason() {
    let dir = collection(
        "  clientCertificates:\n    - domain: api.test\n      type: pem\n      certificateFilePath: certs/absent.pem\n      privateKeyFilePath: certs/k.pem\n",
    );
    write(dir.path(), "certs/k.pem", "KEY");

    let error = prepared(&dir, Some(NetworkPrefs::default())).err().expect("une erreur").to_string();

    assert!(error.starts_with("réglages réseau : certificat client illisible"), "{error}");
}

#[test]
fn ef_req_04_without_host_settings_nothing_is_resolved_or_read() {
    let dir = collection(&format!(
        "{PROXY}  clientCertificates:\n    - domain: api.test\n      type: pem\n      certificateFilePath: absent.pem\n      privateKeyFilePath: absent.pem\n"
    ));

    let network = prepared(&dir, None).unwrap().request.network;

    assert!(network.proxy.is_none() && network.tls.client.is_none() && network.tls.verify);
}

#[test]
fn ef_req_04_host_preferences_set_verification_and_the_proxy_when_the_collection_inherits() {
    let dir = collection("");
    let prefs = NetworkPrefs {
        verify_tls: false,
        proxy: ProxyPref {
            mode: ProxyMode::Manual,
            config: xc_core::ProxyConfig { hostname: "global.test".into(), port: "3128".into(), ..Default::default() },
        },
        ..NetworkPrefs::default()
    };

    let network = prepared(&dir, Some(prefs)).unwrap().request.network;

    assert!(!network.tls.verify);
    assert_eq!(network.proxy.map(|p| (p.host, p.port)), Some(("global.test".to_owned(), 3128)));
}
