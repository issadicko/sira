//! HashiCorp Vault : lire un champ d'un secret (moteur KV, versions 1 et 2) pour l'utiliser comme variable. Une
//! variable `{{$vault.<montage>/<chemin>#<champ>}}` vaut le champ du secret ; la connexion est celle des requêtes HTTP
//! (proxy, autorité de confiance, certificat client), la valeur n'est jamais écrite dans un fichier.

mod client;
mod reference;

pub use client::{VaultAuth, VaultClient, VaultConfig, VaultInfo};
pub use reference::{references, VaultRef, PREFIX};

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum VaultError {
    #[error("Vault n'est pas configuré : donne son adresse (réglages ou VAULT_ADDR)")]
    NotConfigured,
    #[error("aucun jeton Vault : donne un jeton, un AppRole, ou connecte-toi avec `vault login`")]
    NoToken,
    #[error("référence Vault invalide « {0} » : attendu `montage/chemin#champ`")]
    BadReference(String),
    #[error("{0}")]
    Request(String),
    #[error("Vault a refusé ({status}) : {message}")]
    Refused { status: u16, message: String },
    #[error("le secret {path} n'existe pas dans Vault")]
    NotFound { path: String },
    #[error("le secret {path} n'a pas de champ « {field} » (champs : {fields})")]
    NoField { path: String, field: String, fields: String },
}

pub type Result<T> = std::result::Result<T, VaultError>;
