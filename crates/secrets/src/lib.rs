//! Valeurs des variables secrètes d'un environnement : gardées dans le trousseau du système (Keychain sur macOS,
//! Gestionnaire d'identifiants sur Windows, Secret Service sur Linux), jamais dans les fichiers de la collection.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("trousseau indisponible : {0}")]
    Unavailable(String),
    #[error("le trousseau a refusé l'opération : {0}")]
    Failed(String),
}

/// Où vit un secret : une variable d'un environnement d'une collection. `collection` est le chemin de la collection :
/// les secrets restent sur cette machine et ne suivent pas un dossier copié ailleurs.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretKey {
    pub collection: String,
    pub environment: String,
    pub name: String,
}

impl SecretKey {
    pub fn new(collection: &str, environment: &str, name: &str) -> Self {
        Self { collection: collection.to_owned(), environment: environment.to_owned(), name: name.to_owned() }
    }

    /// Le nom du compte dans le trousseau : les trois parties, séparées par un caractère qu'aucune ne contient.
    fn account(&self) -> String {
        format!("{}\u{1f}{}\u{1f}{}", self.collection, self.environment, self.name)
    }
}

pub trait SecretStore: Send + Sync + std::fmt::Debug {
    /// La valeur gardée, `None` quand il n'y en a pas.
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError>;
    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError>;
    /// Oublie la valeur ; sans effet quand il n'y en a pas.
    fn delete(&self, key: &SecretKey) -> Result<(), SecretError>;
}

/// Le trousseau du système.
#[derive(Debug, Default)]
pub struct Keychain;

const SERVICE: &str = "io.github.issadicko.sira";

impl Keychain {
    fn entry(key: &SecretKey) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(SERVICE, &key.account()).map_err(unavailable)
    }
}

fn unavailable(e: keyring::Error) -> SecretError {
    SecretError::Unavailable(e.to_string())
}

fn failed(e: keyring::Error) -> SecretError {
    match e {
        keyring::Error::NoStorageAccess(_) | keyring::Error::PlatformFailure(_) => {
            SecretError::Unavailable(e.to_string())
        }
        other => SecretError::Failed(other.to_string()),
    }
}

impl SecretStore for Keychain {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(failed(e)),
        }
    }

    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError> {
        Self::entry(key)?.set_password(value).map_err(failed)
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(failed(e)),
        }
    }
}

/// Un trousseau en mémoire, pour les tests et les machines sans trousseau.
#[derive(Debug, Default)]
pub struct MemoryStore {
    values: Mutex<HashMap<SecretKey, String>>,
    /// Quand il est posé, chaque opération échoue comme un trousseau indisponible.
    broken: Mutex<bool>,
}

impl MemoryStore {
    pub fn broken() -> Self {
        Self { values: Mutex::default(), broken: Mutex::new(true) }
    }

    fn check(&self) -> Result<(), SecretError> {
        if *self.broken.lock().unwrap_or_else(std::sync::PoisonError::into_inner) {
            Err(SecretError::Unavailable("aucun service de trousseau".into()))
        } else {
            Ok(())
        }
    }

    fn values(&self) -> std::sync::MutexGuard<'_, HashMap<SecretKey, String>> {
        self.values.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl SecretStore for MemoryStore {
    fn get(&self, key: &SecretKey) -> Result<Option<String>, SecretError> {
        self.check()?;
        Ok(self.values().get(key).cloned())
    }

    fn set(&self, key: &SecretKey, value: &str) -> Result<(), SecretError> {
        self.check()?;
        self.values().insert(key.clone(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &SecretKey) -> Result<(), SecretError> {
        self.check()?;
        self.values().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(name: &str) -> SecretKey {
        SecretKey::new("/c", "dev", name)
    }

    #[test]
    fn enf_sec_01_a_secret_is_set_read_replaced_and_forgotten() {
        let store = MemoryStore::default();
        assert_eq!(store.get(&key("t")).unwrap(), None);
        store.set(&key("t"), "one").unwrap();
        store.set(&key("t"), "two").unwrap();
        assert_eq!(store.get(&key("t")).unwrap().as_deref(), Some("two"));
        store.delete(&key("t")).unwrap();
        store.delete(&key("t")).unwrap();
        assert_eq!(store.get(&key("t")).unwrap(), None);
    }

    #[test]
    fn enf_sec_01_collection_environment_and_name_keep_secrets_apart() {
        let store = MemoryStore::default();
        store.set(&key("t"), "a").unwrap();
        for other in [SecretKey::new("/d", "dev", "t"), SecretKey::new("/c", "prod", "t"), key("u")] {
            assert_eq!(store.get(&other).unwrap(), None, "{other:?}");
        }
    }

    #[test]
    fn enf_sec_01_the_keychain_account_joins_the_three_parts() {
        assert_eq!(SecretKey::new("/c", "dev", "t").account(), "/c\u{1f}dev\u{1f}t");
    }

    #[test]
    fn enf_sec_01_a_broken_store_reports_the_keychain_as_unavailable() {
        let store = MemoryStore::broken();
        for result in [store.get(&key("t")).map(|_| ()), store.set(&key("t"), "x"), store.delete(&key("t"))] {
            assert!(matches!(result, Err(SecretError::Unavailable(_))));
        }
    }
}
