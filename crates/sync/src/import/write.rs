//! Écriture d'une collection importée (`renderer:import-collection` de Bruno, format YAML) et de son snapshot.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use xc_core::collection::{write_atomic, COLLECTION_FILE, FOLDER_FILE};
use xc_core::yaml::{emit, Map, Value as Yaml};

use super::naming::{folder_name, stem, Directory};
use super::source::source_value;
use super::{join, text, ImportError};
use crate::openapi::GroupBy;
use crate::stringify;

const ENV_DIR: &str = "environments";
const SYNC_DIR: &str = ".oc-sync";
const REQUEST_EXT: &str = ".yml";
const REQUEST_TYPES: [&str; 2] = ["http-request", "graphql-request"];

struct Operation {
    key: String,
    file: String,
    content: String,
}

fn write(path: &Path, text: &str) -> Result<(), ImportError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| ImportError::io(parent, e))?;
    }
    Ok(write_atomic(path, text)?)
}

/// Écrit la collection JSON de Bruno (`openApiToBruno`) dans un nouveau dossier de `location` et renvoie sa racine.
///
/// Même ordre que Bruno : `opencollection.yml`, les éléments, `environments/`, puis, en dernier, le snapshot
/// `.oc-sync/openapi/` qui sert de marque d'import complet. Seul écart de contenu : `.oc-sync` rejoint la liste
/// `ignore` de `opencollection.yml`. `source` est l'URL ou le chemin de la spec, `group_by` le regroupement utilisé.
/// Une écriture qui échoue laisse le dossier partiel en place, sans snapshot : l'import suivant en crée un autre.
pub fn write_collection(
    collection: &Value,
    location: &Path,
    source: &str,
    group_by: GroupBy,
) -> Result<PathBuf, ImportError> {
    if !location.is_dir() {
        return Err(ImportError::Location(location.display().to_string()));
    }
    let location = std::path::absolute(location).map_err(|e| ImportError::io(location, e))?;
    let (root, name) = new_root(&location, text(collection, "name"));
    fs::create_dir(&root).map_err(|e| ImportError::io(&root, e))?;

    let config = json!({
        "name": name,
        "type": "collection",
        "ignore": ["node_modules", ".git", SYNC_DIR],
        "opencollection": "1.0.0"
    });
    let root_config = collection.get("root").unwrap_or(&Value::Null);
    write(&root.join(COLLECTION_FILE), &stringify::collection(root_config, &config))?;

    let mut walk = Walk::default();
    let mut names = Directory::new(&[COLLECTION_FILE, FOLDER_FILE, ENV_DIR, SYNC_DIR]);
    walk.items(collection.get("items"), &root, "", &mut names)?;
    environments(collection.get("environments"), &root)?;
    snapshot(&root, &source_value(source, &root), group_by, &walk.operations)?;
    Ok(root)
}

/// `findUniqueFolderName` : le dossier de la collection, suffixé ` - 1`, ` - 2`… s'il existe déjà.
fn new_root(location: &Path, title: &str) -> (PathBuf, String) {
    let mut name = title.to_owned();
    let mut counter = 0;
    while location.join(folder_name(&name)).exists() {
        counter += 1;
        name = format!("{title} - {counter}");
    }
    (location.join(folder_name(&name)), name)
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
        fs::create_dir_all(&path).map_err(|e| ImportError::io(&path, e))?;
        let has_name = item.pointer("/root/meta/name").and_then(Value::as_str).is_some_and(|n| !n.is_empty());
        if let (true, Some(folder_root)) = (has_name, item.get("root")) {
            let mut folder_root = folder_root.clone();
            folder_root["meta"]["seq"] = item.get("seq").cloned().unwrap_or(Value::Null);
            write(&path.join(FOLDER_FILE), &stringify::folder(&folder_root))?;
        }
        self.items(item.get("items"), &path, &join(relative, &name), &mut Directory::new(&[FOLDER_FILE]))
    }
}

fn environments(environments: Option<&Value>, root: &Path) -> Result<(), ImportError> {
    let dir = root.join(ENV_DIR);
    fs::create_dir_all(&dir).map_err(|e| ImportError::io(&dir, e))?;
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
