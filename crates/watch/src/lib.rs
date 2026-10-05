//! Surveillance d'une collection : les changements du disque, regroupés en lots de chemins relatifs.

use std::collections::BTreeSet;
use std::ffi::OsStr;
use std::path::{Component, Path};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use notify::event::{CreateKind, ModifyKind, RemoveKind};
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Silence qui clôt un lot : un `git checkout` ou un enregistrement d'éditeur produit des dizaines d'événements d'affilée.
const QUIET: Duration = Duration::from_millis(200);
/// Durée maximale d'un lot, pour qu'un flux continu d'écritures finisse par être annoncé.
const LONGEST: Duration = Duration::from_secs(2);
/// Au-delà, le lot n'énumère plus : il se déclare tronqué et chacun relit ce qui l'intéresse.
const MAX_PATHS: usize = 512;

const IGNORED_DIRS: [&str; 3] = [".git", ".oc-sync", "node_modules"];
const IGNORED_SUFFIXES: [&str; 5] = ["~", ".tmp", ".swp", ".swx", ".swo"];
const IGNORED_NAMES: [&str; 3] = [".DS_Store", "Thumbs.db", "4913"];

/// Ce qui a changé depuis le lot précédent.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    /// Chemins relatifs à la collection, séparés par `/`, triés, sans doublon.
    pub paths: Vec<String>,
    /// Le système a perdu des événements ou le lot dépasse `MAX_PATHS` : tout a pu changer.
    pub truncated: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    #[error("dossier introuvable ou illisible : {0}")]
    Root(String),
    #[error("le système a atteint sa limite de fichiers surveillés")]
    TooMany,
    #[error("surveillance impossible : {0}")]
    System(String),
}

impl From<notify::Error> for WatchError {
    fn from(e: notify::Error) -> Self {
        match e.kind {
            notify::ErrorKind::MaxFilesWatch => Self::TooMany,
            notify::ErrorKind::PathNotFound => Self::Root("dossier introuvable".into()),
            notify::ErrorKind::Io(io) => Self::System(io.to_string()),
            _ => Self::System("erreur du système de fichiers".into()),
        }
    }
}

/// Surveillance en cours ; elle s'arrête quand on la lâche.
pub struct Watch {
    _watcher: RecommendedWatcher,
}

impl Watch {
    /// Surveille `root` et tout ce qu'il contient ; `on_batch` est appelé depuis un fil dédié, à chaque lot non vide.
    pub fn start(root: &Path, on_batch: impl Fn(Batch) + Send + 'static) -> Result<Self, WatchError> {
        let canonical = root.canonicalize().map_err(|_| WatchError::Root(root.display().to_string()))?;
        let (sender, events) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(sender, Config::default().with_follow_symlinks(false))?;
        watcher.watch(&canonical, RecursiveMode::Recursive)?;
        thread::spawn(move || announce(&events, &canonical, &on_batch));
        Ok(Self { _watcher: watcher })
    }
}

fn announce(events: &Receiver<notify::Result<Event>>, root: &Path, on_batch: &dyn Fn(Batch)) {
    let mut pending = Batch::default();
    let mut started: Option<Instant> = None;
    loop {
        let received = match started {
            Some(since) => events.recv_timeout(QUIET.min(LONGEST.saturating_sub(since.elapsed()))),
            None => events.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let quiet = match received {
            Ok(event) => {
                if absorb(&mut pending, event, root) {
                    started.get_or_insert_with(Instant::now);
                }
                false
            }
            Err(RecvTimeoutError::Timeout) => true,
            Err(RecvTimeoutError::Disconnected) => {
                flush(&mut pending, on_batch);
                return;
            }
        };
        if quiet || started.is_some_and(|since| since.elapsed() >= LONGEST) {
            flush(&mut pending, on_batch);
            started = None;
        }
    }
}

fn flush(pending: &mut Batch, on_batch: &dyn Fn(Batch)) {
    let batch = std::mem::take(pending);
    if batch.truncated || !batch.paths.is_empty() {
        on_batch(batch);
    }
}

/// Ajoute l'événement au lot ; vrai s'il y change quelque chose.
fn absorb(pending: &mut Batch, event: notify::Result<Event>, root: &Path) -> bool {
    let Ok(event) = event else {
        pending.truncated = true;
        return true;
    };
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    if event.need_rescan() {
        pending.truncated = true;
        return true;
    }
    let removed = matches!(event.kind, EventKind::Remove(_));
    let mut changed = false;
    for path in &event.paths {
        match relative(root, path).filter(|rel| rel.is_empty() || is_relevant(rel, &event.kind)) {
            Some(rel) if rel.is_empty() => {
                pending.truncated |= removed;
                changed |= removed;
            }
            Some(rel) if pending.paths.len() < MAX_PATHS => {
                pending.paths.push(rel);
                changed = true;
            }
            Some(_) => {
                pending.truncated = true;
                changed = true;
            }
            None => {}
        }
    }
    pending.paths.sort_unstable();
    pending.paths.dedup();
    changed
}

/// Chemin relatif à la collection, ou `None` s'il est hors du dossier surveillé, interne (Git, base de synchro) ou temporaire.
pub fn relative(root: &Path, path: &Path) -> Option<String> {
    let parts: Vec<&OsStr> = path
        .strip_prefix(root)
        .ok()?
        .components()
        .filter_map(|part| match part {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    let names: BTreeSet<&str> = parts.iter().filter_map(|part| part.to_str()).collect();
    if IGNORED_DIRS.iter().any(|dir| names.contains(dir)) {
        return None;
    }
    if parts.last().and_then(|name| name.to_str()).is_some_and(is_noise) {
        return None;
    }
    Some(parts.iter().map(|part| part.to_string_lossy()).collect::<Vec<_>>().join("/"))
}

/// Ce qui peut changer l'arbre ou les variables : fichiers YAML, `.env`, création et suppression de dossiers. Les autres fichiers
/// (journaux, fichiers d'IDE, pièces jointes) sont ignorés, de même que la simple modification d'un chemin sans extension, qui
/// est le plus souvent un dossier dont le contenu change.
fn is_relevant(rel: &str, kind: &EventKind) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    if matches!(kind, EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder))
        || name.starts_with(".env")
    {
        return true;
    }
    match Path::new(name).extension().and_then(OsStr::to_str) {
        Some(extension) => matches!(extension, "yml" | "yaml"),
        None => !matches!(kind, EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_) | ModifyKind::Any)),
    }
}

fn is_noise(name: &str) -> bool {
    IGNORED_NAMES.contains(&name)
        || name.starts_with(".#")
        || IGNORED_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn rel(path: &str) -> Option<String> {
        let root = PathBuf::from("/c");
        relative(&root, &root.join(path))
    }

    #[test]
    fn ef_col_03_paths_are_relative_to_the_collection_with_forward_slashes() {
        assert_eq!(rel("Auth/Connexion.yml").as_deref(), Some("Auth/Connexion.yml"));
        assert_eq!(rel("a.yml").as_deref(), Some("a.yml"));
        assert_eq!(rel("environments/dev.yml").as_deref(), Some("environments/dev.yml"));
        assert_eq!(relative(Path::new("/c"), Path::new("/ailleurs/a.yml")), None);
        assert_eq!(relative(Path::new("/c"), Path::new("/c")).as_deref(), Some(""));
    }

    #[test]
    fn ef_col_03_git_sync_state_and_dependencies_are_never_reported() {
        for path in [
            ".git/index",
            ".git/refs/heads/main",
            ".oc-sync/openapi/source.yml",
            "node_modules/x/y.yml",
            "sous/.git/HEAD",
        ] {
            assert_eq!(rel(path), None, "{path}");
        }
        assert_eq!(rel("git/notes.yml").as_deref(), Some("git/notes.yml"));
        assert_eq!(rel("oc-sync.yml").as_deref(), Some("oc-sync.yml"));
    }

    #[test]
    fn ef_col_03_editor_swap_files_and_our_own_staging_files_are_never_reported() {
        for path in
            ["a.yml~", "a.yml.swp", ".a.yml.swx", ".#a.yml", ".xc-123-4.tmp", "sous/.DS_Store", "4913", "Thumbs.db"]
        {
            assert_eq!(rel(path), None, "{path}");
        }
        assert_eq!(rel(".env").as_deref(), Some(".env"));
        assert_eq!(rel("temp.yml").as_deref(), Some("temp.yml"));
    }

    #[test]
    fn ef_col_03_a_batch_lists_each_path_once_in_order_and_truncates_past_the_limit() {
        let root = PathBuf::from("/c");
        let event =
            |name: &str| Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(root.join(name));
        let mut batch = Batch::default();
        assert!(absorb(&mut batch, Ok(event("b.yml")), &root));
        assert!(absorb(&mut batch, Ok(event("a.yml")), &root));
        assert!(absorb(&mut batch, Ok(event("b.yml")), &root));
        assert_eq!(batch, Batch { paths: vec!["a.yml".into(), "b.yml".into()], truncated: false });
        assert!(!absorb(&mut batch, Ok(event(".git/index")), &root));
        for i in 0..MAX_PATHS + 5 {
            absorb(&mut batch, Ok(event(&format!("f{i}.yml"))), &root);
        }
        assert!(batch.truncated);
        assert_eq!(batch.paths.len(), MAX_PATHS);
    }

    #[test]
    fn ef_col_03_only_yaml_env_files_and_folder_changes_wake_the_application() {
        use notify::event::{DataChange, MetadataKind};

        let root = PathBuf::from("/c");
        let reports = |kind: EventKind, name: &str| {
            let mut batch = Batch::default();
            absorb(&mut batch, Ok(Event::new(kind).add_path(root.join(name))), &root);
            !batch.paths.is_empty()
        };
        let data = || EventKind::Modify(ModifyKind::Data(DataChange::Content));
        assert!(
            reports(data(), "a.yml")
                && reports(data(), "sous/b.yaml")
                && reports(data(), ".env")
                && reports(data(), ".env.local")
        );
        for ignored in ["a.json", "notes.md", ".idea/workspace.xml", "journal.log", "pieces/recu.pdf"] {
            assert!(!reports(data(), ignored), "{ignored}");
        }
        assert!(reports(EventKind::Create(CreateKind::Folder), "v1.2"), "un dossier créé à nom pointé");
        assert!(reports(EventKind::Remove(RemoveKind::Folder), "Auth"));
        assert!(
            reports(EventKind::Modify(ModifyKind::Name(notify::event::RenameMode::Both)), "Auth"),
            "dossier renommé"
        );
        assert!(reports(EventKind::Create(CreateKind::Any), "Auth"), "dossier créé, type inconnu (Windows)");
        assert!(
            !reports(EventKind::Modify(ModifyKind::Metadata(MetadataKind::Any)), "Auth"),
            "le contenu d'un dossier change"
        );
        assert!(!reports(data(), "Auth"));
    }

    #[test]
    fn ef_col_03_reads_do_not_wake_the_watch_but_losing_events_forces_a_full_reread() {
        let root = PathBuf::from("/c");
        let mut batch = Batch::default();
        let access = Event::new(EventKind::Access(notify::event::AccessKind::Any)).add_path(root.join("a.yml"));
        assert!(!absorb(&mut batch, Ok(access), &root));
        assert_eq!(batch, Batch::default());
        let failure = notify::Error::generic("débordement");
        assert!(absorb(&mut batch, Err(failure), &root));
        assert!(batch.truncated);
    }

    #[test]
    fn ef_col_03_removing_the_collection_folder_itself_truncates() {
        let root = PathBuf::from("/c");
        let mut batch = Batch::default();
        let removed = Event::new(EventKind::Remove(notify::event::RemoveKind::Folder)).add_path(root.clone());
        assert!(absorb(&mut batch, Ok(removed), &root));
        assert!(batch.truncated);
        let touched = Event::new(EventKind::Modify(notify::event::ModifyKind::Any)).add_path(root.clone());
        let mut calm = Batch::default();
        assert!(!absorb(&mut calm, Ok(touched), &root));
    }
}
