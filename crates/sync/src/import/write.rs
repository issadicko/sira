//! Écriture d'une collection importée (`renderer:import-collection` de Bruno, format YAML) et de son snapshot.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{json, Value};
use xc_core::collection::{COLLECTION_FILE, ENV_DIR, FOLDER_FILE, REQUEST_EXT};
use xc_core::yaml::{emit, Map, Value as Yaml};
use xc_core::CoreError;

use super::naming::{collection_folder, stem, Directory};
use super::source::source_value;
use super::{join, text, ImportError};
use crate::openapi::GroupBy;
use crate::stringify;

const SYNC_DIR: &str = ".oc-sync";
const REQUEST_TYPES: [&str; 2] = ["http-request", "graphql-request"];

struct Operation {
    key: String,
    file: String,
    content: String,
}

/// Écrit sans fichier temporaire ni synchronisation : le dossier de préparation n'est publié que par son renommage.
fn write(path: &Path, text: &str) -> Result<(), ImportError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| CoreError::io(parent, e))?;
    }
    Ok(fs::write(path, text).map_err(|e| CoreError::io(path, e))?)
}

/// Écrit la collection JSON de Bruno (`openApiToBruno`) dans un nouveau dossier de `location` et renvoie sa racine.
///
/// Même ordre que Bruno : `opencollection.yml`, les éléments, `environments/`, puis, en dernier, le snapshot
/// `.oc-sync/openapi/`. Seul écart de contenu : `.oc-sync` rejoint la liste `ignore` de `opencollection.yml`.
/// `source` est l'URL ou le chemin de la spec, `group_by` le regroupement utilisé.
/// Tout s'écrit dans un dossier de préparation caché de `location`, renommé à la fin : une écriture qui échoue ne
/// laisse ni collection partielle ni dossier de préparation.
pub fn write_collection(
    collection: &Value,
    location: &Path,
    source: &str,
    group_by: GroupBy,
) -> Result<PathBuf, ImportError> {
    if !location.is_dir() {
        return Err(ImportError::Location(location.display().to_string()));
    }
    let location = std::path::absolute(location).map_err(|e| CoreError::io(location, e))?;
    let (folder, name) = collection_folder(text(collection, "name"), |folder| location.join(folder).exists());
    let staging = location.join(staging_name());
    fs::create_dir(&staging).map_err(|e| CoreError::io(&staging, e))?;
    let root = location.join(folder);
    let imported = build(collection, &staging, &name, source, group_by)
        .and_then(|()| fs::rename(&staging, &root).map_err(|e| CoreError::io(&root, e).into()));
    if imported.is_err() {
        fs::remove_dir_all(&staging).ok();
    }
    imported.map(|()| root)
}

/// Nom du dossier de préparation, indépendant de celui de la collection qui peut occuper les 255 octets permis.
fn staging_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(".xc-import-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

fn build(collection: &Value, root: &Path, name: &str, source: &str, group_by: GroupBy) -> Result<(), ImportError> {
    let config = json!({
        "name": name,
        "type": "collection",
        "ignore": ["node_modules", ".git", SYNC_DIR],
        "opencollection": "1.0.0"
    });
    let root_config = collection.get("root").unwrap_or(&Value::Null);
    write(&root.join(COLLECTION_FILE), &stringify::collection(root_config, &config))?;

    let mut walk = Walk::default();
    let mut names = Directory::new(&[COLLECTION_FILE, FOLDER_FILE, ENV_DIR, SYNC_DIR]).listed(true);
    walk.items(collection.get("items"), root, "", &mut names)?;
    environments(collection.get("environments"), root)?;
    snapshot(root, &source_value(source, root), group_by, &walk.operations)
}

#[derive(Default)]
struct Walk {
    operations: Vec<Operation>,
}

impl Walk {
    fn items(
        &mut self,
        items: Option<&Value>,
        dir: &Path,
        relative: &str,
        names: &mut Directory,
    ) -> Result<(), ImportError> {
        for item in items.and_then(Value::as_array).into_iter().flatten() {
            match text(item, "type") {
                kind if REQUEST_TYPES.contains(&kind) => self.request(item, dir, relative, names)?,
                "folder" => self.folder(item, dir, relative, names)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn request(&mut self, item: &Value, dir: &Path, relative: &str, names: &mut Directory) -> Result<(), ImportError> {
        let file = names.claim(&stem(text(item, "name"), REQUEST_EXT, "Untitled Request"), REQUEST_EXT);
        let content = stringify::item(item);
        write(&dir.join(&file), &content)?;
        if let Some(key) = item.get("operationKey").and_then(Value::as_str) {
            self.operations.push(Operation { key: key.to_owned(), file: join(relative, &file), content });
        }
        Ok(())
    }

    fn folder(&mut self, item: &Value, dir: &Path, relative: &str, names: &mut Directory) -> Result<(), ImportError> {
        let name = names.claim(&stem(text(item, "name"), "", "Untitled Folder"), "");
        let path = dir.join(&name);
        fs::create_dir_all(&path).map_err(|e| CoreError::io(&path, e))?;
        let has_name = item.pointer("/root/meta/name").and_then(Value::as_str).is_some_and(|n| !n.is_empty());
        if let (true, Some(folder_root)) = (has_name, item.get("root")) {
            let mut folder_root = folder_root.clone();
            folder_root["meta"]["seq"] = item.get("seq").cloned().unwrap_or(Value::Null);
            write(&path.join(FOLDER_FILE), &stringify::folder(&folder_root))?;
        }
        self.items(item.get("items"), &path, &join(relative, &name), &mut Directory::new(&[FOLDER_FILE]).listed(false))
    }
}

fn environments(environments: Option<&Value>, root: &Path) -> Result<(), ImportError> {
    let dir = root.join(ENV_DIR);
    fs::create_dir_all(&dir).map_err(|e| CoreError::io(&dir, e))?;
    let mut names = Directory::new(&[]);
    for env in environments.and_then(Value::as_array).into_iter().flatten() {
        let file = names.claim(&stem(text(env, "name"), REQUEST_EXT, "Untitled Environment"), REQUEST_EXT);
        write(&dir.join(file), &stringify::environment(env))?;
    }
    Ok(())
}

fn entry(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(Map(pairs.iter().map(|(key, value)| ((*key).to_owned(), Yaml::str(*value))).collect()))
}

/// `.oc-sync/openapi/` : `base/<base>.yml` contient le fichier de requête tel qu'écrit à l'import, `source.yml`
/// la source, le regroupement et la liste ordonnée des opérations. `source.yml` est écrit après les bases.
fn snapshot(root: &Path, source: &str, group_by: GroupBy, operations: &[Operation]) -> Result<(), ImportError> {
    let dir = root.join(SYNC_DIR).join("openapi");
    let mut names = Directory::new(&[]);
    let mut listed = Vec::new();
    for operation in operations {
        let file = names.claim(&stem(&operation.key, REQUEST_EXT, "operation"), REQUEST_EXT);
        write(&dir.join("base").join(&file), &operation.content)?;
        let base = file.strip_suffix(REQUEST_EXT).unwrap_or(&file);
        listed.push(entry(&[("key", &operation.key), ("file", &operation.file), ("base", base)]));
    }
    let document = Map(vec![
        ("source".into(), Yaml::str(source)),
        ("groupBy".into(), Yaml::str(group_by.as_str())),
        ("operations".into(), Yaml::Seq(listed)),
    ]);
    write(&dir.join("source.yml"), &emit(&Yaml::Map(document), &[]))
}
