//! Requêtes et dossiers : création (`newHttpRequest`, `renderer:new-folder`), renommage, duplication et suppression.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value as Json};
use xc_core::collection::{list_folder, write_new, TreeItem, FOLDER_FILE, REQUEST_EXT};
use xc_core::yaml::Value;

use super::copy::{copy_file, copy_tree};
use super::exclusive::rename_new;
use super::info::{commit, plan, symlink_error, Kind, Update};
use super::names::{claim_unique, Wanted};
use super::paths::{split, Item, Scope};
use super::track::Tracking;
use super::{io_error, ManageError};
use crate::import::{folder_dir_name, join, request_file_name};
use crate::stringify;

/// Dossiers et requêtes de `folder` dans l'ordre de l'arbre.
pub(super) fn siblings(root: &Path, folder: &str) -> Result<Vec<TreeItem>, ManageError> {
    list_folder(root, folder)?.ok_or_else(|| ManageError::FolderNotFound(folder.to_owned()))
}

/// `seq` qui place un élément après `siblings` : au-delà du plus grand `seq` et du nombre d'éléments, ce qui ne
/// dépend pas des trous laissés par les suppressions.
pub(super) fn end_seq<'a>(siblings: impl Iterator<Item = &'a TreeItem>) -> i64 {
    let (count, largest) =
        siblings.fold((0_i64, 0), |(count, largest), s| (count.saturating_add(1), largest.max(s.seq().unwrap_or(0))));
    count.max(largest).saturating_add(1)
}

/// Dossier `folder` (`""` pour la racine) et `seq` d'un nouvel élément : la fin de sa liste.
fn destination(scope: &Scope, folder: &str) -> Result<(PathBuf, i64), ManageError> {
    let dir = scope.visible(folder)?;
    Ok((dir, end_seq(siblings(scope.root, folder)?.iter())))
}

fn blank_request(name: &str, seq: i64) -> Json {
    json!({
        "type": "http-request",
        "name": name,
        "seq": seq,
        "request": {
            "method": "GET",
            "url": "",
            "headers": [],
            "params": [],
            "body": {
                "mode": "none",
                "json": null,
                "text": null,
                "xml": null,
                "sparql": null,
                "multipartForm": [],
                "formUrlEncoded": [],
                "file": []
            },
            "vars": { "req": [], "res": [] },
            "assertions": [],
            "auth": { "mode": "inherit" }
        },
        "settings": { "encodeUrl": true, "forwardAuthorizationHeader": false }
    })
}

/// Crée une requête HTTP vierge `name` dans `folder` (relatif à la racine, `""` pour la racine), à la fin de la liste
/// du dossier, et renvoie son chemin relatif avec des `/`. Un nom de fichier pris reçoit un suffixe ` 1`, ` 2`… ;
/// `info.name` garde le nom saisi. Les noms `collection` et `folder`, et ceux de `extensions.bruno.ignore`, sont
/// refusés.
pub fn create_request(root: &Path, folder: &str, name: &str) -> Result<String, ManageError> {
    let file = request_file_name(name).map_err(ManageError::InvalidName)?;
    let scope = Scope::open(root)?;
    let (dir, seq) = destination(&scope, folder)?;
    scope.check_name(&file)?;
    let text = stringify::item(&blank_request(name, seq));
    let stem = file.strip_suffix(REQUEST_EXT).unwrap_or(&file);
    let names = scope.directory(&dir, folder.is_empty(), None)?;
    let created = claim_unique(&dir, names, Wanted::new(stem, REQUEST_EXT), |path| {
        write_new(path, &text).map_err(|e| io_error(path, e))
    })?;
    Ok(join(folder, &created))
}

/// `folder.yml` d'un nouveau dossier : `seq` et authentification héritée.
fn new_folder_file(name: &str, seq: i64) -> String {
    let root = json!({ "meta": { "name": name, "seq": seq }, "request": { "auth": { "mode": "inherit" } } });
    stringify::folder(&root)
}

/// `folder.yml` minimal d'un dossier qui n'en avait pas (E:1188-1207) : son nom, et son `seq` s'il en reçoit un.
pub(super) fn minimal_folder_file(name: &str, seq: Option<i64>) -> String {
    let mut meta = json!({ "name": name });
    if let Some(seq) = seq {
        meta["seq"] = json!(seq);
    }
    stringify::folder(&json!({ "meta": meta }))
}

/// Crée le dossier `name` dans `parent` (relatif à la racine, `""` pour la racine), avec son `folder.yml` et un `seq`
/// à la fin de la liste du dossier parent, et renvoie son chemin relatif. Un nom pris reçoit un suffixe ` 1`, ` 2`… ;
/// `info.name` garde le nom saisi. Les noms que l'arbre cacherait ou que `extensions.bruno.ignore` retire sont refusés.
pub fn create_folder(root: &Path, parent: &str, name: &str) -> Result<String, ManageError> {
    let at_root = parent.is_empty();
    let stem = folder_dir_name(name, at_root).map_err(ManageError::InvalidName)?;
    let scope = Scope::open(root)?;
    let (dir, seq) = destination(&scope, parent)?;
    scope.check_name(&stem)?;
    let names = scope.directory(&dir, at_root, None)?;
    let created =
        claim_unique(&dir, names, Wanted::new(&stem, ""), |path| fs::create_dir(path).map_err(|e| io_error(path, e)))?;
    let folder = dir.join(&created);
    let file = folder.join(FOLDER_FILE);
    write_new(&file, &new_folder_file(name, seq)).map_err(|e| {
        fs::remove_dir(&folder).ok();
        io_error(&file, e)
    })?;
    Ok(join(parent, &created))
}

/// Radical et extension du nom de l'élément `name` : le nom du fichier d'une requête, ou celui du dossier.
fn names_of(item: &Item, name: &str, at_root: bool) -> Result<(String, &'static str), ManageError> {
    if item.is_dir {
        return Ok((folder_dir_name(name, at_root).map_err(ManageError::InvalidName)?, ""));
    }
    let file = request_file_name(name).map_err(ManageError::InvalidName)?;
    Ok((file.strip_suffix(REQUEST_EXT).unwrap_or(&file).to_owned(), REQUEST_EXT))
}

/// Renomme la requête ou le dossier `path` en `name` et renvoie son nouveau chemin relatif. Seule la ligne `info.name`
/// change (dans une requête ou dans le `folder.yml` d'un dossier, créé minimal s'il manque), puis l'élément est
/// renommé de façon atomique et sans jamais remplacer un élément existant, avec un suffixe ` n` si le nouveau nom est
/// pris ; si le nom assaini est inchangé, seul `info.name` change. Un fichier illisible, ou qui n'est pas une requête,
/// est renommé sans que son contenu soit touché ; un lien symbolique est refusé.
pub fn rename_item(root: &Path, path: &str, name: &str) -> Result<String, ManageError> {
    let scope = Scope::open(root)?;
    let item = scope.locate(path)?;
    item.refuse_link()?;
    let tracking = Tracking::load(root)?;
    let (parent, current) = split(path);
    let at_root = parent.is_empty();
    let (stem, ext) = names_of(&item, name, at_root)?;
    scope.check_name(&format!("{stem}{ext}"))?;
    let dir = item.path.parent().unwrap_or(root);
    let created = item.is_dir.then(|| minimal_folder_file(name, None));
    match plan(&item.info_file(), item.kind(), &[("name", Value::str(name))], created) {
        Update::Write(edit) => commit(root, &[edit])?,
        Update::Link => return Err(symlink_error(&item.info_file())),
        Update::Keep | Update::Foreign => {}
    }
    let names = scope.directory(dir, at_root, Some(current))?;
    let target = claim_unique(dir, names, Wanted::new(&stem, ext), |target| match target == item.path {
        true => Ok(()),
        false => rename_new(&item.path, target).map_err(|e| io_error(&item.path, e)),
    })?;
    if target == current {
        return Ok(path.to_owned());
    }
    let renamed = join(parent, &target);
    tracking.moved(path, &renamed, "l'élément est bien renommé")?;
    Ok(renamed)
}

/// Duplique la requête ou le dossier `path` sous le nom `name`, dans le même dossier, à la fin de la liste, et renvoie
/// le chemin relatif de la copie. Une requête est copiée à l'octet puis seules `info.name` et `info.seq` changent (un
/// fichier qui n'est pas une requête est copié tel quel) ; un dossier est copié en entier (tous ses fichiers, sans
/// jamais remplacer un fichier existant), puis seules `info.name` et `info.seq` de son `folder.yml` changent. La copie
/// n'est pas suivie par la synchro OpenAPI ; un lien symbolique est refusé.
pub fn clone_item(root: &Path, path: &str, name: &str) -> Result<String, ManageError> {
    let scope = Scope::open(root)?;
    let item = scope.locate(path)?;
    item.refuse_link()?;
    let (parent, _) = split(path);
    let at_root = parent.is_empty();
    let (stem, ext) = names_of(&item, name, at_root)?;
    scope.check_name(&format!("{stem}{ext}"))?;
    let dir = item.path.parent().unwrap_or(root);
    let seq = end_seq(siblings(root, parent)?.iter());
    let fields = [("name", Value::str(name)), ("seq", Value::Int(seq))];
    let names = scope.directory(dir, at_root, None)?;
    let wanted = Wanted::new(&stem, ext);
    let created = if item.is_dir {
        let created = claim_unique(dir, names, wanted, |copy| fs::create_dir(copy).map_err(|e| io_error(copy, e)))?;
        let copy = dir.join(&created);
        let finished = copy_tree(&item.path, &copy).and_then(|()| {
            let file = copy.join(FOLDER_FILE);
            match plan(&file, Kind::Folder, &fields, Some(minimal_folder_file(name, Some(seq)))) {
                Update::Write(edit) => commit(root, &[edit]),
                Update::Link => Err(symlink_error(&file)),
                Update::Keep | Update::Foreign => Ok(()),
            }
        });
        finished.inspect_err(|_| {
            fs::remove_dir_all(&copy).ok();
        })?;
        created
    } else {
        let text = match plan(&item.path, Kind::Request, &fields, None) {
            Update::Write(edit) => Some(edit.into_text()),
            Update::Link => return Err(symlink_error(&item.path)),
            Update::Keep | Update::Foreign => None,
        };
        claim_unique(dir, names, wanted, |copy| match &text {
            Some(text) => write_new(copy, text).map_err(|e| io_error(copy, e)),
            None => copy_file(&item.path, copy),
        })?
    };
    Ok(join(parent, &created))
}

/// Envoie la requête ou le dossier `path` à la corbeille avec `trash`. Les frères ne sont pas renumérotés ; les
/// opérations suivies de la synchro OpenAPI deviennent `ignored` : l'équipe ne les veut plus. `source.yml`, lu avant
/// la corbeille, est écrit aussitôt après ; si cette écriture échoue, l'erreur dit que l'élément est bien dans la
/// corbeille.
pub fn delete_item(root: &Path, path: &str, trash: impl Fn(&Path) -> Result<(), String>) -> Result<(), ManageError> {
    let scope = Scope::open(root)?;
    let item = scope.locate(path)?;
    let tracking = Tracking::load(root)?;
    trash(&item.path).map_err(ManageError::Trash)?;
    tracking.deleted(path)
}
