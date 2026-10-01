//! Requêtes et dossiers : création (`newHttpRequest`, `renderer:new-folder`), renommage, duplication et suppression.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use serde_json::{json, Value as Json};
use xc_core::collection::{
    count_entries, list_folder, with_info, write_atomic, write_new, TreeItem, FOLDER_FILE, REQUEST_EXT,
};
use xc_core::yaml::Value;
use xc_core::CoreError;

use super::copy::copy_tree;
use super::paths::{locate, split, visible, Item};
use super::track::Tracking;
use super::ManageError;
use crate::import::{folder_dir_name, join, request_file_name, Directory};
use crate::stringify;

/// Noms déjà pris dans `dir`, hors `except` (l'élément qu'on renomme, que son propre nom ne gêne pas) ; `listed` :
/// les noms attribués doivent rester visibles de l'arbre (`Some(true)` pour la racine).
pub(super) fn taken_names(dir: &Path, listed: Option<bool>, except: Option<&str>) -> Result<Directory, ManageError> {
    let entries = fs::read_dir(dir).map_err(|e| CoreError::io(dir, e))?;
    let names: Vec<String> = entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    let taken: Vec<&str> = names.iter().map(String::as_str).filter(|name| Some(*name) != except).collect();
    let directory = Directory::new(&taken);
    Ok(match listed {
        Some(at_root) => directory.listed(at_root),
        None => directory,
    })
}

/// Crée dans `dir` un fichier ou un dossier au premier nom libre parmi `radical`, `radical 1`, `radical 2`… avec
/// l'extension `ext` ; `create` ne doit jamais écraser un élément existant. Renvoie le nom retenu.
pub(super) fn create_unique(
    dir: &Path,
    listed: Option<bool>,
    stem: &str,
    ext: &str,
    create: impl Fn(&Path) -> io::Result<()>,
) -> Result<String, ManageError> {
    let mut names = taken_names(dir, listed, None)?;
    loop {
        let name = names.claim(stem, ext);
        match create(&dir.join(&name)) {
            Ok(()) => return Ok(name),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}
            Err(e) => return Err(CoreError::io(&dir.join(&name), e).into()),
        }
    }
}

/// Dossier `folder` (`""` pour la racine) et `seq` d'un nouvel élément : le nombre de dossiers et de requêtes qu'il
/// contient, plus un.
fn destination(root: &Path, folder: &str) -> Result<(PathBuf, usize), ManageError> {
    let dir = visible(root, folder)?;
    let count = count_entries(root, folder)?.ok_or_else(|| ManageError::FolderNotFound(folder.to_owned()))?;
    Ok((dir, count + 1))
}

/// Dossiers et requêtes de `folder` dans l'ordre de l'arbre.
pub(super) fn siblings(root: &Path, folder: &str) -> Result<Vec<TreeItem>, ManageError> {
    list_folder(root, folder)?.ok_or_else(|| ManageError::FolderNotFound(folder.to_owned()))
}

/// `seq` qui place un élément après `siblings` : au-delà du plus grand `seq` et du nombre d'éléments, ce qui ne
/// dépend pas des trous laissés par les suppressions.
pub(super) fn end_seq<'a>(siblings: impl Iterator<Item = &'a TreeItem>) -> i64 {
    let (count, largest) = siblings.fold((0, 0), |(count, largest), s| (count + 1, largest.max(s.seq().unwrap_or(0))));
    count.max(largest) + 1
}

fn blank_request(name: &str, seq: usize) -> Json {
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

/// Crée une requête HTTP vierge `name` dans `folder` (relatif à la racine, `""` pour la racine), avec le `seq` suivant
/// du dossier, et renvoie son chemin relatif avec des `/`. Un nom de fichier pris reçoit un suffixe ` 1`, ` 2`… ;
/// `info.name` garde le nom saisi. Les noms `collection` et `folder` sont refusés.
pub fn create_request(root: &Path, folder: &str, name: &str) -> Result<String, ManageError> {
    let file = request_file_name(name).map_err(ManageError::InvalidName)?;
    let (dir, seq) = destination(root, folder)?;
    let text = stringify::item(&blank_request(name, seq));
    let stem = file.strip_suffix(REQUEST_EXT).unwrap_or(&file);
    let created = create_unique(&dir, Some(folder.is_empty()), stem, REQUEST_EXT, |path| write_new(path, &text))?;
    Ok(join(folder, &created))
}

/// `folder.yml` d'un nouveau dossier : `seq` et authentification héritée.
fn new_folder_file(name: &str, seq: usize) -> String {
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

/// Crée le dossier `name` dans `parent` (relatif à la racine, `""` pour la racine), avec son `folder.yml` et le `seq`
/// suivant du dossier parent, et renvoie son chemin relatif. Un nom pris reçoit un suffixe ` 1`, ` 2`… ; `info.name`
/// garde le nom saisi. Les noms que l'arbre cacherait sont refusés.
pub fn create_folder(root: &Path, parent: &str, name: &str) -> Result<String, ManageError> {
    let at_root = parent.is_empty();
    let stem = folder_dir_name(name, at_root).map_err(ManageError::InvalidName)?;
    let (dir, seq) = destination(root, parent)?;
    let created = create_unique(&dir, Some(at_root), &stem, "", |path| fs::create_dir(path))?;
    let folder = dir.join(&created);
    let file = folder.join(FOLDER_FILE);
    write_new(&file, &new_folder_file(name, seq)).map_err(|e| {
        fs::remove_dir(&folder).ok();
        CoreError::io(&file, e)
    })?;
    Ok(join(parent, &created))
}

/// Fichier qui porte `info` : la requête, ou le `folder.yml` d'un dossier.
pub(super) fn info_file(item: &Item) -> PathBuf {
    if item.is_dir {
        item.path.join(FOLDER_FILE)
    } else {
        item.path.clone()
    }
}

/// Nouveau texte de `file` quand `fields` changent ses champs `info`, `None` sinon. Un `folder.yml` absent prend le
/// texte `created`.
pub(super) fn updated_info(
    file: &Path,
    fields: &[(&str, Value)],
    created: Option<String>,
) -> Result<Option<String>, ManageError> {
    let text = match (fs::read_to_string(file), created) {
        (Ok(text), _) => text,
        (Err(e), Some(created)) if e.kind() == ErrorKind::NotFound => return Ok(Some(created)),
        (Err(e), _) => return Err(CoreError::io(file, e).into()),
    };
    let updated = with_info(file, &text, fields)?;
    Ok((updated != text).then_some(updated))
}

fn set_info(file: &Path, fields: &[(&str, Value)], created: Option<String>) -> Result<(), ManageError> {
    match updated_info(file, fields, created)? {
        Some(text) => Ok(write_atomic(file, &text)?),
        None => Ok(()),
    }
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
/// change, puis l'élément est renommé de façon atomique, avec un suffixe ` n` si le nouveau nom est pris ; si le nom
/// assaini est inchangé, seul `info.name` change. Un dossier sans `folder.yml` en reçoit un, minimal.
pub fn rename_item(root: &Path, path: &str, name: &str) -> Result<String, ManageError> {
    let item = locate(root, path)?;
    let tracking = Tracking::load(root)?;
    let (parent, current) = split(path);
    let at_root = parent.is_empty();
    let (stem, ext) = names_of(&item, name, at_root)?;
    let dir = item.path.parent().unwrap_or(root);
    let target = taken_names(dir, Some(at_root), Some(current))?.claim(&stem, ext);
    let created = item.is_dir.then(|| minimal_folder_file(name, None));
    set_info(&info_file(&item), &[("name", Value::str(name))], created)?;
    if target == current {
        return Ok(path.to_owned());
    }
    fs::rename(&item.path, dir.join(&target)).map_err(|e| CoreError::io(&item.path, e))?;
    let renamed = join(parent, &target);
    tracking.moved(path, &renamed)?;
    Ok(renamed)
}

/// Duplique la requête ou le dossier `path` sous le nom `name`, dans le même dossier, à la fin de la liste, et renvoie
/// le chemin relatif de la copie. Une requête est copiée à l'octet puis seules `info.name` et `info.seq` changent ; un
/// dossier est copié en entier (tous ses fichiers), puis seules `info.name` et `info.seq` de son `folder.yml` changent.
/// La copie n'est pas suivie par la synchro OpenAPI.
pub fn clone_item(root: &Path, path: &str, name: &str) -> Result<String, ManageError> {
    let item = locate(root, path)?;
    let (parent, _) = split(path);
    let at_root = parent.is_empty();
    let (stem, ext) = names_of(&item, name, at_root)?;
    let dir = item.path.parent().unwrap_or(root);
    let seq = end_seq(siblings(root, parent)?.iter());
    let fields = [("name", Value::str(name)), ("seq", Value::Int(seq))];
    let created = if item.is_dir {
        let created = create_unique(dir, Some(at_root), &stem, ext, |copy| fs::create_dir(copy))?;
        let copy = dir.join(&created);
        let finished = copy_tree(&item.path, &copy)
            .and_then(|()| set_info(&copy.join(FOLDER_FILE), &fields, Some(minimal_folder_file(name, Some(seq)))));
        finished.inspect_err(|_| {
            fs::remove_dir_all(&copy).ok();
        })?;
        created
    } else {
        let text = fs::read_to_string(&item.path).map_err(|e| CoreError::io(&item.path, e))?;
        let copy = with_info(&item.path, &text, &fields)?;
        create_unique(dir, Some(at_root), &stem, ext, |file| write_new(file, &copy))?
    };
    Ok(join(parent, &created))
}

/// Envoie la requête ou le dossier `path` à la corbeille avec `trash`. Les frères ne sont pas renumérotés ; les
/// opérations suivies de la synchro OpenAPI deviennent `ignored` : l'équipe ne les veut plus.
pub fn delete_item(root: &Path, path: &str, trash: impl Fn(&Path) -> Result<(), String>) -> Result<(), ManageError> {
    let item = locate(root, path)?;
    let tracking = Tracking::load(root)?;
    trash(&item.path).map_err(ManageError::Trash)?;
    tracking.deleted(path)
}
