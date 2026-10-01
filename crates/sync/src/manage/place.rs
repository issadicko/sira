//! Réordonner et déplacer par glisser-déposer (`handleCollectionItemDrop` de Bruno).

use std::fs;
use std::path::Path;

use xc_core::collection::{TreeItem, FOLDER_FILE, REQUEST_EXT};
use xc_core::yaml::Value;
use xc_core::CoreError;

use super::copy::relocate;
use super::info::{commit, plan, symlink_error, Edit, Kind, Update};
use super::item::{end_seq, minimal_folder_file, siblings};
use super::names::{claim_unique, Wanted};
use super::paths::{split, Item, Scope};
use super::track::Tracking;
use super::{DropPosition, ManageError};
use crate::import::join;

/// Frères dont le `seq` est à réécrire, avec leur nouveau `seq`.
type Renumbered<'a> = Vec<(&'a TreeItem, i64)>;

/// Index où l'élément déplacé s'insère parmi les frères `others` : avant ou après le frère `anchor`, ou à la fin.
fn slot(others: &[TreeItem], anchor: Option<&str>, after: bool) -> Result<usize, ManageError> {
    let Some(anchor) = anchor else { return Ok(others.len()) };
    let at = others.iter().position(|s| s.path() == anchor).ok_or_else(|| ManageError::NotFound(anchor.to_owned()))?;
    Ok(at + usize::from(after))
}

/// `seq` de l'élément déplacé et `seq` à réécrire chez les frères `others` (hors l'élément déplacé). Déposer dans un
/// dossier (`slot` absent) place l'élément en fin de liste sans toucher aux frères ; déposer avant ou après un frère
/// renumérote le dossier de 1 à n, et seuls les frères dont le `seq` change sont à réécrire.
fn numbering(others: &[TreeItem], slot: Option<usize>) -> (i64, Renumbered<'_>) {
    let Some(slot) = slot else { return (end_seq(others.iter()), Vec::new()) };
    let mut order: Vec<Option<&TreeItem>> = others.iter().map(Some).collect();
    order.insert(slot, None);
    let (mut moved, mut changed) = (0, Vec::new());
    for (entry, seq) in order.into_iter().zip(1..) {
        match entry {
            None => moved = seq,
            Some(sibling) if sibling.seq() != Some(seq) => changed.push((sibling, seq)),
            Some(_) => {}
        }
    }
    (moved, changed)
}

/// Écritures de `seq` des frères renumérotés. Un frère qui ne peut pas être réécrit (YAML illisible, fichier qui n'est
/// pas une requête, lien symbolique) garde son `seq` : il ne fait pas échouer toute l'action.
fn sibling_edits(root: &Path, changed: &Renumbered) -> Vec<Edit> {
    let edits = changed.iter().filter_map(|(sibling, seq)| {
        let path = root.join(sibling.path());
        let update = match sibling {
            TreeItem::Folder { name, .. } => {
                let created = Some(minimal_folder_file(name, Some(*seq)));
                plan(&path.join(FOLDER_FILE), Kind::Folder, &[("seq", Value::Int(*seq))], created)
            }
            TreeItem::Request { .. } => plan(&path, Kind::Request, &[("seq", Value::Int(*seq))], None),
        };
        match update {
            Update::Write(edit) => Some(edit),
            Update::Keep | Update::Link | Update::Foreign => None,
        }
    });
    edits.collect()
}

/// Déplace la requête ou le dossier `path` par rapport à `target`, et renvoie son nouveau chemin relatif. `target` est
/// le frère de référence pour `Before` et `After`, le dossier d'accueil (`""` pour la racine) pour `Inside`.
///
/// Ne rien changer (même dossier, même position) n'écrit rien. Les `seq` sont écrits d'abord, sur place (ligne
/// `info.seq` seulement) ; l'élément est ensuite renommé, en dernier, de façon atomique et sans jamais remplacer un
/// élément existant (copie vérifiée puis suppression de la source si les volumes diffèrent), puis `source.yml` est mis
/// à jour aussitôt. Dans un autre dossier, l'élément garde son nom de fichier s'il y est libre, sinon un suffixe ` n`
/// le distingue. Déposer un dossier dans lui-même ou dans un de ses descendants est refusé, comme déplacer un lien
/// symbolique ou changer la position d'un fichier qui n'est pas une requête.
pub fn move_item(root: &Path, path: &str, target: &str, position: DropPosition) -> Result<String, ManageError> {
    let scope = Scope::open(root)?;
    let item = scope.locate(path)?;
    item.refuse_link()?;
    let tracking = Tracking::load(root)?;
    let (parent, name) = split(path);
    let (folder, anchor) = match position {
        DropPosition::Inside => (target, None),
        DropPosition::Before | DropPosition::After => {
            scope.locate(target)?;
            (split(target).0, Some(target))
        }
    };
    if anchor == Some(path) {
        return Ok(path.to_owned());
    }
    let dest = scope.visible(folder)?;
    let current = siblings(root, folder)?;
    let others: Vec<TreeItem> = current.iter().filter(|s| s.path() != path).cloned().collect();
    if item.is_dir && lies_within(&dest, &item.path)? {
        return Err(ManageError::IntoItself(path.to_owned()));
    }
    let slot = slot(&others, anchor, position == DropPosition::After)?;
    if folder == parent && keeps_order(&current, &others, slot, path) {
        return Ok(path.to_owned());
    }
    let (moved_seq, changed) = numbering(&others, anchor.map(|_| slot));
    let mut edits = sibling_edits(root, &changed);
    let created = item.is_dir.then(|| minimal_folder_file(name, Some(moved_seq)));
    match plan(&item.info_file(), item.kind(), &[("seq", Value::Int(moved_seq))], created) {
        Update::Write(edit) => edits.push(edit),
        Update::Keep => {}
        Update::Link => return Err(symlink_error(&item.info_file())),
        Update::Foreign if folder == parent => return Err(ManageError::NotRequest(path.to_owned())),
        Update::Foreign => {}
    }
    commit(root, &edits)?;

    let new_path = if folder == parent { path.to_owned() } else { relocated(&scope, &item, &dest, folder, name)? };
    if new_path != path {
        tracking.moved(path, &new_path, "l'élément est bien déplacé")?;
    }
    Ok(new_path)
}

/// Placer `path` à l'index `slot` parmi `others` donne l'ordre actuel `current` du dossier.
fn keeps_order(current: &[TreeItem], others: &[TreeItem], slot: usize, path: &str) -> bool {
    let mut order: Vec<&str> = others.iter().map(TreeItem::path).collect();
    order.insert(slot, path);
    order.iter().copied().eq(current.iter().map(TreeItem::path))
}

/// `dest` est `folder` ou s'y trouve, liens symboliques résolus.
fn lies_within(dest: &Path, folder: &Path) -> Result<bool, CoreError> {
    let resolve = |path: &Path| fs::canonicalize(path).map_err(|e| CoreError::io(path, e));
    Ok(resolve(dest)?.starts_with(resolve(folder)?))
}

/// Déplace `item` dans `dest` (le dossier relatif `folder`) et renvoie son nouveau chemin relatif.
fn relocated(scope: &Scope, item: &Item, dest: &Path, folder: &str, name: &str) -> Result<String, ManageError> {
    let (stem, ext) = match item.is_dir {
        true => (name, ""),
        false => (name.strip_suffix(REQUEST_EXT).unwrap_or(name), REQUEST_EXT),
    };
    let names = scope.directory(dest, folder.is_empty(), None)?;
    let target = claim_unique(dest, names, Wanted::new(stem, ext).preferring(name), |to| relocate(&item.path, to))?;
    Ok(join(folder, &target))
}
