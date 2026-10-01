//! Mise à jour de `.oc-sync/openapi/source.yml` (§ 6) quand une action touche une requête suivie par la synchro.

use std::path::Path;

use crate::import::join;
use crate::store::{self, Entry, Store};

use super::ManageError;

/// `source.yml` de la collection, lu avant l'action : un fichier illisible la refuse avant toute écriture. Il n'y en
/// a pas pour une collection qui n'est pas connectée.
pub(super) struct Tracking<'a> {
    root: &'a Path,
    store: Option<Store>,
}

fn parts(path: &str) -> Vec<&str> {
    path.split('/').filter(|part| !part.is_empty() && *part != ".").collect()
}

/// Ce qui suit `prefix` dans `file` quand `file` est `prefix` ou s'y trouve, sans tenir compte de la casse comme le
/// fait la synchro.
fn below<'a>(file: &'a str, prefix: &str) -> Option<Vec<&'a str>> {
    let (file, prefix) = (parts(file), parts(prefix));
    let same = |(a, b): (&&str, &&str)| a.to_lowercase() == b.to_lowercase();
    (prefix.len() <= file.len() && prefix.iter().zip(&file).all(same)).then(|| file[prefix.len()..].to_vec())
}

impl<'a> Tracking<'a> {
    pub fn load(root: &'a Path) -> Result<Self, ManageError> {
        Ok(Self { root, store: store::read(root)? })
    }

    fn rewrite(&self, change: impl Fn(&Entry) -> Option<Entry>) -> Result<(), ManageError> {
        let Some(store) = &self.store else { return Ok(()) };
        let operations: Vec<Entry> = store.operations.iter().map(|e| change(e).unwrap_or_else(|| e.clone())).collect();
        if operations != store.operations {
            store::write_operations(self.root, store, &operations)?;
        }
        Ok(())
    }

    /// Renommage ou déplacement de `from` vers `to` : les entrées dont le fichier est `from` ou s'y trouve suivent.
    pub fn moved(&self, from: &str, to: &str) -> Result<(), ManageError> {
        self.rewrite(|entry| {
            let rest = below(entry.file.as_deref()?, from)?;
            let file = rest.iter().fold(to.to_owned(), |path, part| join(&path, part));
            Some(Entry { file: Some(file), ..entry.clone() })
        })
    }

    /// Suppression de `path` : les entrées dont le fichier est `path` ou s'y trouve passent à `ignored`, sans fichier.
    pub fn deleted(&self, path: &str) -> Result<(), ManageError> {
        self.rewrite(|entry| below(entry.file.as_deref()?, path).map(|_| Entry::ignored(&entry.key)))
    }
}
