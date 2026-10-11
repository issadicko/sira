use std::fs;
use std::path::Path;

use super::{
    ensure_collection, parse_exact, read_tree, resolve_path, styled_like, write_atomic, COLLECTION_FILE,
    COLLECTION_ORDER, FOLDER_FILE,
};
use crate::request::{BLANK_BEFORE, INFO_ORDER, TOP_ORDER};
use crate::yaml::{self, Map, Value};
use crate::CoreError;

const MARKDOWN: &str = "text/markdown";

/// Texte de la clé `docs` : un texte seul, ou une table `{ content, type }` comme l'écrit OpenCollection.
pub fn docs_text(value: &Value) -> Option<String> {
    match value {
        Value::Map(table) => table.get("content").and_then(Value::scalar),
        other => other.scalar(),
    }
}

/// Remplace `docs` dans `tree` en gardant sa forme (texte seul ou table, type compris) ; un texte vide retire la clé.
/// Sans `docs` existant, `as_table` choisit la forme écrite.
pub(crate) fn set_docs(tree: &mut Map, text: &str, order: &[&str], as_table: bool) {
    if text.is_empty() {
        tree.remove("docs");
        return;
    }
    if let Some(Value::Map(table)) = tree.get_mut("docs") {
        table.set("content", Value::str(text), &["content", "type"]);
        if table.get("type").is_none() {
            table.set("type", Value::str(MARKDOWN), &["content", "type"]);
        }
        return;
    }
    let value = match (tree.get("docs"), as_table) {
        (Some(_), _) | (None, false) => Value::str(text),
        (None, true) => {
            let mut table = Map::default();
            table.set("content", Value::str(text), &["content", "type"]);
            table.set("type", Value::str(MARKDOWN), &["content", "type"]);
            Value::Map(table)
        }
    };
    tree.set("docs", value, order);
}

/// Documentation Markdown de la collection (`folder` vide) ou du dossier `folder` ; vide quand il n'y en a pas.
pub fn read_docs(root: &Path, folder: &str) -> Result<String, CoreError> {
    ensure_collection(root)?;
    let file = docs_file(root, folder)?;
    if !file.is_file() {
        return Ok(String::new());
    }
    Ok(read_tree(&file)?.get("docs").and_then(docs_text).unwrap_or_default())
}

/// Enregistre la documentation Markdown de la collection (`folder` vide) ou du dossier `folder`. Renvoie `false`
/// quand le fichier n'a pas changé. Le reste du fichier est conservé ; un texte vide retire la clé `docs`, et le
/// `folder.yml` d'un dossier qui n'en avait pas est créé avec son nom.
pub fn save_docs(root: &Path, folder: &str, text: &str) -> Result<bool, CoreError> {
    ensure_collection(root)?;
    let file = docs_file(root, folder)?;
    let order = if folder.is_empty() { COLLECTION_ORDER } else { TOP_ORDER };
    if !file.is_file() {
        if text.is_empty() {
            return Ok(false);
        }
        let mut tree = Map::default();
        let mut info = Map::default();
        let name = folder.rsplit('/').next().unwrap_or(folder);
        info.set("name", Value::str(name), INFO_ORDER);
        info.set("type", Value::str("folder"), INFO_ORDER);
        tree.set("info", Value::Map(info), order);
        set_docs(&mut tree, text, order, true);
        write_atomic(root, &file, &yaml::emit(&Value::Map(tree), BLANK_BEFORE))?;
        return Ok(true);
    }
    let current = fs::read_to_string(&file).map_err(|e| CoreError::io(&file, e))?;
    let mut tree = parse_exact(&current, &file)?;
    let before = tree.get("docs").and_then(docs_text).unwrap_or_default();
    if before == text {
        return Ok(false);
    }
    set_docs(&mut tree, text, order, true);
    let next = styled_like(&current, yaml::emit(&Value::Map(tree), BLANK_BEFORE));
    if next == current {
        return Ok(false);
    }
    write_atomic(root, &file, &next)?;
    Ok(true)
}

fn docs_file(root: &Path, folder: &str) -> Result<std::path::PathBuf, CoreError> {
    match folder.is_empty() {
        true => Ok(root.join(COLLECTION_FILE)),
        false => resolve_path(root, &format!("{folder}/{FOLDER_FILE}")),
    }
}
