//! Lecture des opérations d'une spec (converties comme à l'import) et des fichiers de requête de l'équipe.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde_json::Value;
use xc_core::collection::resolve_visible_path;
use xc_core::yaml::{self, Map};
use xc_core::{CoreError, RequestDoc};

use super::SyncError;
use crate::import::{folder_file, text, REQUEST_TYPES};
use crate::openapi::{to_bruno, GroupBy};
use crate::stringify;

/// Dossier de tag (ou de chemin) qui contient une opération, avec son `folder.yml`.
#[derive(Clone)]
pub(super) struct Folder {
    pub name: String,
    pub file: Option<String>,
}

/// Opération d'une spec convertie : son item JSON, le fichier YAML qui en découle et sa vue typée.
pub(super) struct Op {
    pub key: String,
    pub item: Value,
    pub folders: Vec<Folder>,
    pub text: String,
    pub doc: RequestDoc,
}

/// Fichier de requête de l'équipe tel qu'il est sur le disque ; `is_request` : son `info.type` est bien `http`.
pub(super) struct Ours {
    pub file: String,
    pub text: String,
    pub doc: RequestDoc,
    pub is_request: bool,
}

pub(super) fn tree(text: &str, path: &str) -> Result<Map, CoreError> {
    match yaml::parse(text).map_err(|e| CoreError::Yaml { path: path.to_owned(), message: e.to_string() })? {
        yaml::Value::Map(tree) => Ok(tree),
        _ => Err(CoreError::Yaml { path: path.to_owned(), message: "le document doit être une table".into() }),
    }
}

/// Les opérations de la spec, dans son ordre, converties avec le regroupement donné.
pub(super) fn operations(spec: &Value, group_by: GroupBy) -> Result<Vec<Op>, SyncError> {
    let collection = to_bruno(spec, group_by)?;
    let mut out = Vec::new();
    walk(collection.get("items"), &mut Vec::new(), &mut out)?;
    Ok(out)
}

fn walk(items: Option<&Value>, folders: &mut Vec<Folder>, out: &mut Vec<Op>) -> Result<(), SyncError> {
    for item in items.and_then(Value::as_array).into_iter().flatten() {
        let kind = text(item, "type");
        if kind == "folder" {
            folders.push(Folder { name: text(item, "name").to_owned(), file: folder_file(item) });
            walk(item.get("items"), folders, out)?;
            folders.pop();
        } else if let (true, Some(key)) =
            (REQUEST_TYPES.contains(&kind), item.get("operationKey").and_then(Value::as_str))
        {
            let rendered = stringify::item(item);
            let doc = RequestDoc::from_tree(&tree(&rendered, key)?);
            out.push(Op { key: key.to_owned(), item: item.clone(), folders: folders.clone(), text: rendered, doc });
        }
    }
    Ok(())
}

/// Le fichier de requête `file` de la collection, ou `None` s'il n'existe pas ; un chemin qui sort de la collection
/// ou passe par un élément caché est refusé.
pub(super) fn load_ours(root: &Path, file: &str) -> Result<Option<Ours>, SyncError> {
    let path = resolve_visible_path(root, file)?;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == ErrorKind::NotFound || !path.is_file() => return Ok(None),
        Err(e) => return Err(CoreError::io(&path, e).into()),
    };
    let unreadable = |e: CoreError| match e {
        CoreError::Yaml { message, .. } => SyncError::Unreadable { file: file.to_owned(), message },
        other => other.into(),
    };
    let tree = tree(&text, file).map_err(unreadable)?;
    let is_request = tree.map("info").and_then(|info| info.str("type")) == Some("http");
    Ok(Some(Ours { file: file.to_owned(), text, doc: RequestDoc::from_tree(&tree), is_request }))
}

pub(super) fn exists(root: &Path, file: &str) -> Result<bool, SyncError> {
    Ok(resolve_visible_path(root, file)?.is_file())
}
