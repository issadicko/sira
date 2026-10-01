//! Création des nouvelles requêtes (EF-SYN-02) : dossier du tag, nommage de l'import, `seq` suivant du dossier.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;
use xc_core::collection::{
    count_entries, is_hidden, resolve_visible_path, write_atomic, write_new, COLLECTION_FILE, ENV_DIR, FOLDER_FILE,
    REQUEST_EXT,
};
use xc_core::CoreError;

use super::specs::{tree, Folder, Op};
use super::SyncError;
use crate::import::{join, stem, text, Directory, Slot};
use crate::store::SYNC_DIR;
use crate::stringify;

pub(super) struct Creator<'a> {
    root: &'a Path,
    dirs: HashMap<String, Directory>,
    seqs: HashMap<String, usize>,
    pending: Vec<(PathBuf, String)>,
}

/// Écrit des fichiers indépendants en parallèle : chaque écriture est synchronisée sur le disque, ce qui domine le
/// temps d'une synchro qui touche des centaines de fichiers.
pub(super) fn write_parallel<T: Sync>(
    jobs: &[T],
    write: impl Fn(&T) -> Result<(), CoreError> + Sync,
) -> Result<(), CoreError> {
    let workers = std::thread::available_parallelism().map_or(1, usize::from).clamp(1, 8);
    let size = jobs.len().div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        let handles: Vec<_> =
            jobs.chunks(size).map(|chunk| scope.spawn(|| chunk.iter().try_for_each(&write))).collect();
        handles
            .into_iter()
            .try_for_each(|handle| handle.join().unwrap_or_else(|panic| std::panic::resume_unwind(panic)))
    })
}

impl<'a> Creator<'a> {
    pub fn new(root: &'a Path) -> Self {
        Self { root, dirs: HashMap::new(), seqs: HashMap::new(), pending: Vec::new() }
    }

    /// Écrit les requêtes choisies par [`Creator::create`] (création exclusive : aucun fichier existant n'est écrasé).
    pub fn flush(&mut self) -> Result<(), SyncError> {
        let pending = std::mem::take(&mut self.pending);
        Ok(write_parallel(&pending, |(path, text)| write_new(path, text).map_err(|e| CoreError::io(path, e)))?)
    }

    /// Choisit le fichier de la requête de `op` dans le dossier de son tag et renvoie son chemin relatif ; il est écrit
    /// par [`Creator::flush`]. Un fichier de même nom au contenu identique (au `seq` près) est réutilisé : rejouer une
    /// synchro interrompue ne crée aucun doublon.
    pub fn create(&mut self, op: &Op) -> Result<String, SyncError> {
        let (relative, dir) = self.folder(&op.folders)?;
        let stem = stem(text(&op.item, "name"), REQUEST_EXT, "Untitled Request");
        let wanted = OnceCell::new();
        let slot = |name: &str| match fs::read_to_string(dir.join(name)) {
            Err(_) if !dir.join(name).exists() => Slot::Free,
            Ok(existing) if same_request(&existing, &op.text, &wanted) => Slot::Same,
            _ => Slot::Other,
        };
        let file = self.directory(&relative).claim_reusing(&stem, REQUEST_EXT, slot);
        let path = dir.join(&file);
        if !path.exists() {
            let mut item = op.item.clone();
            item["seq"] = json!(self.next_seq(&relative)?);
            self.pending.push((path, stringify::item(&item)));
        }
        Ok(join(&relative, &file))
    }

    fn directory(&mut self, relative: &str) -> &mut Directory {
        self.dirs.entry(relative.to_owned()).or_insert_with(|| match relative.is_empty() {
            true => Directory::new(&[COLLECTION_FILE, FOLDER_FILE, ENV_DIR, SYNC_DIR]).listed(true),
            false => Directory::new(&[FOLDER_FILE]).listed(false),
        })
    }

    fn next_seq(&mut self, relative: &str) -> Result<usize, SyncError> {
        let count = match self.seqs.get(relative) {
            Some(count) => *count,
            None => count_entries(self.root, relative)?.unwrap_or(0),
        };
        self.seqs.insert(relative.to_owned(), count + 1);
        Ok(count + 1)
    }

    /// Dossier de la collection qui correspond à la chaîne de dossiers du tag, créé avec son `folder.yml` s'il
    /// n'existe pas.
    fn folder(&mut self, chain: &[Folder]) -> Result<(String, PathBuf), SyncError> {
        let mut relative = String::new();
        let mut dir = self.root.to_path_buf();
        for folder in chain {
            let stem = stem(&folder.name, "", "Untitled Folder");
            let at_root = relative.is_empty();
            let name = match existing_folder(&dir, &stem, at_root) {
                Some(name) => name,
                None => {
                    let taken = |name: &str| if dir.join(name).exists() { Slot::Other } else { Slot::Free };
                    let name = self.directory(&relative).claim_reusing(&stem, "", taken);
                    let created = resolve_visible_path(self.root, &join(&relative, &name))?;
                    fs::create_dir_all(&created).map_err(|e| CoreError::io(&created, e))?;
                    if let Some(content) = &folder.file {
                        write_atomic(&created.join(FOLDER_FILE), content)?;
                    }
                    name
                }
            };
            relative = join(&relative, &name);
            dir = resolve_visible_path(self.root, &relative)?;
        }
        Ok((relative, dir))
    }
}

fn existing_folder(dir: &Path, stem: &str, at_root: bool) -> Option<String> {
    let wanted = stem.to_lowercase();
    let entries = fs::read_dir(dir).ok()?;
    entries.flatten().find_map(|entry| {
        let name = entry.file_name().to_string_lossy().into_owned();
        (entry.path().is_dir() && name.to_lowercase() == wanted && !is_hidden(&name, at_root)).then_some(name)
    })
}

fn same_request(existing: &str, wanted: &str, cache: &OnceCell<xc_core::yaml::Map>) -> bool {
    let without_seq = |text: &str| {
        let mut map = tree(text, "").ok()?;
        if let Some(xc_core::yaml::Value::Map(info)) = map.get_mut("info") {
            info.remove("seq");
        }
        Some(map)
    };
    let expected = cache.get_or_init(|| without_seq(wanted).unwrap_or_default());
    without_seq(existing).is_some_and(|map| &map == expected)
}
