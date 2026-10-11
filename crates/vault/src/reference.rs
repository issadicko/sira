use crate::{Result, VaultError};

/// Le préfixe des variables Vault : `{{$vault.secret/app/db#password}}`.
pub const PREFIX: &str = "$vault.";

/// Un champ d'un secret : le montage du moteur de secrets, le chemin dans ce montage et le champ.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VaultRef {
    pub mount: String,
    pub path: String,
    pub field: String,
}

impl VaultRef {
    /// Lit `montage/chemin#champ`, avec ou sans le préfixe `$vault.`.
    pub fn parse(text: &str) -> Result<Self> {
        let body = text.strip_prefix(PREFIX).unwrap_or(text).trim();
        let bad = || VaultError::BadReference(text.to_owned());
        let (location, field) = body.split_once('#').ok_or_else(bad)?;
        let location = location.trim_matches('/');
        let (mount, path) = location.split_once('/').ok_or_else(bad)?;
        let segments_ok = location.split('/').all(|segment| !segment.is_empty());
        if !segments_ok || path.is_empty() || field.trim().is_empty() {
            return Err(bad());
        }
        Ok(Self { mount: mount.to_owned(), path: path.to_owned(), field: field.trim().to_owned() })
    }

    /// Le nom de la variable que cette référence remplit : celui que l'interpolation cherche.
    pub fn variable(&self) -> String {
        format!("{PREFIX}{}/{}#{}", self.mount, self.path, self.field)
    }

    /// Le secret entier, sans le champ : de quoi ne le lire qu'une fois pour plusieurs champs.
    pub fn secret(&self) -> String {
        format!("{}/{}", self.mount, self.path)
    }
}

/// Les références Vault parmi des noms de variables sans valeur (ceux que l'interpolation n'a pas su remplir) ; une
/// référence mal écrite est rendue avec son erreur.
pub fn references(names: &[String]) -> Vec<Result<VaultRef>> {
    names.iter().filter(|name| name.starts_with(PREFIX)).map(|name| VaultRef::parse(name)).collect()
}
