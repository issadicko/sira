use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

use serde::Serialize;

use crate::request::{RequestDoc, BLANK_BEFORE, INFO_ORDER, TOP_ORDER};
use crate::yaml::{self, Map, Value};
use crate::CoreError;

mod environment;

pub use environment::{
    default_environment, environment_file, list_environments, read_collection_variables, read_environment,
    read_environment_file, save_collection_variables, save_environment, set_default_environment, with_environment_name,
    CollectionVar, EnvVar,
};

pub const COLLECTION_FILE: &str = "opencollection.yml";
pub const FOLDER_FILE: &str = "folder.yml";
pub const REQUEST_EXT: &str = ".yml";
pub const REQUEST_KINDS: [&str; 4] = ["http", "graphql", "grpc", "websocket"];
/// Autres éléments que Bruno lit dans une collection : montrés dans l'arbre, jamais ouverts ni réécrits ici.
pub const OTHER_ITEM_KINDS: [&str; 2] = ["script", "app"];
pub const ENV_DIR: &str = "environments";
pub const MOCKS_DIR: &str = "mocks";
const COLLECTION_ORDER: &[&str] = &["opencollection", "info", "config", "request", "docs", "bundled", "extensions"];
const BRUNO_ORDER: &[&str] = &["ignore", "presets", "scripts", "openapi"];
const BOM: char = '\u{feff}';
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
        deprecated: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

impl TreeItem {
    pub fn path(&self) -> &str {
        match self {
            Self::Folder { path, .. } | Self::Request { path, .. } => path,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Folder { name, .. } | Self::Request { name, .. } => name,
        }
    }

    /// `seq` s'il est strictement positif : les autres valeurs ne comptent pas pour le tri.
    pub fn seq(&self) -> Option<i64> {
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

/// Le fichier de la collection (`opencollection.yml`), tel qu'il est lu.
pub fn read_collection_file(root: &Path) -> Result<Map, CoreError> {
    read_tree(&root.join(COLLECTION_FILE))
}

/// Le fichier du dossier `dir` (chemin relatif à la racine) ; vide quand le dossier n'a pas de `folder.yml`.
pub fn read_folder_file(root: &Path, dir: &str) -> Result<Map, CoreError> {
    let file = resolve_path(root, &format!("{dir}/{FOLDER_FILE}"))?;
    if file.is_file() {
        read_tree(&file)
    } else {
        Ok(Map::default())
    }
}

pub(crate) fn read_tree(path: &Path) -> Result<Map, CoreError> {
    let text = fs::read_to_string(path).map_err(|e| CoreError::io(path, e))?;
    parse_tree(&text, path)
}

fn invalid(path: &Path, message: impl Into<String>) -> CoreError {
    CoreError::Yaml { path: path.display().to_string(), message: message.into() }
}

fn table(parsed: Result<Value, yaml::YamlError>, path: &Path) -> Result<Map, CoreError> {
    match parsed.map_err(|e| invalid(path, e.to_string()))? {
        Value::Map(m) => Ok(m),
        Value::Null => Ok(Map::default()),
        _ => Err(invalid(path, "le document doit être une table")),
    }
}

fn without_bom(text: &str) -> &str {
    text.strip_prefix(BOM).unwrap_or(text)
}

fn parse_tree(text: &str, path: &Path) -> Result<Map, CoreError> {
    table(yaml::parse(without_bom(text)), path)
}

/// Pour réécrire : un flux de plusieurs documents est refusé plutôt que tronqué.
fn parse_exact(text: &str, path: &Path) -> Result<Map, CoreError> {
    table(yaml::parse_single(without_bom(text)), path)
}

/// `emitted` avec la signature (BOM) et les fins de ligne (CRLF) du texte `original` dont il est la réécriture.
fn styled_like(original: &str, emitted: String) -> String {
    let emitted = if original.contains("\r\n") { emitted.replace('\n', "\r\n") } else { emitted };
    if original.starts_with(BOM) {
        format!("{BOM}{emitted}")
    } else {
        emitted
    }
}

/// Réécriture de `text` par `change`, qui dit si l'arbre a changé ; sinon `text` est rendu tel quel. Un flux de
/// plusieurs documents est refusé, le BOM et les fins de ligne CRLF sont conservés.
fn rewrite(
    path: &Path,
    text: &str,
    change: impl FnOnce(&mut Map) -> Result<bool, CoreError>,
) -> Result<String, CoreError> {
    let mut tree = parse_exact(text, path)?;
    if !change(&mut tree)? {
        return Ok(text.to_owned());
    }
    Ok(styled_like(text, yaml::emit(&Value::Map(tree), BLANK_BEFORE)))
}

/// `root` contient `opencollection.yml`.
pub fn ensure_collection(root: &Path) -> Result<(), CoreError> {
    match root.join(COLLECTION_FILE).is_file() {
        true => Ok(()),
        false => Err(CoreError::NotACollection(root.display().to_string())),
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
    ensure_collection(root)?;
    read_tree(&root.join(COLLECTION_FILE))
}

fn ignored(bruno: Option<&Map>) -> Vec<String> {
    bruno.map(|b| b.seq("ignore").iter().filter_map(Value::scalar).collect()).unwrap_or_default()
}

/// Noms que `extensions.bruno.ignore` de `opencollection.yml` retire de l'arbre, à tous les niveaux.
pub fn ignored_names(root: &Path) -> Result<Vec<String>, CoreError> {
    let config = read_config(root)?;
    Ok(ignored(config.map("extensions").and_then(|e| e.map("bruno"))))
}

fn name_of(config: &Map, root: &Path) -> String {
    config
        .map("info")
        .and_then(|i| i.str("name"))
        .map(str::to_owned)
        .unwrap_or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default())
}

/// Nom de la collection (`info.name`, sinon le nom du dossier), sans parcourir ses fichiers.
pub fn collection_name(root: &Path) -> Result<String, CoreError> {
    Ok(name_of(&read_config(root)?, root))
}

pub fn open_collection(root: &Path) -> Result<CollectionInfo, CoreError> {
    let config = read_config(root)?;
    let bruno = config.map("extensions").and_then(|e| e.map("bruno"));
    let ignore = ignored(bruno);
    let default_environment =
        bruno.and_then(|b| b.map("presets")).and_then(|p| p.str("defaultEnvironment")).map(str::to_owned);

    let items = read_folder(root, root, &ignore, true)?;
    let mut count = 0;
    count_requests(&items, &mut count);
    let name = name_of(&config, root);

    Ok(CollectionInfo {
        root: root.display().to_string(),
        name,
        items,
        environments: list_environments(root)?,
        default_environment,
        request_count: count,
    })
}

/// Marque comme dépréciées les requêtes dont le chemin relatif est dans `paths`.
pub fn mark_deprecated(items: &mut [TreeItem], paths: &HashSet<String>) {
    for item in items {
        match item {
            TreeItem::Folder { children, .. } => mark_deprecated(children, paths),
            TreeItem::Request { path, deprecated, .. } => *deprecated = paths.contains(path.as_str()),
        }
    }
}

fn count_requests(items: &[TreeItem], count: &mut usize) {
    for item in items {
        match item {
            TreeItem::Folder { children, .. } => count_requests(children, count),
            TreeItem::Request { request_type, error: None, .. } if REQUEST_KINDS.contains(&request_type.as_str()) => {
                *count += 1
            }
            TreeItem::Request { .. } => {}
        }
    }
}

/// `info.type` d'un élément que Bruno sait lire (requête, script, app). Un autre fichier YAML (une spec OpenAPI rangée
/// dans la collection, par exemple) n'est pas un élément : Bruno le montre en erreur, sans l'ouvrir.
fn item_type(tree: &Map, file: &str) -> Result<String, CoreError> {
    let not_an_item = |reason: String| CoreError::NotARequest { path: file.to_owned(), reason };
    match tree.map("info").and_then(|info| info.str("type")) {
        Some(kind) if REQUEST_KINDS.contains(&kind) || OTHER_ITEM_KINDS.contains(&kind) => Ok(kind.to_owned()),
        Some(kind) => Err(not_an_item(format!("type « {kind} » non pris en charge"))),
        None => Err(not_an_item("info.type absent".into())),
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
/// ni lu s'il mène hors de la collection, `folder.yml` compris.
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

/// Dossier `folder` (`""` pour la racine) et les noms que l'arbre ignore dans la collection, ou `None` quand ce
/// dossier n'est pas lui-même montré.
fn find_folder(root: &Path, folder: &str) -> Result<Option<(PathBuf, Vec<String>)>, CoreError> {
    let ignore = ignored_names(root)?;
    let mut dir = root.to_path_buf();
    for name in folder.split('/').filter(|part| !part.is_empty()) {
        let found = visible_entries(root, &dir, &ignore)?.into_iter().find(|e| e.is_dir && e.name == name);
        let Some(found) = found else { return Ok(None) };
        dir = found.path;
    }
    Ok(Some((dir, ignore)))
}

/// Nombre de dossiers et de requêtes que l'arbre montre dans `folder` (`""` pour la racine), ou `None` quand ce
/// dossier n'est pas lui-même montré.
pub fn count_entries(root: &Path, folder: &str) -> Result<Option<usize>, CoreError> {
    let Some((dir, ignore)) = find_folder(root, folder)? else { return Ok(None) };
    Ok(Some(visible_entries(root, &dir, &ignore)?.len()))
}

/// Dossiers et requêtes que l'arbre montre dans `folder` (`""` pour la racine), dans l'ordre de l'arbre et sans leurs
/// enfants, ou `None` quand ce dossier n'est pas lui-même montré.
pub fn list_folder(root: &Path, folder: &str) -> Result<Option<Vec<TreeItem>>, CoreError> {
    let Some((dir, ignore)) = find_folder(root, folder)? else { return Ok(None) };
    read_folder(root, &dir, &ignore, false).map(Some)
}

fn read_folder(root: &Path, dir: &Path, ignore: &[String], deep: bool) -> Result<Vec<TreeItem>, CoreError> {
    let mut items = Vec::new();
    for Entry { name, path, is_dir } in visible_entries(root, dir, ignore)? {
        if !is_dir {
            items.push(read_request_item(root, &path, &name));
            continue;
        }
        let meta = path.join(FOLDER_FILE);
        let (folder_name, seq) = match (meta.is_file() && is_inside(root, &meta)).then(|| read_tree(&meta)) {
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
            children: if deep { read_folder(root, &path, ignore, true)? } else { Vec::new() },
        });
    }
    Ok(sort_by_name_then_sequence(items))
}

fn read_request_item(root: &Path, path: &Path, file_name: &str) -> TreeItem {
    let fallback = file_name.trim_end_matches(REQUEST_EXT).to_owned();
    let file = relative(root, path);
    match read_tree(path).and_then(|tree| item_type(&tree, &file).map(|_| tree)) {
        Ok(tree) => {
            let doc = RequestDoc::from_tree(&tree);
            TreeItem::Request {
                path: file,
                name: if doc.name.is_empty() { fallback } else { doc.name },
                seq: doc.seq,
                method: doc.method.to_uppercase(),
                url: doc.url,
                request_type: doc.request_type,
                deprecated: false,
                error: None,
            }
        }
        Err(e) => TreeItem::Request {
            path: file,
            name: fallback,
            seq: None,
            method: String::new(),
            url: String::new(),
            request_type: "http".into(),
            deprecated: false,
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

pub fn read_request(root: &Path, relative: &str) -> Result<RequestDoc, CoreError> {
    let tree = read_tree(&resolve_path(root, relative)?)?;
    item_type(&tree, relative)?;
    Ok(RequestDoc::from_tree(&tree))
}

/// Enregistre la requête. Renvoie `false` quand le fichier n'a pas changé (aucune écriture) : un document que
/// l'utilisateur n'a pas modifié ne réécrit jamais son fichier, quelle que soit la façon dont il était mis en forme.
pub fn save_request(root: &Path, relative: &str, doc: &RequestDoc) -> Result<bool, CoreError> {
    ensure_collection(root)?;
    let path = resolve_path(root, relative)?;
    let current = fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
    let mut tree = read_tree(&path)?;
    let kind = item_type(&tree, relative)?;
    if !REQUEST_KINDS.contains(&kind.as_str()) {
        return Err(CoreError::NotARequest { path: relative.to_owned(), reason: format!("élément « {kind} »") });
    }
    let previous = RequestDoc::from_tree(&tree);
    if doc == &previous {
        return Ok(false);
    }
    doc.apply(&mut tree, &previous);
    let next = styled_like(&current, yaml::emit(&Value::Map(tree), BLANK_BEFORE));
    if next == current {
        return Ok(false);
    }
    write_atomic(root, &path, &next)?;
    Ok(true)
}

/// Contenu du fichier `path` (requête ou `folder.yml`), dont `text` est le texte actuel, avec les champs `fields` de
/// `info` pour seuls changements : le reste de l'arbre (clés inconnues comprises) est conservé, et `text` est rendu tel
/// quel quand ces champs ont déjà leur valeur. Comme pour [`save_request`], le document est réécrit par l'émetteur : un
/// fichier tel que Bruno ou l'application l'écrit ne change que sur les lignes concernées, mais les commentaires et la
/// mise en forme d'un fichier écrit à la main sont normalisés ; le BOM et les fins de ligne CRLF sont conservés. Un
/// flux de plusieurs documents ou un `info` qui n'est pas une table sont refusés, plutôt que tronqués ou remplacés.
pub fn with_info(path: &Path, text: &str, fields: &[(&str, Value)]) -> Result<String, CoreError> {
    rewrite(path, text, |tree| {
        if !matches!(tree.get("info"), None | Some(Value::Null | Value::Map(_))) {
            return Err(invalid(path, "« info » doit être une table"));
        }
        let info = tree.map_mut_or_insert("info", TOP_ORDER);
        if fields.iter().all(|(key, value)| info.get(key) == Some(value)) {
            return Ok(false);
        }
        for (key, value) in fields {
            info.set(key, value.clone(), INFO_ORDER);
        }
        Ok(true)
    })
}

/// `info.type` du document `text`, `None` quand il n'y en a pas : `http`, `graphql`, `grpc` ou `websocket` pour une
/// requête (voir [`REQUEST_KINDS`]), `folder` pour un `folder.yml`.
pub fn info_type(path: &Path, text: &str) -> Result<Option<String>, CoreError> {
    let tree = parse_tree(text, path)?;
    Ok(tree.map("info").and_then(|info| info.str("type")).map(str::to_owned))
}

/// Ajoute `name` à `extensions.bruno.ignore` de `opencollection.yml`, la seule liste que le fichier change, pour que
/// Bruno n'affiche pas ce dossier. Sans effet quand `name` y figure déjà ; refusé quand `opencollection.yml` est un
/// lien symbolique.
pub fn ignore_name(root: &Path, name: &str) -> Result<(), CoreError> {
    let path = root.join(COLLECTION_FILE);
    if path.is_symlink() {
        return Err(CoreError::Symlink(path.display().to_string()));
    }
    let text = fs::read_to_string(&path).map_err(|e| CoreError::io(&path, e))?;
    let updated = rewrite(&path, &text, |tree| {
        let fits = |value: Option<&Value>, shape: fn(&Value) -> bool| {
            value.is_none_or(|value| matches!(value, Value::Null) || shape(value))
        };
        let extensions = tree.get("extensions");
        let bruno = extensions.and_then(Value::as_map).and_then(|e| e.get("bruno"));
        let ignore = bruno.and_then(Value::as_map).and_then(|b| b.get("ignore"));
        let is_map = |value: &Value| matches!(value, Value::Map(_));
        let is_seq = |value: &Value| matches!(value, Value::Seq(_));
        if !(fits(extensions, is_map) && fits(bruno, is_map) && fits(ignore, is_seq)) {
            return Err(invalid(&path, "« extensions.bruno.ignore » doit être une liste dans des tables"));
        }
        let bruno = tree.map_mut_or_insert("extensions", COLLECTION_ORDER).map_mut_or_insert("bruno", &[]);
        let mut ignore = bruno.seq("ignore").to_vec();
        if ignore.iter().any(|entry| entry.scalar().as_deref() == Some(name)) {
            return Ok(false);
        }
        ignore.push(Value::str(name));
        bruno.set("ignore", Value::Seq(ignore), BRUNO_ORDER);
        Ok(true)
    })?;
    match updated == text {
        true => Ok(()),
        false => write_atomic(root, &path, &updated),
    }
}

/// Écrit via un fichier temporaire voisin, synchronisé sur le disque, puis renomme, sans jamais laisser un fichier
/// à moitié écrit. Le fichier garde les permissions de celui qu'il remplace, et un lien symbolique est suivi : c'est
/// sa cible qui est écrite, le lien reste en place. Rien n'est jamais écrit hors de `root` : un lien, ou un dossier
/// lien, dont la cible sort de la collection est refusé. Le nom temporaire ne dépend pas de celui du fichier, qui
/// peut déjà occuper les 255 octets permis.
pub fn write_atomic(root: &Path, path: &Path, text: &str) -> Result<(), CoreError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = &if path.is_symlink() {
        fs::canonicalize(path).map_err(|e| CoreError::io(path, e))?
    } else {
        path.to_path_buf()
    };
    if !is_inside(root, path) {
        return Err(CoreError::OutsideCollection(path.display().to_string()));
    }
    let permissions = fs::metadata(path).map(|meta| meta.permissions()).ok();
    let dir = path.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(".xc-{}-{}.tmp", std::process::id(), NEXT.fetch_add(1, AtomicOrdering::Relaxed)));
    let staged = fs::File::create(&tmp).and_then(|mut file| {
        file.write_all(text.as_bytes())?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
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

/// [`normalize`] avec le BOM et les fins de ligne de `text` : le fichier tel qu'un enregistrement le réécrirait.
pub fn restyle(text: &str, blank_before: &[&str]) -> Result<String, yaml::YamlError> {
    normalize(without_bom(text), blank_before).map(|out| styled_like(text, out))
}
