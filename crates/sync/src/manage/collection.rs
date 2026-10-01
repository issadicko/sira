//! Ouverture d'un dossier quelconque et création d'une collection (`renderer:create-collection` de Bruno).

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::json;
use xc_core::collection::{write_new, COLLECTION_FILE};
use xc_core::CoreError;

use super::item::create_unique;
use super::{FolderKind, ManageError};
use crate::import::{sanitize_name, validate_name};
use crate::stringify;

const BRUNO_FILE: &str = "bruno.json";
const SYSTEM_FILES: [&str; 3] = [".DS_Store", "Thumbs.db", "desktop.ini"];
const GITIGNORE_FILE: &str = ".gitignore";
const GITIGNORE: &str = "# Secrets\n.env*\n\n# Dependencies\nnode_modules\n\n# OS files\n.DS_Store\nThumbs.db";

/// Classe le dossier choisi : collection OpenCollection, dossier vide (hors fichiers cachés du système), collection
/// au format `.bru` ou autre contenu.
pub fn inspect_folder(path: &Path) -> Result<FolderKind, ManageError> {
    if !path.is_dir() {
        return Err(ManageError::FolderNotFound(path.display().to_string()));
    }
    if path.join(COLLECTION_FILE).is_file() {
        return Ok(FolderKind::Collection);
    }
    if path.join(BRUNO_FILE).is_file() {
        return Ok(FolderKind::Bru);
    }
    let is_empty = is_empty(path)?;
    Ok(if is_empty { FolderKind::Empty } else { FolderKind::Other })
}

fn is_empty(dir: &Path) -> Result<bool, CoreError> {
    let mut entries = fs::read_dir(dir).map_err(|e| CoreError::io(dir, e))?;
    Ok(entries.all(|entry| entry.is_ok_and(|e| SYSTEM_FILES.contains(&e.file_name().to_string_lossy().as_ref()))))
}

/// Nom du dossier d'une collection : `sanitizeName(nom)`, validé par `validateName`.
fn folder_name(name: &str) -> Result<String, ManageError> {
    let folder = sanitize_name(name);
    validate_name(&folder).map_err(ManageError::InvalidName)?;
    Ok(folder)
}

fn existing_dir(dir: &Path) -> Result<PathBuf, ManageError> {
    match dir.is_dir() {
        true => std::path::absolute(dir).map_err(|e| CoreError::io(dir, e).into()),
        false => Err(ManageError::FolderNotFound(dir.display().to_string())),
    }
}

/// Crée la collection `name` dans un nouveau dossier de `parent`, comme « Nouvelle collection » de Bruno, et renvoie
/// sa racine. Un dossier du même nom, s'il est vide, est réutilisé ; sinon le nouveau dossier reçoit un suffixe
/// ` 1`, ` 2`…
pub fn create_collection(parent: &Path, name: &str) -> Result<PathBuf, ManageError> {
    let folder = folder_name(name)?;
    let parent = existing_dir(parent)?;
    let desired = parent.join(&folder);
    if desired.is_dir() && is_empty(&desired)? {
        write_files(&desired, name)?;
        return Ok(desired);
    }
    let root = parent.join(create_unique(&parent, None, &folder, "", |path| fs::create_dir(path))?);
    write_files(&root, name).inspect_err(|_| {
        fs::remove_dir(&root).ok();
    })?;
    Ok(root)
}

/// Crée la collection `name` dans le dossier existant `dir` (« Créer une collection ici ») sans rien modifier de ce
/// qu'il contient, et renvoie sa racine. Refusé quand `opencollection.yml` y existe déjà.
pub fn init_collection(dir: &Path, name: &str) -> Result<PathBuf, ManageError> {
    folder_name(name)?;
    let root = existing_dir(dir)?;
    write_files(&root, name)?;
    Ok(root)
}

/// `opencollection.yml` puis `.gitignore`, seuls fichiers écrits (ni `environments/` ni `.env`) ; un `.gitignore`
/// existant n'est pas écrasé.
fn write_files(dir: &Path, name: &str) -> Result<(), ManageError> {
    let config = dir.join(COLLECTION_FILE);
    let root = json!({ "meta": { "name": name } });
    let bruno = json!({
        "opencollection": "1.0.0",
        "name": name,
        "type": "collection",
        "ignore": ["node_modules", ".git"]
    });
    write_new(&config, &stringify::collection(&root, &bruno)).map_err(|e| match e.kind() {
        ErrorKind::AlreadyExists => ManageError::AlreadyCollection(dir.display().to_string()),
        _ => CoreError::io(&config, e).into(),
    })?;
    let ignore = dir.join(GITIGNORE_FILE);
    match write_new(&ignore, GITIGNORE) {
        Err(e) if e.kind() != ErrorKind::AlreadyExists => {
            fs::remove_file(&config).ok();
            Err(CoreError::io(&ignore, e).into())
        }
        _ => Ok(()),
    }
}
