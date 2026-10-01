//! Chemins relatifs reçus de l'interface : confinement à la collection et élément visé.

use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use xc_core::collection::{is_hidden, resolve_path, REQUEST_EXT};
use xc_core::CoreError;

use super::ManageError;

/// Requête (fichier `.yml`) ou dossier visé par une action.
pub(super) struct Item {
    pub path: PathBuf,
    pub is_dir: bool,
}

/// Sépare le dossier parent (`""` pour la racine) et le nom d'un chemin relatif.
pub(super) fn split(relative: &str) -> (&str, &str) {
    relative.rsplit_once('/').unwrap_or(("", relative))
}

/// Chemin d'un dossier ou d'un élément que l'arbre montre : ni hors de la collection (`..`, chemin absolu, lien
/// symbolique qui sort), ni caché ou réservé (`opencollection.yml`, `environments`, `.oc-sync`…), à aucun niveau du
/// chemin et sans tenir compte de la casse. `""` désigne la racine.
pub(super) fn visible(root: &Path, relative: &str) -> Result<PathBuf, ManageError> {
    let path = resolve_path(root, relative)?;
    let mut at_root = true;
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(name) if !is_hidden(&name.to_string_lossy().to_lowercase(), at_root) => at_root = false,
            _ => return Err(ManageError::Forbidden(relative.to_owned())),
        }
    }
    Ok(path)
}

/// La requête ou le dossier `relative`, qui doit exister ; la racine n'est jamais une cible.
pub(super) fn locate(root: &Path, relative: &str) -> Result<Item, ManageError> {
    if relative.is_empty() {
        return Err(ManageError::Forbidden("la racine de la collection".into()));
    }
    let path = visible(root, relative)?;
    let meta = fs::symlink_metadata(&path).map_err(|e| match e.kind() {
        ErrorKind::NotFound => ManageError::NotFound(relative.to_owned()),
        _ => CoreError::io(&path, e).into(),
    })?;
    if !meta.is_dir() && !relative.ends_with(REQUEST_EXT) {
        return Err(ManageError::Forbidden(relative.to_owned()));
    }
    Ok(Item { path, is_dir: meta.is_dir() })
}
