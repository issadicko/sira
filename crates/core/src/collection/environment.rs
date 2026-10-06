//! Environnements (`environments/<nom>.yml`) : liste, lecture, enregistrement, et environnement ouvert par défaut.

use std::cmp::Ordering;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{
    ensure_collection, invalid, parse_exact, read_config, read_tree, resolve_path, rewrite, styled_like, write_atomic,
    write_new, BRUNO_ORDER, COLLECTION_FILE, COLLECTION_ORDER, ENV_DIR, REQUEST_EXT,
};
use crate::request::{is_enabled, non_blank, opt_text, set_list, table, text, Entry, BLANK_BEFORE};
use crate::yaml::{self, Map, Value};
use crate::CoreError;

const ENV_ORDER: &[&str] = &["name", "extends", "color", "variables", "externalSecrets"];
const PRESETS_ORDER: &[&str] = &["requestType", "requestUrl", "defaultEnvironment"];

/// `name` peut être celui d'un environnement : un simple nom de fichier visible, jamais un chemin.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\'])
}

/// Les environnements de la collection : les fichiers `*.yml` de `environments/` dont le nom est utilisable.
pub fn list_environments(root: &Path) -> Result<Vec<String>, CoreError> {
    let dir = root.join(ENV_DIR);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map_err(|e| CoreError::io(&dir, e))?
        .flatten()
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(REQUEST_EXT).map(str::to_owned))
        .filter(|name| is_valid_name(name))
        .collect();
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(Ordering::Equal));
    Ok(names)
}

/// Variable d'un environnement. La valeur d'un secret n'est jamais dans le fichier : `value` est alors `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub name: String,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub secret: bool,
    pub enabled: bool,
    #[serde(default)]
    pub description: Option<String>,
    /// Type d'une valeur qui n'est pas du texte (`number`, `boolean`…), conservé à l'enregistrement.
    #[serde(default)]
    pub data_type: Option<String>,
}

impl Entry for EnvVar {
    const ORDER: &'static [&'static str] = &["secret", "name", "value", "type", "disabled", "description"];

    fn read(m: &Map) -> Self {
        let secret = m.get("secret").is_some_and(Value::is_true);
        let kind = match (secret, m.get("value")) {
            (true, _) => m.str("type"),
            (false, Some(Value::Map(value))) => value.str("type"),
            _ => None,
        };
        Self {
            name: text(m.get("name")),
            value: if secret { None } else { m.get("value").map(|v| text(Some(v))) },
            secret,
            enabled: is_enabled(m),
            description: opt_text(m, "description"),
            data_type: kind.filter(|kind| *kind != "string").map(str::to_owned),
        }
    }

    fn build(&self) -> Map {
        let mut pairs = Vec::new();
        if self.secret {
            pairs.push(("secret", Value::Bool(true)));
            pairs.push(("name", Value::str(&self.name)));
            if let Some(kind) = &self.data_type {
                pairs.push(("type", Value::str(kind)));
            }
        } else {
            let data = self.value.clone().unwrap_or_default();
            let value = match &self.data_type {
                Some(kind) => Value::Map(table(vec![("type", Value::str(kind)), ("data", Value::Str(data))])),
                None => Value::Str(data),
            };
            pairs.push(("name", Value::str(&self.name)));
            pairs.push(("value", value));
        }
        if !self.enabled {
            pairs.push(("disabled", Value::Bool(true)));
        }
        if let Some(description) = non_blank(&self.description) {
            pairs.push(("description", Value::str(description)));
        }
        table(pairs)
    }

    fn ident(&self) -> String {
        self.name.clone()
    }
}

fn variables_of(tree: &Map) -> Vec<EnvVar> {
    tree.seq("variables").iter().filter_map(Value::as_map).map(EnvVar::read).collect()
}

/// Fichier de l'environnement `name` : un simple nom, jamais un chemin.
pub fn environment_file(root: &Path, name: &str) -> Result<PathBuf, CoreError> {
    if !is_valid_name(name) {
        return Err(CoreError::InvalidEnvironment(name.to_owned()));
    }
    resolve_path(root, &format!("{ENV_DIR}/{name}{REQUEST_EXT}"))
}

pub fn read_environment(root: &Path, name: &str) -> Result<Vec<EnvVar>, CoreError> {
    let path = environment_file(root, name)?;
    Ok(variables_of(&read_tree(&path)?))
}

/// Variables d'un fichier d'environnement YAML à un chemin quelconque (`xc run --env-file`), sans suivre `extends`.
pub fn read_environment_file(path: &Path) -> Result<Vec<EnvVar>, CoreError> {
    Ok(variables_of(&read_tree(path)?))
}

/// Enregistre les variables de l'environnement `name`. Chaque variable reprend sa table existante (repérée par son
/// nom) et n'y change que ce qui a changé : les clés inconnues, l'ordre et la forme des valeurs sont conservés, ainsi
/// que les autres clés du fichier (`extends`, `color`…). Un fichier absent est refusé, sauf avec `create` : un
/// environnement supprimé ou renommé ailleurs n'est jamais recréé par erreur. Renvoie `false` quand rien ne change :
/// le fichier n'est alors pas réécrit, ni normalisé. Le fichier est réémis par l'émetteur : les commentaires et la mise
/// en forme d'un fichier écrit à la main sont normalisés ; rien n'est écrit à travers un `environments/` lien
/// symbolique.
pub fn save_environment(root: &Path, name: &str, vars: &[EnvVar], create: bool) -> Result<bool, CoreError> {
    ensure_collection(root)?;
    let path = environment_file(root, name)?;
    if root.join(ENV_DIR).is_symlink() {
        return Err(CoreError::Symlink(ENV_DIR.into()));
    }
    let current = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == ErrorKind::NotFound && create => None,
        Err(e) => return Err(CoreError::io(&path, e)),
    };
    let mut tree = match &current {
        Some(text) => parse_exact(text, &path)?,
        None => {
            let mut fresh = Map::default();
            fresh.set("name", Value::str(name), ENV_ORDER);
            fresh
        }
    };
    if current.is_some() && variables_of(&tree) == vars {
        return Ok(false);
    }
    set_list(&mut tree, "variables", vars, ENV_ORDER);
    let next = styled_like(current.as_deref().unwrap_or_default(), yaml::emit(&Value::Map(tree), BLANK_BEFORE));
    match current {
        Some(text) if text == next => Ok(false),
        Some(_) => write_atomic(root, &path, &next).map(|()| true),
        None => {
            fs::create_dir_all(path.parent().unwrap_or(root)).map_err(|e| CoreError::io(&path, e))?;
            write_new(&path, &next).map_err(|e| CoreError::io(&path, e)).map(|()| true)
        }
    }
}

/// Texte de l'environnement `text` dont `name` est la seule clé qui change ; rendu tel quel quand il a déjà ce nom.
pub fn with_environment_name(path: &Path, text: &str, name: &str) -> Result<String, CoreError> {
    rewrite(path, text, |tree| {
        if tree.str("name") == Some(name) {
            return Ok(false);
        }
        tree.set("name", Value::str(name), ENV_ORDER);
        Ok(true)
    })
}

/// Environnement que `opencollection.yml` fait ouvrir par défaut, s'il en nomme un.
pub fn default_environment(root: &Path) -> Result<Option<String>, CoreError> {
    let config = read_config(root)?;
    let presets = config.map("extensions").and_then(|e| e.map("bruno")).and_then(|b| b.map("presets"));
    Ok(presets.and_then(|p| p.str("defaultEnvironment")).map(str::to_owned))
}

/// Choisit l'environnement que la collection ouvre par défaut (`extensions.bruno.presets.defaultEnvironment` de
/// `opencollection.yml`), ou n'en choisit aucun avec `None` : la clé est alors retirée, avec les tables que ce retrait
/// vide. Le reste du fichier est conservé ; un lien symbolique est refusé.
pub fn set_default_environment(root: &Path, name: Option<&str>) -> Result<(), CoreError> {
    ensure_collection(root)?;
    let path = root.join(COLLECTION_FILE);
    if path.is_symlink() {
        return Err(CoreError::Symlink(path.display().to_string()));
    }
    let text = fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
    let updated = rewrite(&path, &text, |tree| {
        let is_table = |value: Option<&Value>| value.is_none_or(|v| matches!(v, Value::Null | Value::Map(_)));
        let extensions = tree.get("extensions");
        let bruno = extensions.and_then(Value::as_map).and_then(|e| e.get("bruno"));
        let presets = bruno.and_then(Value::as_map).and_then(|b| b.get("presets"));
        if !(is_table(extensions) && is_table(bruno) && is_table(presets)) {
            return Err(invalid(&path, "« extensions.bruno.presets » doit être une table dans des tables"));
        }
        let Some(name) = name else { return Ok(clear_default(tree)) };
        let presets = tree
            .map_mut_or_insert("extensions", COLLECTION_ORDER)
            .map_mut_or_insert("bruno", &[])
            .map_mut_or_insert("presets", BRUNO_ORDER);
        if presets.str("defaultEnvironment") == Some(name) {
            return Ok(false);
        }
        presets.set("defaultEnvironment", Value::str(name), PRESETS_ORDER);
        Ok(true)
    })?;
    match updated == text {
        true => Ok(()),
        false => write_atomic(root, &path, &updated),
    }
}

/// Variable de collection (`request.variables` de `opencollection.yml`) : la valeur en texte et, quand ce n'est pas du
/// texte, son type (`number`, `boolean`, `object`), que le fichier écrit en `{ type, data }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionVar {
    pub name: String,
    pub value: String,
    pub data_type: Option<String>,
    pub enabled: bool,
    pub description: Option<String>,
}

impl Entry for CollectionVar {
    const ORDER: &'static [&'static str] = &["name", "value", "description", "disabled"];

    fn read(m: &Map) -> Self {
        let kind = m.get("value").and_then(Value::as_map).and_then(|v| v.str("type"));
        Self {
            name: text(m.get("name")),
            value: text(m.get("value")),
            data_type: kind.filter(|kind| *kind != "string").map(str::to_owned),
            enabled: is_enabled(m),
            description: opt_text(m, "description"),
        }
    }

    fn build(&self) -> Map {
        let value = match &self.data_type {
            Some(kind) => Value::Map(table(vec![("type", Value::str(kind)), ("data", Value::Str(self.value.clone()))])),
            None => Value::str(&self.value),
        };
        let mut pairs = vec![("name", Value::str(&self.name)), ("value", value)];
        if let Some(d) = non_blank(&self.description) {
            pairs.push(("description", Value::str(d)));
        }
        if !self.enabled {
            pairs.push(("disabled", Value::Bool(true)));
        }
        table(pairs)
    }

    fn ident(&self) -> String {
        self.name.clone()
    }
}

fn collection_variables(tree: &Map) -> Vec<CollectionVar> {
    tree.map("request")
        .map(|r| r.seq("variables").iter().filter_map(Value::as_map).map(CollectionVar::read).collect())
        .unwrap_or_default()
}

/// Les variables de collection (`request.variables` de `opencollection.yml`), actives ou non.
pub fn read_collection_variables(root: &Path) -> Result<Vec<CollectionVar>, CoreError> {
    Ok(collection_variables(&read_tree(&root.join(COLLECTION_FILE))?))
}

/// Enregistre les variables de collection, sans toucher au reste du fichier ; chaque variable reprend sa table existante
/// (repérée par son nom). Rend `true` si le fichier a changé. Un lien symbolique est refusé.
pub fn save_collection_variables(root: &Path, vars: &[CollectionVar]) -> Result<bool, CoreError> {
    ensure_collection(root)?;
    let path = root.join(COLLECTION_FILE);
    if path.is_symlink() {
        return Err(CoreError::Symlink(path.display().to_string()));
    }
    let text = fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
    let updated = rewrite(&path, &text, |tree| {
        if collection_variables(tree) == vars {
            return Ok(false);
        }
        let request = tree.map_mut_or_insert("request", COLLECTION_ORDER);
        set_list(request, "variables", vars, CollectionVar::ORDER);
        if request.is_empty() {
            tree.remove("request");
        }
        Ok(true)
    })?;
    match updated == text {
        true => Ok(false),
        false => write_atomic(root, &path, &updated).map(|()| true),
    }
}

/// Retire `defaultEnvironment`, puis chaque table qui devient vide. Faux quand il n'y avait rien à retirer.
fn clear_default(tree: &mut Map) -> bool {
    let Some(Value::Map(extensions)) = tree.get_mut("extensions") else { return false };
    let Some(Value::Map(bruno)) = extensions.get_mut("bruno") else { return false };
    let Some(Value::Map(presets)) = bruno.get_mut("presets") else { return false };
    if presets.remove("defaultEnvironment").is_none() {
        return false;
    }
    if presets.is_empty() {
        bruno.remove("presets");
    }
    if bruno.is_empty() {
        extensions.remove("bruno");
    }
    if extensions.is_empty() {
        tree.remove("extensions");
    }
    true
}
