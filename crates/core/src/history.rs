//! Historique local des requêtes envoyées : un fichier par collection, dans le dossier de données de l'application
//! (jamais dans la collection, qui se versionne). Il garde l'adresse telle que saisie, avec ses `{{variables}}`
//! non résolues, pour qu'aucun secret du trousseau n'y soit écrit.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::collection::write_atomic;
use crate::CoreError;

pub const MAX_ENTRIES: usize = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub env: Option<String>,
    /// Absent quand aucune réponse n'est venue (erreur d'envoi, requête ignorée par un script).
    pub status: Option<u16>,
    pub error: Option<String>,
    pub duration_ms: f64,
    pub size: u64,
    pub at: String,
}

/// L'historique de la collection dont la racine est `root`, rangé sous `data_dir`.
pub struct History {
    folder: PathBuf,
    file: PathBuf,
}

fn key(root: &str) -> String {
    let hash = root.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

impl History {
    pub fn of(data_dir: &Path, root: &str) -> Self {
        let folder = data_dir.join("history");
        let file = folder.join(format!("{}.json", key(root)));
        Self { folder, file }
    }

    /// Les entrées, la plus récente d'abord ; vide si le fichier manque ou est illisible.
    pub fn list(&self) -> Vec<Entry> {
        fs::read_to_string(&self.file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default()
    }

    /// Ajoute `entry` en tête et ne garde que les [`MAX_ENTRIES`] plus récentes.
    pub fn push(&self, entry: Entry) -> Result<Vec<Entry>, CoreError> {
        let mut entries = self.list();
        entries.insert(0, entry);
        entries.truncate(MAX_ENTRIES);
        self.save(&entries)?;
        Ok(entries)
    }

    /// Les entrées d'un élément renommé ou déplacé le suivent : celles de la requête `from` passent à `to`, celles des
    /// requêtes d'un dossier `from` à `to/…`. `name` est le nouveau nom d'une requête renommée (le nom affiché change
    /// même quand son fichier garde le sien). Rien n'est écrit quand aucune entrée n'est touchée ; rend le nombre
    /// d'entrées modifiées.
    pub fn follow(&self, from: &str, to: &str, name: Option<&str>) -> Result<usize, CoreError> {
        let mut entries = self.list();
        let inside = format!("{from}/");
        let mut touched = 0;
        for entry in &mut entries {
            let before = entry.clone();
            if entry.path == from {
                to.clone_into(&mut entry.path);
                if let Some(name) = name {
                    name.clone_into(&mut entry.name);
                }
            } else if let Some(rest) = entry.path.strip_prefix(&inside) {
                entry.path = format!("{to}/{rest}");
            }
            touched += usize::from(*entry != before);
        }
        if touched > 0 {
            self.save(&entries)?;
        }
        Ok(touched)
    }

    pub fn clear(&self) -> Result<(), CoreError> {
        match fs::remove_file(&self.file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(CoreError::io(&self.file, e)),
            _ => Ok(()),
        }
    }

    fn save(&self, entries: &[Entry]) -> Result<(), CoreError> {
        fs::create_dir_all(&self.folder).map_err(|e| CoreError::io(&self.folder, e))?;
        let text = serde_json::to_string(entries)
            .map_err(|e| CoreError::Io { path: self.file.display().to_string(), message: e.to_string() })?;
        write_atomic(&self.folder, &self.file, &text)
    }
}
