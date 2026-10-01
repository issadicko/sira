//! Stockage `.oc-sync/openapi/` : `source.yml` (source, regroupement, opérations suivies) et copie brute de la spec.
//!
//! La base d'une opération n'est pas stockée : elle est recalculée à chaque synchro depuis la copie brute avec le
//! convertisseur courant. `source.yml` ne contient aucun champ volatil (ni date, ni empreinte).

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use xc_core::collection::write_atomic;
use xc_core::pretty::pretty_json;
use xc_core::yaml::{self, emit, Map, Value};
use xc_core::CoreError;

use crate::openapi::GroupBy;

pub const SYNC_DIR: &str = ".oc-sync";
const SOURCE_FILE: &str = "source.yml";
const SPEC_FILES: [&str; 2] = ["spec.json", "spec.yaml"];

/// Opération suivie : sa clé, son fichier de requête et son état.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub key: String,
    pub file: Option<String>,
    pub removed: bool,
    pub ignored: bool,
}

impl Entry {
    pub fn tracked(key: &str, file: &str) -> Self {
        Self { key: key.to_owned(), file: Some(file.to_owned()), removed: false, ignored: false }
    }

    pub fn ignored(key: &str) -> Self {
        Self { key: key.to_owned(), file: None, removed: false, ignored: true }
    }
}

/// Contenu de `source.yml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Store {
    pub source: String,
    pub group_by: GroupBy,
    pub spec: String,
    pub operations: Vec<Entry>,
}

pub fn dir(root: &Path) -> PathBuf {
    root.join(SYNC_DIR).join("openapi")
}

fn invalid(path: &Path, message: impl Into<String>) -> CoreError {
    CoreError::Yaml { path: path.display().to_string(), message: message.into() }
}

/// Nom et contenu de la copie brute : une spec JSON est réindentée (les jetons sont préservés), une spec YAML est
/// copiée octet pour octet.
pub fn spec_copy(text: &str) -> (&'static str, String) {
    match pretty_json(text) {
        Some(pretty) => (SPEC_FILES[0], pretty),
        None => (SPEC_FILES[1], text.to_owned()),
    }
}

/// `source.yml` s'il existe, `None` pour une collection qui n'est pas connectée.
pub fn read(root: &Path) -> Result<Option<Store>, CoreError> {
    let path = dir(root).join(SOURCE_FILE);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(CoreError::io(&path, e)),
    };
    let tree = match yaml::parse(&text).map_err(|e| invalid(&path, e.to_string()))? {
        Value::Map(tree) => tree,
        _ => return Err(invalid(&path, "le document doit être une table")),
    };
    let text_of = |key: &str| {
        tree.get(key)
            .and_then(Value::scalar)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| invalid(&path, format!("« {key} » manquant")))
    };
    let group_by = text_of("groupBy")?.parse::<GroupBy>().map_err(|e| invalid(&path, e.to_string()))?;
    let spec = text_of("spec")?;
    if spec.contains(['/', '\\']) || spec.starts_with('.') {
        return Err(invalid(&path, format!("« spec » doit être un nom de fichier : {spec}")));
    }
    let operations = tree.seq("operations").iter().map(|op| entry(&path, op)).collect::<Result<_, _>>()?;
    Ok(Some(Store { source: text_of("source")?, group_by, spec, operations }))
}

fn entry(path: &Path, value: &Value) -> Result<Entry, CoreError> {
    let map = value.as_map().ok_or_else(|| invalid(path, "une opération doit être une table"))?;
    let key = map.get("key").and_then(Value::scalar).filter(|k| !k.is_empty());
    let key = key.ok_or_else(|| invalid(path, "opération sans « key »"))?;
    let flag = |name: &str| map.get(name).is_some_and(Value::is_true);
    let entry = Entry {
        file: map.get("file").and_then(Value::scalar).filter(|f| !f.is_empty()),
        removed: flag("removed"),
        ignored: flag("ignored"),
        key,
    };
    if entry.file.is_none() && !entry.ignored {
        return Err(invalid(path, format!("l'opération « {} » n'a ni « file » ni « ignored »", entry.key)));
    }
    Ok(entry)
}

/// Copie brute de la spec de la base, telle que `source.yml` la désigne.
pub fn read_spec(root: &Path, store: &Store) -> Result<String, CoreError> {
    let path = dir(root).join(&store.spec);
    fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))
}

/// Fichiers de requête que `source.yml` marque comme retirés de la spec (chemins relatifs, avec des `/`).
pub fn removed_files(root: &Path) -> HashSet<String> {
    let store = read(root).ok().flatten();
    let entries = store.into_iter().flat_map(|s| s.operations);
    entries.filter(|e| e.removed).filter_map(|e| e.file).collect()
}

fn write_if_changed(path: &Path, text: &str) -> Result<(), CoreError> {
    if fs::read_to_string(path).is_ok_and(|current| current == text) {
        return Ok(());
    }
    write_atomic(path, text)
}

/// Écrit la copie brute de la spec, puis `source.yml` en dernier : une écriture interrompue avant la fin laisse la
/// base d'avant intacte, et la synchro se rejoue sans dégât.
pub fn write(
    root: &Path,
    source: &str,
    group_by: GroupBy,
    operations: &[Entry],
    spec_text: &str,
) -> Result<(), CoreError> {
    let dir = dir(root);
    fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    let (spec, content) = spec_copy(spec_text);
    write_if_changed(&dir.join(spec), &content)?;
    let document = Map(vec![
        ("source".into(), Value::str(source)),
        ("groupBy".into(), Value::str(group_by.as_str())),
        ("spec".into(), Value::str(spec)),
        ("operations".into(), Value::Seq(operations.iter().map(entry_value).collect())),
    ]);
    write_if_changed(&dir.join(SOURCE_FILE), &emit(&Value::Map(document), &[]))?;
    for stale in SPEC_FILES.iter().filter(|name| **name != spec) {
        fs::remove_file(dir.join(stale)).ok();
    }
    Ok(())
}

fn entry_value(entry: &Entry) -> Value {
    let mut pairs = vec![("key".to_owned(), Value::str(&entry.key))];
    if let Some(file) = &entry.file {
        pairs.push(("file".into(), Value::str(file)));
    }
    for (name, set) in [("removed", entry.removed), ("ignored", entry.ignored)] {
        if set {
            pairs.push((name.into(), Value::Bool(true)));
        }
    }
    Value::Map(Map(pairs))
}
