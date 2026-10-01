use std::cmp::Ordering;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use serde::Serialize;

use crate::request::{RequestDoc, BLANK_BEFORE};
use crate::yaml::{self, Map, Value};
use crate::CoreError;

pub const COLLECTION_FILE: &str = "opencollection.yml";
pub const FOLDER_FILE: &str = "folder.yml";
pub const REQUEST_EXT: &str = ".yml";
pub const ENV_DIR: &str = "environments";
const MOCKS_DIR: &str = "mocks";
const NODE_MODULES: &str = "node_modules";

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TreeItem {
    #[serde(rename_all = "camelCase")]
    Folder { path: String, name: String, seq: Option<i64>, children: Vec<TreeItem> },
    #[serde(rename_all = "camelCase")]
    Request {
        path: String,
        name: String,
        seq: Option<i64>,
        method: String,
        url: String,
        request_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

impl TreeItem {
    fn name(&self) -> &str {
        match self {
            Self::Folder { name, .. } | Self::Request { name, .. } => name,
        }
    }

    fn seq(&self) -> Option<i64> {
        match self {
            Self::Folder { seq, .. } | Self::Request { seq, .. } => seq.filter(|s| *s > 0),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionInfo {
    pub root: String,
    pub name: String,
    pub items: Vec<TreeItem>,
    pub environments: Vec<String>,
    pub default_environment: Option<String>,
    pub request_count: usize,
}

pub(crate) fn read_tree(path: &Path) -> Result<Map, CoreError> {
    let text = fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
    match yaml::parse(&text)
        .map_err(|e| CoreError::Yaml { path: path.display().to_string(), message: e.to_string() })?
    {
        Value::Map(m) => Ok(m),
        Value::Null => Ok(Map::default()),
        _ => Err(CoreError::Yaml {
            path: path.display().to_string(),
            message: "le document doit être une table".into(),
        }),
    }
}

/// Résout un chemin relatif à la collection en refusant toute sortie de la racine, y compris par un lien symbolique.
pub fn resolve_path(root: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    let rel = Path::new(relative);
    let escapes = |c: Component| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_));
    let path = root.join(rel);
    if rel.is_absolute() || rel.components().any(escapes) || !is_inside(root, &path) {
        return Err(CoreError::OutsideCollection(relative.to_owned()));
    }
    Ok(path)
}

/// Comme [`resolve_path`], en refusant aussi les éléments cachés (`.env`, `.git`, `.oc-sync`) que l'arbre n'expose pas.
pub fn resolve_visible_path(root: &Path, relative: &str) -> Result<PathBuf, CoreError> {
    let hidden = |c: Component| matches!(c, Component::Normal(name) if name.to_string_lossy().starts_with('.'));
    if Path::new(relative).components().any(hidden) {
        return Err(CoreError::HiddenPath(relative.to_owned()));
    }
    resolve_path(root, relative)
}

/// `path` reste sous `root` une fois les liens symboliques résolus, ceux de son plus proche ancêtre existant
/// quand lui-même n'existe pas encore ; un lien pendant est refusé.
fn is_inside(root: &Path, path: &Path) -> bool {
    let resolved =
        |p: &Path| p.ancestors().find(|a| a.symlink_metadata().is_ok()).and_then(|a| fs::canonicalize(a).ok());
    matches!((resolved(root), resolved(path)), (Some(root), Some(path)) if path.starts_with(&root))
}

/// Nom que [`open_collection`] ne montre jamais : commençant par un point, `node_modules`, les fichiers
/// `opencollection.yml` et `folder.yml` et, à la racine, les dossiers `environments` et `mocks`.
pub fn is_hidden(name: &str, at_root: bool) -> bool {
    name.starts_with('.')
        || matches!(name, NODE_MODULES | COLLECTION_FILE | FOLDER_FILE)
        || (at_root && matches!(name, ENV_DIR | MOCKS_DIR))
}

fn read_config(root: &Path) -> Result<Map, CoreError> {
    let config_path = root.join(COLLECTION_FILE);
    if !config_path.is_file() {
        return Err(CoreError::NotACollection(root.display().to_string()));
    }
    read_tree(&config_path)
}

fn ignored(bruno: Option<&Map>) -> Vec<String> {
    bruno.map(|b| b.seq("ignore").iter().filter_map(Value::scalar).collect()).unwrap_or_default()
}

pub fn open_collection(root: &Path) -> Result<CollectionInfo, CoreError> {
    let config = read_config(root)?;
    let bruno = config.map("extensions").and_then(|e| e.map("bruno"));
    let ignore = ignored(bruno);
    let default_environment =
        bruno.and_then(|b| b.map("presets")).and_then(|p| p.str("defaultEnvironment")).map(str::to_owned);

    let items = read_folder(root, root, &ignore)?;
    let mut count = 0;
    count_requests(&items, &mut count);
    let name = config
        .map("info")
        .and_then(|i| i.str("name"))
        .map(str::to_owned)
        .unwrap_or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());

    Ok(CollectionInfo {
        root: root.display().to_string(),
        name,
        items,
        environments: list_environments(root)?,
        default_environment,
        request_count: count,
    })
}

fn count_requests(items: &[TreeItem], count: &mut usize) {
    for item in items {
        match item {
            TreeItem::Folder { children, .. } => count_requests(children, count),
            TreeItem::Request { .. } => *count += 1,
        }
    }
}

fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
}

/// Dossiers et fichiers de requête de `dir` que l'arbre montre ; un lien symbolique n'est ni suivi vers un dossier
/// ni lu s'il mène hors de la collection.
fn visible_entries(root: &Path, dir: &Path, ignore: &[String]) -> Result<Vec<Entry>, CoreError> {
    let entries = fs::read_dir(dir).map_err(|e| CoreError::io(dir, e))?;
    Ok(entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if ignore.contains(&name) || is_hidden(&name, dir == root) {
                return None;
            }
            let path = entry.path();
            let kind = entry.file_type().ok()?;
            if kind.is_symlink() && !is_inside(root, &path) {
                return None;
            }
            let is_dir = kind.is_dir();
            let is_request = !is_dir && name.ends_with(REQUEST_EXT) && !path.is_dir();
            (is_dir || is_request).then_some(Entry { name, path, is_dir })
        })
        .collect())
}

/// Nombre de dossiers et de requêtes que l'arbre montre dans `folder` (`""` pour la racine), ou `None` quand ce
/// dossier n'est pas lui-même montré.
pub fn count_entries(root: &Path, folder: &str) -> Result<Option<usize>, CoreError> {
    let config = read_config(root)?;
    let ignore = ignored(config.map("extensions").and_then(|e| e.map("bruno")));
    let mut dir = root.to_path_buf();
    for name in folder.split('/').filter(|part| !part.is_empty()) {
        let found = visible_entries(root, &dir, &ignore)?.into_iter().find(|e| e.is_dir && e.name == name);
        let Some(found) = found else { return Ok(None) };
        dir = found.path;
    }
    Ok(Some(visible_entries(root, &dir, &ignore)?.len()))
}

fn read_folder(root: &Path, dir: &Path, ignore: &[String]) -> Result<Vec<TreeItem>, CoreError> {
    let mut items = Vec::new();
    for Entry { name, path, is_dir } in visible_entries(root, dir, ignore)? {
        if !is_dir {
            items.push(read_request_item(root, &path, &name));
            continue;
        }
        let meta = path.join(FOLDER_FILE);
        let (folder_name, seq) = match meta.is_file().then(|| read_tree(&meta)) {
            Some(Ok(m)) => {
                let info = m.map("info");
                (
                    info.and_then(|i| i.str("name")).unwrap_or(&name).to_owned(),
                    info.and_then(|i| i.get("seq")).and_then(Value::as_i64),
                )
            }
            _ => (name.clone(), None),
        };
        items.push(TreeItem::Folder {
            path: relative(root, &path),
            name: folder_name,
            seq,
            children: read_folder(root, &path, ignore)?,
        });
    }
    Ok(sort_by_name_then_sequence(items))
}

fn read_request_item(root: &Path, path: &Path, file_name: &str) -> TreeItem {
    let fallback = file_name.trim_end_matches(REQUEST_EXT).to_owned();
    match read_tree(path) {
        Ok(tree) => {
            let doc = RequestDoc::from_tree(&tree);
            TreeItem::Request {
                path: relative(root, path),
                name: if doc.name.is_empty() { fallback } else { doc.name },
                seq: doc.seq,
                method: doc.method.to_uppercase(),
                url: doc.url,
                request_type: doc.request_type,
                error: None,
            }
        }
        Err(e) => TreeItem::Request {
            path: relative(root, path),
            name: fallback,
            seq: None,
            method: String::new(),
            url: String::new(),
            request_type: "http".into(),
            error: Some(e.to_string()),
        },
    }
}

/// Même ordre que Bruno : les éléments sans `seq` par nom, puis chaque `seq` inséré à l'index `seq - 1`.
fn sort_by_name_then_sequence(items: Vec<TreeItem>) -> Vec<TreeItem> {
    let by_name = |a: &TreeItem, b: &TreeItem| {
        a.name().to_lowercase().cmp(&b.name().to_lowercase()).then_with(|| a.name().cmp(b.name()))
    };
    let (mut with_seq, mut without): (Vec<_>, Vec<_>) = items.into_iter().partition(|i| i.seq().is_some());
    without.sort_by(by_name);
    with_seq.sort_by(|a, b| a.seq().cmp(&b.seq()).then_with(|| by_name(a, b)));
    let mut result = without;
    for item in with_seq {
        let at = usize::try_from(item.seq().unwrap_or(1) - 1).unwrap_or(0).min(result.len());
        result.insert(at, item);
    }
    result
}

pub fn list_environments(root: &Path) -> Result<Vec<String>, CoreError> {
    let dir = root.join(ENV_DIR);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = fs::read_dir(&dir)
        .map_err(|e| CoreError::io(&dir, e))?
        .flatten()
        .filter_map(|e| e.file_name().to_string_lossy().strip_suffix(REQUEST_EXT).map(str::to_owned))
        .collect();
    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()).then(Ordering::Equal));
    Ok(names)
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EnvVar {
    pub name: String,
    pub value: Option<String>,
    pub secret: bool,
    pub enabled: bool,
}

pub fn read_environment(root: &Path, name: &str) -> Result<Vec<EnvVar>, CoreError> {
    let path = resolve_path(root, &format!("{ENV_DIR}/{name}{REQUEST_EXT}"))?;
    let tree = read_tree(&path)?;
    Ok(tree
        .seq("variables")
        .iter()
        .filter_map(Value::as_map)
        .map(|m| {
            let secret = m.get("secret").is_some_and(Value::is_true);
            EnvVar {
                name: crate::request::text(m.get("name")),
                value: if secret { None } else { m.get("value").map(|v| crate::request::text(Some(v))) },
                secret,
                enabled: !m.get("disabled").is_some_and(Value::is_true),
            }
        })
        .collect())
}

pub fn read_request(root: &Path, relative: &str) -> Result<RequestDoc, CoreError> {
    Ok(RequestDoc::from_tree(&read_tree(&resolve_path(root, relative)?)?))
}

/// Enregistre la requête. Renvoie `false` quand le fichier n'a pas changé (aucune écriture).
pub fn save_request(root: &Path, relative: &str, doc: &RequestDoc) -> Result<bool, CoreError> {
    let path = resolve_path(root, relative)?;
    let current = fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
    let mut tree = read_tree(&path)?;
    let previous = RequestDoc::from_tree(&tree);
    doc.apply(&mut tree, &previous);
    let next = yaml::emit(&Value::Map(tree), BLANK_BEFORE);
    if next == current || (doc == &previous && current.trim_end() == next.trim_end()) {
        return Ok(false);
    }
    write_atomic(&path, &next)?;
    Ok(true)
}

/// Écrit via un fichier temporaire voisin, synchronisé sur le disque, puis renomme, sans jamais laisser un fichier
/// à moitié écrit. Le nom temporaire ne dépend pas de celui du fichier, qui peut déjà occuper les 255 octets permis.
pub fn write_atomic(path: &Path, text: &str) -> Result<(), CoreError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".xc-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, AtomicOrdering::Relaxed)));
    let staged = fs::File::create(&tmp).and_then(|mut file| {
        file.write_all(text.as_bytes())?;
        file.sync_all()
    });
    staged
        .map_err(|e| CoreError::io(&tmp, e))
        .and_then(|()| fs::rename(&tmp, path).map_err(|e| CoreError::io(path, e)))
        .inspect_err(|_| {
            fs::remove_file(&tmp).ok();
        })
}

/// Crée `path` sans jamais écraser un fichier existant (création exclusive, erreur `AlreadyExists` sinon).
pub fn write_new(path: &Path, text: &str) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(text.as_bytes()).and_then(|()| file.sync_all()).inspect_err(|_| {
        fs::remove_file(path).ok();
    })
}

/// Relit puis réécrit un document avec l'émetteur, sans rien modifier.
pub fn normalize(text: &str, blank_before: &[&str]) -> Result<String, yaml::YamlError> {
    Ok(yaml::emit(&yaml::parse(text)?, blank_before))
}
