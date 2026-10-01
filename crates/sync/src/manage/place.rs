//! Réordonner et déplacer par glisser-déposer (`handleCollectionItemDrop` de Bruno).

use std::fs;
use std::path::{Path, PathBuf};

use xc_core::collection::{write_atomic, TreeItem, FOLDER_FILE, REQUEST_EXT};
use xc_core::yaml::Value;
use xc_core::CoreError;

use super::copy::relocate;
use super::item::{end_seq, info_file, minimal_folder_file, siblings, taken_names, updated_info};
use super::paths::{locate, split, visible, Item};
use super::track::Tracking;
use super::{DropPosition, ManageError};
use crate::import::join;
use crate::sync::write_parallel;

/// Fichier qui porte `seq` d'un frère de la collection.
fn sibling_file(root: &Path, sibling: &TreeItem) -> PathBuf {
    let path = root.join(sibling.path());
    match sibling {
        TreeItem::Folder { .. } => path.join(FOLDER_FILE),
        TreeItem::Request { .. } => path,
    }
}

/// Frères dont le `seq` est à réécrire, avec leur nouveau `seq`.
type Renumbered<'a> = Vec<(&'a TreeItem, i64)>;

/// `seq` de l'élément déplacé et `seq` à réécrire chez les frères `others` (hors l'élément déplacé). Déposer dans un
/// dossier place l'élément en fin de liste sans toucher aux frères ; déposer avant ou après le frère `anchor`
/// renumérote le dossier de 1 à n, et seuls les frères dont le `seq` change sont à réécrire.
fn numbering<'a>(
    others: &'a [TreeItem],
    anchor: Option<&str>,
    after: bool,
) -> Result<(i64, Renumbered<'a>), ManageError> {
    let Some(anchor) = anchor else { return Ok((end_seq(others.iter()), Vec::new())) };
    let at = others.iter().position(|s| s.path() == anchor).ok_or_else(|| ManageError::NotFound(anchor.to_owned()))?;
    let mut order: Vec<Option<&TreeItem>> = others.iter().map(Some).collect();
    order.insert(at + usize::from(after), None);
    let (mut moved, mut changed) = (0, Vec::new());
    for (slot, seq) in order.into_iter().zip(1..) {
        match slot {
            None => moved = seq,
            Some(sibling) if sibling.seq() != Some(seq) => changed.push((sibling, seq)),
            Some(_) => {}
        }
    }
    Ok((moved, changed))
}

/// Nouveau texte des fichiers des frères dont `seq` change, lu avant toute écriture : rien n'est écrit si l'un d'eux
/// est illisible.
fn sibling_writes(root: &Path, changed: &Renumbered) -> Result<Vec<(PathBuf, String)>, ManageError> {
    let mut writes = Vec::new();
    for (sibling, seq) in changed {
        let file = sibling_file(root, sibling);
        let created =
            matches!(sibling, TreeItem::Folder { .. }).then(|| minimal_folder_file(sibling.name(), Some(*seq)));
        if let Some(text) = updated_info(&file, &[("seq", Value::Int(*seq))], created)? {
            writes.push((file, text));
        }
    }
    Ok(writes)
}

/// Déplace la requête ou le dossier `path` par rapport à `target`, et renvoie son nouveau chemin relatif. `target` est
/// le frère de référence pour `Before` et `After`, le dossier d'accueil (`""` pour la racine) pour `Inside`.
///
/// Dans le même dossier, seuls les `seq` sont réécrits (ligne `info.seq`). Vers un autre dossier, l'élément est
/// renommé de façon atomique (copie vérifiée puis suppression de la source si les volumes diffèrent), avec un suffixe
/// ` n` si le nom est pris. Déposer un dossier dans lui-même ou dans un de ses descendants est refusé.
pub fn move_item(root: &Path, path: &str, target: &str, position: DropPosition) -> Result<String, ManageError> {
    let item = locate(root, path)?;
    let tracking = Tracking::load(root)?;
    let (parent, name) = split(path);
    let (folder, anchor) = match position {
        DropPosition::Inside => (target, None),
        DropPosition::Before | DropPosition::After => {
            locate(root, target)?;
            (split(target).0, Some(target))
        }
    };
    if anchor == Some(path) {
        return Ok(path.to_owned());
    }
    let dest = visible(root, folder)?;
    let others: Vec<TreeItem> = siblings(root, folder)?.into_iter().filter(|s| s.path() != path).collect();
    if item.is_dir && lies_within(&dest, &item.path)? {
        return Err(ManageError::IntoItself(path.to_owned()));
    }
    let (moved_seq, changed) = numbering(&others, anchor, position == DropPosition::After)?;
    let mut writes = sibling_writes(root, &changed)?;
    let created = item.is_dir.then(|| minimal_folder_file(name, Some(moved_seq)));
    let moved_text = updated_info(&info_file(&item), &[("seq", Value::Int(moved_seq))], created)?;

    let new_path = if folder == parent { path.to_owned() } else { relocated(&item, &dest, folder, name)? };
    if let Some(text) = moved_text {
        let moved = Item { path: root.join(&new_path), is_dir: item.is_dir };
        writes.push((info_file(&moved), text));
    }
    write_parallel(&writes, |(file, text)| write_atomic(file, text))?;
    if new_path != path {
        tracking.moved(path, &new_path)?;
    }
    Ok(new_path)
}

/// `dest` est `folder` ou s'y trouve, liens symboliques résolus.
fn lies_within(dest: &Path, folder: &Path) -> Result<bool, CoreError> {
    let resolve = |path: &Path| fs::canonicalize(path).map_err(|e| CoreError::io(path, e));
    Ok(resolve(dest)?.starts_with(resolve(folder)?))
}

/// Déplace `item` dans `dest` (le dossier relatif `folder`) et renvoie son nouveau chemin relatif.
fn relocated(item: &Item, dest: &Path, folder: &str, name: &str) -> Result<String, ManageError> {
    let (stem, ext) = match item.is_dir {
        true => (name, ""),
        false => (name.strip_suffix(REQUEST_EXT).unwrap_or(name), REQUEST_EXT),
    };
    let target = taken_names(dest, Some(folder.is_empty()), None)?.claim(stem, ext);
    relocate(&item.path, &dest.join(&target))?;
    Ok(join(folder, &target))
}
