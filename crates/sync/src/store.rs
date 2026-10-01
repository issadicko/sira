//! Stockage `.oc-sync/openapi/` : `source.yml` (source, regroupement, opérations suivies) et copie brute de la spec.
//!
//! La base d'une opération n'est pas stockée : elle est recalculée à chaque synchro depuis la copie brute avec le
//! convertisseur courant, sauf pour une opération retirée de la spec, dont la dernière version est gardée dans
//! `removed/` le temps qu'elle reste retirée. `source.yml` ne contient aucun champ volatil (ni date, ni empreinte).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};

use xc_core::collection::{is_hidden, write_atomic};
use xc_core::pretty::pretty_json;
use xc_core::yaml::{self, emit, Map, Value};
use xc_core::CoreError;

use crate::import::{fit, is_device_name};
use crate::openapi::GroupBy;

pub const SYNC_DIR: &str = ".oc-sync";
const SOURCE_FILE: &str = "source.yml";
const SPEC_FILES: [&str; 2] = ["spec.json", "spec.yaml"];
const REMOVED_DIR: &str = "removed";
const REMOVED_EXT: &str = ".yml";

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
    let operations: Vec<Entry> = tree.seq("operations").iter().map(|op| entry(&path, op)).collect::<Result<_, _>>()?;
    distinct_files(&path, &operations)?;
    Ok(Some(Store { source: text_of("source")?, group_by, spec, operations }))
}

/// Un fichier n'est suivi que par une entrée : deux entrées sur le même fichier lui feraient subir deux mises à jour,
/// dont une seule survivrait.
fn distinct_files(path: &Path, operations: &[Entry]) -> Result<(), CoreError> {
    let mut seen: HashMap<String, &str> = HashMap::new();
    for entry in operations.iter().filter(|e| !e.ignored) {
        let Some(file) = &entry.file else { continue };
        if let Some(other) = seen.insert(file_id(file), &entry.key) {
            return Err(invalid(path, format!("« {other} » et « {} » désignent le même fichier : {file}", entry.key)));
        }
    }
    Ok(())
}

fn file_id(file: &str) -> String {
    let names: Vec<_> = Path::new(file).components().filter(|c| !matches!(c, Component::CurDir)).collect();
    names.iter().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect::<Vec<_>>().join("/")
}

/// Le fichier d'une entrée est une requête que l'arbre montre : ni hors de la collection, ni caché, ni réservé
/// (`opencollection.yml`, `folder.yml`, `environments`…), à aucun niveau du chemin.
fn check_file(path: &Path, file: &str) -> Result<(), CoreError> {
    let mut at_root = true;
    for component in Path::new(file).components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(name) if !is_hidden(&name.to_string_lossy(), at_root) => at_root = false,
            _ => {
                return Err(invalid(path, format!("chemin refusé (hors de la collection, caché ou réservé) : {file}")))
            }
        }
    }
    Ok(())
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
    if let Some(file) = &entry.file {
        check_file(path, file)?;
    }
    Ok(entry)
}

/// Copie brute de la spec de la base, telle que `source.yml` la désigne ; `None` si le fichier a disparu.
pub fn read_spec(root: &Path, store: &Store) -> Result<Option<String>, CoreError> {
    let path = dir(root).join(&store.spec);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CoreError::io(&path, e)),
    }
}

/// Nom du fichier qui garde la dernière version de l'opération `key` : la clé, dont les caractères interdits dans un
/// nom de fichier sont écrits `%XX`, ce qui ne confond jamais deux clés.
fn removed_name(key: &str) -> String {
    let mut name = String::new();
    for c in key.chars() {
        let plain = c.is_alphanumeric() || " _-.(){}[]#,=+@!~'".contains(c);
        if plain && !(name.is_empty() && c == '.') {
            name.push(c);
        } else {
            let mut utf8 = [0; 4];
            c.encode_utf8(&mut utf8).bytes().for_each(|byte| write!(name, "%{byte:02X}").unwrap_or_default());
        }
    }
    let name = if is_device_name(&name) { format!("_{name}") } else { name };
    fit(&name, "", REMOVED_EXT)
}

fn removed_path(root: &Path, key: &str) -> PathBuf {
    dir(root).join(REMOVED_DIR).join(removed_name(key))
}

/// Dernière version de la requête de l'opération `key`, gardée quand la spec l'a retirée.
pub fn read_removed(root: &Path, key: &str) -> Option<String> {
    fs::read_to_string(removed_path(root, key)).ok()
}

/// Garde la dernière version de la requête de l'opération `key` que la spec vient de retirer.
pub fn write_removed(root: &Path, key: &str, text: &str) -> Result<(), CoreError> {
    let path = removed_path(root, key);
    let parent = path.parent().unwrap_or(&path);
    if parent.is_symlink() {
        return Err(invalid(parent, "un lien symbolique n'est pas suivi"));
    }
    fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
    write_if_changed(&path, text)
}

/// Supprime les versions gardées des opérations qui ne sont plus retirées de la spec (restaurées, rapprochées ou
/// oubliées). Seuls les fichiers `.yml` du dossier sont touchés, jamais un lien ni un dossier.
fn prune_removed(root: &Path, operations: &[Entry]) {
    let dir = dir(root).join(REMOVED_DIR);
    if dir.is_symlink() {
        return;
    }
    let wanted: HashSet<String> = operations.iter().filter(|e| e.removed).map(|e| removed_name(&e.key)).collect();
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_copy = name.ends_with(REMOVED_EXT) && entry.file_type().is_ok_and(|kind| kind.is_file());
        if is_copy && !wanted.contains(&name) {
            fs::remove_file(entry.path()).ok();
        }
    }
}

/// Fichiers de requête que `source.yml` marque comme retirés de la spec (chemins relatifs, avec des `/`).
pub fn removed_files(root: &Path) -> HashSet<String> {
    let store = read(root).ok().flatten();
    let entries = store.into_iter().flat_map(|s| s.operations);
    entries.filter(|e| e.removed).filter_map(|e| e.file).collect()
}

/// Un lien symbolique est remplacé par le fichier, jamais suivi : le stockage ne doit pas écrire hors de lui.
fn write_if_changed(path: &Path, text: &str) -> Result<(), CoreError> {
    if path.is_symlink() {
        fs::remove_file(path).map_err(|e| CoreError::io(path, e))?;
    }
    if fs::read_to_string(path).is_ok_and(|current| current == text) {
        return Ok(());
    }
    write_atomic(path, text)
}

/// Écrit la copie brute de la spec, puis `source.yml` en dernier : une écriture interrompue avant la fin laisse la
/// base d'avant intacte, et la synchro se rejoue sans dégât. Les versions gardées des opérations qui ne sont plus
/// retirées sont ensuite supprimées.
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
    write_if_changed(&dir.join(SOURCE_FILE), &source_document(source, group_by, spec, operations))?;
    for stale in SPEC_FILES.iter().filter(|name| **name != spec) {
        fs::remove_file(dir.join(stale)).ok();
    }
    prune_removed(root, operations);
    Ok(())
}

/// Réécrit `source.yml` avec de nouvelles `operations` (renommage, déplacement ou suppression d'une requête suivie),
/// sans toucher à la copie brute de la spec.
pub fn write_operations(root: &Path, store: &Store, operations: &[Entry]) -> Result<(), CoreError> {
    let document = source_document(&store.source, store.group_by, &store.spec, operations);
    write_if_changed(&dir(root).join(SOURCE_FILE), &document)?;
    prune_removed(root, operations);
    Ok(())
}

fn source_document(source: &str, group_by: GroupBy, spec: &str, operations: &[Entry]) -> String {
    let document = Map(vec![
        ("source".into(), Value::str(source)),
        ("groupBy".into(), Value::str(group_by.as_str())),
        ("spec".into(), Value::str(spec)),
        ("operations".into(), Value::Seq(operations.iter().map(entry_value).collect())),
    ]);
    emit(&Value::Map(document), &[])
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
