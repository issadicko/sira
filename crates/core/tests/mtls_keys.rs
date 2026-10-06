use std::collections::HashMap;
use std::fs;
use std::path::Path;

use base64::Engine as _;
use xc_core::{prepare_with, read_request, NetworkPrefs, Overrides};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/mtls");

fn collection(certificate: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for entry in fs::read_dir(FIXTURES).unwrap().flatten() {
        fs::copy(entry.path(), dir.path().join(entry.file_name())).unwrap();
    }
    fs::write(
        dir.path().join("opencollection.yml"),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: Réseau\n\nrequest:\n  variables:\n    - name: pass\n      value: secret\n\nconfig:\n  clientCertificates:\n{certificate}"),
    )
    .unwrap();
    fs::write(
        dir.path().join("r.yml"),
        "info:\n  name: R\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"https://api.test/v1\"\n",
    )
    .unwrap();
    dir
}

fn identity(dir: &tempfile::TempDir) -> Result<(Vec<u8>, Vec<u8>), String> {
    let doc = read_request(dir.path(), "r.yml").unwrap();
    let overrides = Overrides { network: Some(NetworkPrefs::default()), ..Overrides::default() };
    prepare_with(dir.path(), "r.yml", &doc, None, &HashMap::new(), overrides)
        .map(|p| {
            let client = p.request.network.tls.client.expect("un certificat client");
            (client.cert_pem, client.key_pem)
        })
        .map_err(|e| e.to_string())
}

/// Les octets DER du premier bloc PEM de `pem`.
fn der(pem: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(pem);
    let body: String = text
        .lines()
        .skip_while(|l| !l.starts_with("-----BEGIN"))
        .skip(1)
        .take_while(|l| !l.starts_with("-----END"))
        .collect();
    base64::engine::general_purpose::STANDARD.decode(body.trim()).unwrap()
}

fn original() -> (Vec<u8>, Vec<u8>) {
    let read = |name: &str| fs::read(Path::new(FIXTURES).join(name)).unwrap();
    (der(&read("cert.pem")), der(&read("key.pem")))
}

fn pkcs12(file: &str, passphrase: &str) -> String {
    format!("    - domain: api.test\n      type: pkcs12\n      pkcs12FilePath: {file}\n      passphrase: \"{passphrase}\"\n")
}

fn pem_pair(key: &str, passphrase: &str) -> String {
    format!(
        "    - domain: api.test\n      type: pem\n      certificateFilePath: cert.pem\n      privateKeyFilePath: {key}\n      passphrase: \"{passphrase}\"\n"
    )
}

#[test]
fn ef_req_04_a_pkcs12_certificate_gives_the_same_certificate_and_key_as_its_pem_files() {
    let (cert, key) = original();
    for file in ["modern.p12", "legacy.p12"] {
        let (cert_pem, key_pem) =
            identity(&collection(&pkcs12(file, "secret"))).unwrap_or_else(|e| panic!("{file} : {e}"));
        assert_eq!(der(&cert_pem), cert, "{file} : le certificat");
        assert_eq!(der(&key_pem), key, "{file} : la clé");
        assert!(String::from_utf8_lossy(&key_pem).starts_with("-----BEGIN PRIVATE KEY-----"));
    }
}

#[test]
fn ef_req_04_the_passphrase_of_a_pkcs12_certificate_may_be_a_variable_or_empty() {
    let (cert, _) = original();
    let (cert_pem, _) = identity(&collection(&pkcs12("modern.p12", "{{pass}}"))).unwrap();
    assert_eq!(der(&cert_pem), cert);
    let (empty, _) = identity(&collection(&pkcs12("nopass.p12", ""))).unwrap();
    assert_eq!(der(&empty), cert);
}

#[test]
fn ef_req_04_a_wrong_pkcs12_passphrase_is_said_without_repeating_it() {
    let error = identity(&collection(&pkcs12("modern.p12", "mauvaise"))).unwrap_err();
    assert!(error.contains("modern.p12") && error.contains("passphrase"), "{error}");
    assert!(!error.contains("mauvaise"), "{error}");
}

#[test]
fn ef_req_04_a_pkcs12_file_that_is_not_one_or_is_missing_is_explained() {
    let dir = collection(&pkcs12("cert.pem", "secret"));
    assert!(identity(&dir).unwrap_err().contains("PKCS#12"));
    assert!(identity(&collection(&pkcs12("absent.p12", "x"))).unwrap_err().contains("illisible"));
    assert!(identity(&collection("    - domain: api.test\n      type: pkcs12\n")).unwrap_err().contains("fichier"));
}

#[test]
fn ef_req_04_an_encrypted_pkcs8_key_is_decrypted_with_the_passphrase() {
    let (cert, key) = original();
    let (cert_pem, key_pem) = identity(&collection(&pem_pair("key-encrypted.pem", "secret"))).unwrap();
    assert_eq!(der(&cert_pem), cert);
    assert_eq!(der(&key_pem), key);
}

#[test]
fn ef_req_04_a_pkcs8_key_encrypted_with_the_old_pkcs12_scheme_is_refused_with_a_reason() {
    let error = identity(&collection(&pem_pair("key-encrypted-3des.pem", "secret"))).unwrap_err();
    assert!(
        error.contains("key-encrypted-3des.pem") && error.contains("PBES2") && error.contains("-v2 aes-256-cbc"),
        "{error}"
    );
}

#[test]
fn ef_req_04_an_encrypted_key_without_or_with_a_wrong_passphrase_is_refused_with_the_reason() {
    let missing = identity(&collection(&pem_pair("key-encrypted.pem", ""))).unwrap_err();
    assert!(missing.contains("chiffrée") && missing.contains("passphrase"), "{missing}");
    let wrong = identity(&collection(&pem_pair("key-encrypted.pem", "mauvaise"))).unwrap_err();
    assert!(wrong.contains("passphrase") && !wrong.contains("mauvaise"), "{wrong}");
}

#[test]
fn ef_req_04_the_old_openssl_encrypted_format_is_refused_with_the_conversion_to_run() {
    let error = identity(&collection(&pem_pair("key-legacy-encrypted.pem", "secret"))).unwrap_err();
    assert!(error.contains("openssl pkcs8 -topk8"), "{error}");
}

#[test]
fn ef_req_04_a_plain_key_is_passed_through_untouched_whatever_the_passphrase() {
    let (_, key_pem) = identity(&collection(&pem_pair("key.pem", "inutile"))).unwrap();
    assert_eq!(key_pem, fs::read(Path::new(FIXTURES).join("key.pem")).unwrap());
}
