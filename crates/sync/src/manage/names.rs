//! Choix d'un nom libre dans un dossier, sans jamais écraser un élément existant.

use std::fs;
use std::path::Path;

use xc_core::CoreError;

use super::ManageError;
use crate::import::Directory;

/// Noms que porte déjà `dir`, hors `except` (l'élément qu'on renomme, que son propre nom ne gêne pas).
pub(super) fn taken_names(dir: &Path, except: Option<&str>) -> Result<Directory, ManageError> {
    let entries = fs::read_dir(dir).map_err(|e| CoreError::io(dir, e))?;
    let names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let taken: Vec<&str> = names.iter().map(String::as_str).filter(|name| Some(*name) != except).collect();
    Ok(Directory::new(&taken))
}

/// Nom voulu : `stem` et `ext`, avec un suffixe ` 1`, ` 2`… s'il est pris ; `exact`, quand il est libre, est gardé tel
/// quel, sans suffixe ni assainissement.
pub(super) struct Wanted<'a> {
    stem: &'a str,
    ext: &'a str,
    exact: Option<&'a str>,
}

impl<'a> Wanted<'a> {
    pub fn new(stem: &'a str, ext: &'a str) -> Self {
        Self { stem, ext, exact: None }
    }

    pub fn preferring(self, exact: &'a str) -> Self {
        Self { exact: Some(exact), ..self }
    }
}

/// Fait `attempt` sur le premier nom libre de `dir` selon `names`, puis sur le suivant tant que `attempt` répond
/// « existe déjà » : un élément créé entre-temps n'est jamais remplacé. Renvoie le nom retenu.
pub(super) fn claim_unique(
    dir: &Path,
    mut names: Directory,
    wanted: Wanted,
    attempt: impl Fn(&Path) -> Result<(), ManageError>,
) -> Result<String, ManageError> {
    loop {
        let exact = wanted.exact.and_then(|exact| names.claim_exact(exact));
        let name = exact.unwrap_or_else(|| names.claim(wanted.stem, wanted.ext));
        match attempt(&dir.join(&name)) {
            Ok(()) => return Ok(name),
            Err(ManageError::Exists(_)) => {}
            Err(e) => return Err(e),
        }
    }
}
