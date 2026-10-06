//! Ce que le moteur ne lit pas lui-même d'un certificat client : un conteneur PKCS#12 (`.pfx`, `.p12`) et une clé privée
//! PKCS#8 chiffrée. Les deux sont ramenés à du PEM sans passphrase (certificat, clé), la forme que le moteur attend ;
//! la passphrase ne sort jamais de cette fonction et n'est citée dans aucun message.

use base64::Engine as _;
use p12_keystore::{KeyStore, Pkcs12ImportPolicy};
use pkcs8::der::Decode;
use pkcs8::EncryptedPrivateKeyInfoOwned;

/// Du PEM de `label` autour de `der`, lignes de 64 caractères.
fn pem(label: &str, der: &[u8]) -> String {
    let body = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for line in body.as_bytes().chunks(64) {
        out.push_str(&String::from_utf8_lossy(line));
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

/// Le certificat (suivi de sa chaîne) et la clé du premier couple d'un PKCS#12, en PEM.
pub fn from_pkcs12(data: &[u8], passphrase: &str) -> Result<(Vec<u8>, Vec<u8>), String> {
    let open = |policy| KeyStore::from_pkcs12(data, passphrase, policy);
    let store = open(Pkcs12ImportPolicy::Strict)
        .map_err(|e| format!("le conteneur PKCS#12 ne s'ouvre pas ({e}) : la passphrase est-elle la bonne ?"))?;
    let (_, chain) = match store.private_key_chain() {
        Some(found) => found,
        None => {
            return Err("le conteneur PKCS#12 ne contient aucune clé privée avec son certificat".to_owned());
        }
    };
    let certs: String = chain.certs().iter().map(|c| pem("CERTIFICATE", c.as_der())).collect();
    if certs.is_empty() {
        return Err("le conteneur PKCS#12 ne contient aucun certificat pour sa clé privée".to_owned());
    }
    Ok((certs.into_bytes(), pem("PRIVATE KEY", chain.key().as_der()).into_bytes()))
}

/// Le contenu d'un fichier de clé privée sans passphrase : une clé PKCS#8 chiffrée (`BEGIN ENCRYPTED PRIVATE KEY`) est
/// déchiffrée, une clé en clair rendue telle quelle. Le très ancien format chiffré d'OpenSSL (`Proc-Type: 4,ENCRYPTED`)
/// n'est pas lu : l'erreur dit comment le convertir.
pub fn plain_key(file: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    let text = String::from_utf8_lossy(file);
    if text.contains("Proc-Type: 4,ENCRYPTED") {
        return Err("la clé privée utilise l'ancien format chiffré d'OpenSSL, que seul le PKCS#8 remplace : \
             openssl pkcs8 -topk8 -in cle.pem -out cle-pkcs8.pem"
            .to_owned());
    }
    if !text.contains("BEGIN ENCRYPTED PRIVATE KEY") {
        return Ok(file.to_vec());
    }
    if passphrase.is_empty() {
        return Err("la clé privée du certificat client est chiffrée : renseigne sa passphrase".to_owned());
    }
    let body: String = text
        .lines()
        .skip_while(|l| !l.contains("BEGIN ENCRYPTED PRIVATE KEY"))
        .skip(1)
        .take_while(|l| !l.contains("END ENCRYPTED PRIVATE KEY"))
        .map(str::trim)
        .collect();
    let der = base64::engine::general_purpose::STANDARD
        .decode(body)
        .map_err(|_| "la clé privée chiffrée n'est pas du PEM valide".to_owned())?;
    let info = EncryptedPrivateKeyInfoOwned::from_der(&der).map_err(|_| {
        "la clé privée chiffrée n'est pas lisible : seul le chiffrement PBES2 est pris en charge \
             (openssl pkcs8 -topk8 -in cle.pem -out cle-aes.pem -v2 aes-256-cbc)"
            .to_owned()
    })?;
    let plain = info
        .decrypt(passphrase)
        .map_err(|_| "la clé privée ne se déchiffre pas : la passphrase est-elle la bonne ?".to_owned())?;
    Ok(pem("PRIVATE KEY", plain.as_bytes()).into_bytes())
}
