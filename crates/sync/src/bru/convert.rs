//! Conversion d'un dossier de collection `.bru` en collection JSON de Bruno : `bruno.json`, `collection.bru`, les
//! `folder.bru`, les requêtes `.bru` et `environments/*.bru`. L'écriture en OpenCollection YAML est celle des autres
//! imports (`import::write_plain_collection`).

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value as Json};

use super::{environment, semantics};
use crate::postman::Issue;

pub const CONFIG_FILE: &str = "bruno.json";
const COLLECTION_FILE: &str = "collection.bru";
const FOLDER_FILE: &str = "folder.bru";
const ENV_DIR: &str = "environments";

#[derive(Debug, thiserror::Error)]
pub enum BruError {
    #[error("{0} n'est pas une collection .bru : bruno.json est introuvable")]
    NotACollection(String),
    #[error("bruno.json illisible : {0}")]
    Config(String),
    #[error("{path} illisible : {message}")]
    Io { path: String, message: String },
}

pub struct Converted {
    pub collection: Json,
    pub issues: Vec<Issue>,
}

/// `true` quand `dir` est une collection au format `.bru` (un `bruno.json` et pas d'`opencollection.yml`).
pub fn is_bru_collection(dir: &Path) -> bool {
    dir.join(CONFIG_FILE).is_file() && !dir.join("opencollection.yml").exists()
}

pub fn collection_from_dir(dir: &Path) -> Result<Converted, BruError> {
    let config_path = dir.join(CONFIG_FILE);
    if !config_path.is_file() {
        return Err(BruError::NotACollection(dir.display().to_string()));
    }
    let config: Json = serde_json::from_str(&read(&config_path)?).map_err(|e| BruError::Config(e.to_string()))?;
    if !config.is_object() {
        return Err(BruError::Config("un objet JSON est attendu".into()));
    }
    let name = config
        .get("name")
        .and_then(Json::as_str)
        .filter(|n| !n.is_empty())
        .map(str::to_owned)
        .or_else(|| dir.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "Untitled Collection".into());
    let ignored = ignored_names(&config);

    let mut issues = Vec::new();
    let mut other_files = BTreeMap::new();
    let root = match dir.join(COLLECTION_FILE) {
        path if path.is_file() => folder_root(&path, COLLECTION_FILE, &mut issues).unwrap_or(Json::Null),
        _ => Json::Null,
    };
    let items = items(dir, "", &ignored, true, &mut issues, &mut other_files);
    let environments = environments(dir, &mut issues);
    summarize(&other_files, &mut issues);

    let mut collection = json!({ "name": name, "items": items, "environments": environments, "config": config });
    if !root.is_null() {
        collection["root"] = root;
    }
    Ok(Converted { collection, issues })
}

fn read(path: &Path) -> Result<String, BruError> {
    fs::read_to_string(path).map_err(|e| BruError::Io { path: path.display().to_string(), message: e.to_string() })
}

/// Noms de fichiers et de dossiers que la collection demande d'ignorer (`ignore` de `bruno.json`).
fn ignored_names(config: &Json) -> Vec<String> {
    match config.get("ignore").and_then(Json::as_array) {
        Some(list) => list.iter().filter_map(Json::as_str).map(str::to_owned).collect(),
        None => vec!["node_modules".into(), ".git".into()],
    }
}

fn join(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_owned()
    } else {
        format!("{parent}/{name}")
    }
}

/// `collection.bru` ou `folder.bru` : la racine (en-têtes, auth, variables, scripts, documentation) à écrire.
fn folder_root(path: &Path, shown: &str, issues: &mut Vec<Issue>) -> Option<Json> {
    let parsed =
        read(path).map_err(|e| e.to_string()).and_then(|text| semantics::read(&text).map_err(|e| e.to_string()));
    match parsed {
        Ok((json, skipped)) => {
            skipped.iter().for_each(|(line, block)| issues.push(unknown_block(shown, *line, block)));
            Some(semantics::collection_root(&json))
        }
        Err(message) => {
            issues.push(Issue {
                path: shown.to_owned(),
                severity: "error",
                message: format!("Fichier ignoré : {message}"),
            });
            None
        }
    }
}

fn unknown_block(path: &str, line: usize, block: &str) -> Issue {
    Issue {
        path: path.to_owned(),
        severity: "warning",
        message: format!("Bloc « {block} » ignoré, il est inconnu de Sira (ligne {line})"),
    }
}

fn seq_of(item: &Json) -> f64 {
    item.get("seq").and_then(Json::as_f64).unwrap_or(f64::MAX)
}

fn by_seq_then_name(a: &Json, b: &Json) -> Ordering {
    seq_of(a).partial_cmp(&seq_of(b)).unwrap_or(Ordering::Equal).then_with(|| {
        let name = |i: &Json| i.get("name").and_then(Json::as_str).unwrap_or_default().to_lowercase();
        name(a).cmp(&name(b))
    })
}

fn items(
    dir: &Path,
    relative: &str,
    ignored: &[String],
    top: bool,
    issues: &mut Vec<Issue>,
    other_files: &mut BTreeMap<String, usize>,
) -> Vec<Json> {
    let Ok(listing) = fs::read_dir(dir) else { return Vec::new() };
    let mut entries: Vec<_> = listing.filter_map(Result::ok).collect();
    entries.sort_by_key(|e| e.file_name());

    let mut folders = Vec::new();
    let mut requests = Vec::new();
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let shown = join(relative, &name);
        if ignored.contains(&name) || (top && name == ENV_DIR && path.is_dir()) {
            continue;
        }
        if path.is_dir() {
            if name.starts_with('.') {
                continue;
            }
            folders.push(folder(&path, &name, &shown, ignored, issues, other_files));
        } else if name.ends_with(".bru") {
            if name == COLLECTION_FILE && top || name == FOLDER_FILE {
                continue;
            }
            requests.extend(request(&path, &name, &shown, issues));
        } else if (top && name == CONFIG_FILE) || (name.starts_with('.') && name != ".env") {
            continue;
        } else {
            let extension = path.extension().map_or_else(|| name.clone(), |e| format!(".{}", e.to_string_lossy()));
            *other_files.entry(extension).or_default() += 1;
        }
    }
    folders.sort_by(by_seq_then_name);
    requests.sort_by(by_seq_then_name);
    folders.into_iter().chain(requests).collect()
}

fn folder(
    path: &Path,
    name: &str,
    shown: &str,
    ignored: &[String],
    issues: &mut Vec<Issue>,
    other_files: &mut BTreeMap<String, usize>,
) -> Json {
    let root = match path.join(FOLDER_FILE) {
        file if file.is_file() => folder_root(&file, &join(shown, FOLDER_FILE), issues),
        _ => None,
    };
    let title = root
        .as_ref()
        .and_then(|r| r.pointer("/meta/name"))
        .and_then(Json::as_str)
        .filter(|n| !n.is_empty())
        .unwrap_or(name)
        .to_owned();
    let seq = root.as_ref().and_then(|r| r.pointer("/meta/seq")).cloned();
    let mut item = json!({
        "name": title,
        "type": "folder",
        "items": items(path, shown, ignored, false, issues, other_files),
    });
    if let Some(seq) = seq {
        item["seq"] = seq;
    }
    if let Some(mut root) = root {
        if root.pointer("/meta/name").and_then(Json::as_str).is_none_or(str::is_empty) {
            root["meta"] = json!({ "name": title });
        }
        item["root"] = root;
    }
    item
}

fn request(path: &Path, file: &str, shown: &str, issues: &mut Vec<Issue>) -> Option<Json> {
    let text = match read(path) {
        Ok(text) => text,
        Err(e) => {
            issues
                .push(Issue { path: shown.to_owned(), severity: "error", message: format!("Requête ignorée : {e}") });
            return None;
        }
    };
    let (json, skipped) = match semantics::read(&text) {
        Ok(read) => read,
        Err(e) => {
            issues
                .push(Issue { path: shown.to_owned(), severity: "error", message: format!("Requête ignorée : {e}") });
            return None;
        }
    };
    skipped.iter().for_each(|(line, block)| issues.push(unknown_block(shown, *line, block)));
    match semantics::request(&json) {
        Ok((mut item, warnings)) => {
            warnings
                .into_iter()
                .for_each(|message| issues.push(Issue { path: shown.to_owned(), severity: "warning", message }));
            if item["name"].as_str().is_none_or(str::is_empty) {
                item["name"] = json!(file.strip_suffix(".bru").unwrap_or(file));
            }
            Some(item)
        }
        Err(semantics::Unsupported::Type(kind)) => {
            issues.push(Issue {
                path: shown.to_owned(),
                severity: "error",
                message: format!("Requête ignorée : le type « {kind} » n'est pas pris en charge"),
            });
            None
        }
    }
}

fn environments(dir: &Path, issues: &mut Vec<Issue>) -> Vec<Json> {
    let folder = dir.join(ENV_DIR);
    let Ok(listing) = fs::read_dir(&folder) else { return Vec::new() };
    let mut files: Vec<_> =
        listing.filter_map(Result::ok).filter(|e| e.file_name().to_string_lossy().ends_with(".bru")).collect();
    files.sort_by_key(|e| e.file_name());
    let mut out = Vec::new();
    for entry in files {
        let file = entry.file_name().to_string_lossy().into_owned();
        let shown = join(ENV_DIR, &file);
        let parsed = read(&entry.path())
            .map_err(|e| e.to_string())
            .and_then(|t| environment::read(&t).map_err(|e| e.to_string()));
        match parsed {
            Ok((mut env, skipped)) => {
                skipped.iter().for_each(|(line, block)| issues.push(unknown_block(&shown, *line, block)));
                let map: &mut Map<String, Json> = env.as_object_mut().expect("objet");
                map.insert("name".into(), json!(file.strip_suffix(".bru").unwrap_or(&file)));
                out.push(env);
            }
            Err(message) => issues.push(Issue {
                path: shown,
                severity: "error",
                message: format!("Environnement ignoré : {message}"),
            }),
        }
    }
    out
}

/// Un seul avertissement pour les fichiers qui ne sont pas des `.bru` : l'import ne les copie pas.
fn summarize(other_files: &BTreeMap<String, usize>, issues: &mut Vec<Issue>) {
    if other_files.is_empty() {
        return;
    }
    let list: Vec<_> = other_files.iter().map(|(ext, n)| format!("{ext} ×{n}")).collect();
    issues.push(Issue {
        path: "(collection)".into(),
        severity: "warning",
        message: format!(
            "Fichiers non copiés : {}. Copie-les à la main si des scripts ou des corps de requête s'en servent",
            list.join(", ")
        ),
    });
}
