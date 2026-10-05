//! Environnements : création, renommage, duplication et suppression de `environments/<nom>.yml`.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde_json::json;
use xc_core::collection::{
    default_environment, environment_file, set_default_environment, with_environment_name, write_new, ENV_DIR,
    REQUEST_EXT,
};
use xc_core::CoreError;

use super::exclusive::rename_new;
use super::info::{commit, rewrite_file};
use super::names::{claim_unique, taken_names, Wanted};
use super::paths::Scope;
use super::{io_error, ManageError};
use crate::import::request_file_name;
use crate::stringify;

/// Radical (le nom de l'environnement) d'un nom de fichier.
fn stem_of(file: &str) -> &str {
    file.strip_suffix(REQUEST_EXT).unwrap_or(file)
}

fn file_stem(path: &Path) -> String {
    stem_of(&path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()).to_owned()
}

/// Nom de fichier voulu pour l'environnement `name` : le même assainissement que pour une requête.
fn wanted_stem(name: &str) -> Result<String, ManageError> {
    let file = request_file_name(name).map_err(ManageError::InvalidName)?;
    Ok(stem_of(&file).to_owned())
}

/// Dossier `environments/`, créé s'il manque ; un lien symbolique est refusé, il pourrait mener hors de la collection.
fn ensure_dir(root: &Path) -> Result<PathBuf, ManageError> {
    let dir = root.join(ENV_DIR);
    match fs::symlink_metadata(&dir) {
        Ok(meta) if meta.is_symlink() => Err(CoreError::Symlink(ENV_DIR.into()).into()),
        Ok(meta) if meta.is_dir() => Ok(dir),
        Ok(_) => Err(ManageError::Forbidden(ENV_DIR.into())),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            fs::create_dir(&dir).map_err(|e| io_error(&dir, e))?;
            Ok(dir)
        }
        Err(e) => Err(CoreError::io(&dir, e).into()),
    }
}

/// Fichier de l'environnement existant `name`, qui n'est pas un lien symbolique.
fn locate(root: &Path, name: &str) -> Result<PathBuf, ManageError> {
    let path = environment_file(root, name)?;
    let shown = || format!("{ENV_DIR}/{name}{REQUEST_EXT}");
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_symlink() => Err(CoreError::Symlink(shown()).into()),
        Ok(meta) if meta.is_file() => Ok(path),
        Ok(_) => Err(ManageError::Forbidden(shown())),
        Err(e) if e.kind() == ErrorKind::NotFound => Err(ManageError::NotFound(shown())),
        Err(e) => Err(CoreError::io(&path, e).into()),
    }
}

const NAME_KEY: &str = "la clé name du fichier n'a pas pu être mise à jour";
const DEFAULT_KEY: &str = "l'environnement par défaut de opencollection.yml n'a pas pu être mis à jour";

fn not_updated(done: &str, what: &str, e: impl std::fmt::Display) -> ManageError {
    ManageError::NotUpdated { done: done.to_owned(), what: what.to_owned(), message: e.to_string() }
}

/// Crée un environnement vide `name` et renvoie son nom. Un nom pris reçoit un suffixe ` 1`, ` 2`… ; `name` dans le
/// fichier est le nom retenu. Le nom n'est jamais un chemin : les caractères interdits d'un nom de fichier sont
/// remplacés, comme pour une requête.
pub fn create_environment(root: &Path, name: &str) -> Result<String, ManageError> {
    let stem = wanted_stem(name)?;
    let scope = Scope::open(root)?;
    let dir = ensure_dir(scope.root)?;
    let names = taken_names(&dir, None)?;
    let created = claim_unique(&dir, names, Wanted::new(&stem, REQUEST_EXT), |path| {
        let text = stringify::environment(&json!({ "name": file_stem(path), "variables": [] }));
        write_new(path, &text).map_err(|e| io_error(path, e))
    })?;
    Ok(stem_of(&created).to_owned())
}

/// Renomme l'environnement `from` en `name` et renvoie son nouveau nom. Le fichier est renommé de façon atomique,
/// sans jamais remplacer un environnement existant (suffixe ` n` si le nom est pris), puis seule la clé `name` du
/// fichier change. Si c'était l'environnement par défaut de la collection, `opencollection.yml` le suit.
pub fn rename_environment(root: &Path, from: &str, name: &str) -> Result<String, ManageError> {
    let stem = wanted_stem(name)?;
    let scope = Scope::open(root)?;
    let source = locate(scope.root, from)?;
    let dir = source.parent().unwrap_or(root);
    let current = format!("{from}{REQUEST_EXT}");
    let names = taken_names(dir, Some(&current))?;
    let target = claim_unique(dir, names, Wanted::new(&stem, REQUEST_EXT), |target| match target == source {
        true => Ok(()),
        false => rename_new(&source, target).map_err(|e| io_error(&source, e)),
    })?;
    let renamed = stem_of(&target).to_owned();
    if renamed == from {
        return Ok(renamed);
    }
    let done = "l'environnement est bien renommé";
    let file = dir.join(&target);
    rewrite_file(&file, |text| with_environment_name(&file, text, &renamed))
        .and_then(|edit| edit.map_or(Ok(()), |edit| commit(root, &[edit])))
        .map_err(|e| not_updated(done, NAME_KEY, e))?;
    let follows = default_environment(root).map_err(|e| not_updated(done, DEFAULT_KEY, e))?;
    if follows.as_deref() == Some(from) {
        set_default_environment(root, Some(&renamed)).map_err(|e| not_updated(done, DEFAULT_KEY, e))?;
    }
    Ok(renamed)
}

/// Duplique l'environnement `from` sous le nom `name` et renvoie le nom de la copie : le fichier est copié, seule sa
/// clé `name` change. Aucun fichier existant n'est remplacé ; un lien symbolique est refusé.
pub fn clone_environment(root: &Path, from: &str, name: &str) -> Result<String, ManageError> {
    let stem = wanted_stem(name)?;
    let scope = Scope::open(root)?;
    let source = locate(scope.root, from)?;
    let text = fs::read_to_string(&source).map_err(|e| CoreError::io(&source, e))?;
    let dir = source.parent().unwrap_or(root);
    let names = taken_names(dir, None)?;
    let created = claim_unique(dir, names, Wanted::new(&stem, REQUEST_EXT), |copy| {
        let renamed = with_environment_name(&source, &text, &file_stem(copy))?;
        write_new(copy, &renamed).map_err(|e| io_error(copy, e))
    })?;
    Ok(stem_of(&created).to_owned())
}

/// Envoie l'environnement `name` à la corbeille avec `trash`. S'il était l'environnement par défaut, la collection n'en
/// a plus ; si cette écriture échoue, l'erreur dit que l'environnement est bien dans la corbeille.
pub fn delete_environment(
    root: &Path,
    name: &str,
    trash: impl Fn(&Path) -> Result<(), String>,
) -> Result<(), ManageError> {
    let scope = Scope::open(root)?;
    let source = locate(scope.root, name)?;
    let was_default = default_environment(root)?.as_deref() == Some(name);
    trash(&source).map_err(ManageError::Trash)?;
    if was_default {
        set_default_environment(root, None)
            .map_err(|e| not_updated("l'environnement est bien dans la corbeille", DEFAULT_KEY, e))?;
    }
    Ok(())
}
