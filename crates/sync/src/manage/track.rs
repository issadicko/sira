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

/// Ce qui suit `prefix` dans `file` quand `file` est `prefix` ou s'y trouve. Les noms sont comparés exactement : ce
/// sont les noms réels que l'import, la synchro et les actions écrivent, et deux dossiers qui ne diffèrent que par la
/// casse sont distincts sur un volume qui les distingue.
fn below<'a>(file: &'a str, prefix: &str) -> Option<Vec<&'a str>> {
    let (file, prefix) = (parts(file), parts(prefix));
    (prefix.len() <= file.len() && prefix.iter().zip(&file).all(|(a, b)| a == b)).then(|| file[prefix.len()..].to_vec())
}

impl<'a> Tracking<'a> {
    pub fn load(root: &'a Path) -> Result<Self, ManageError> {
        Ok(Self { root, store: store::read(root)? })
    }

    /// Écrit les entrées changées par `change`. `done` dit ce que l'action a déjà fait quand cette écriture échoue.
    fn rewrite(&self, done: &str, change: impl Fn(&Entry) -> Option<Entry>) -> Result<(), ManageError> {
        let Some(store) = &self.store else { return Ok(()) };
        let operations: Vec<Entry> = store.operations.iter().map(|e| change(e).unwrap_or_else(|| e.clone())).collect();
        if operations == store.operations {
            return Ok(());
        }
        store::write_operations(self.root, store, &operations)
            .map_err(|e| ManageError::SourceNotUpdated { done: done.to_owned(), message: e.to_string() })
    }

    /// Renommage ou déplacement de `from` vers `to` : les entrées dont le fichier est `from` ou s'y trouve suivent.
    pub fn moved(&self, from: &str, to: &str, done: &str) -> Result<(), ManageError> {
        self.rewrite(done, |entry| {
            let rest = below(entry.file.as_deref()?, from)?;
            let file = rest.iter().fold(to.to_owned(), |path, part| join(&path, part));
            Some(Entry { file: Some(file), ..entry.clone() })
        })
    }

    /// Suppression de `path` : les entrées dont le fichier est `path` ou s'y trouve passent à `ignored`, sans fichier.
    pub fn deleted(&self, path: &str) -> Result<(), ManageError> {
        self.rewrite("l'élément est bien dans la corbeille", |entry| {
            below(entry.file.as_deref()?, path).map(|_| Entry::ignored(&entry.key))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ef_syn_01_below_compares_the_names_exactly() {
        assert_eq!(below("api/x.yml", "api"), Some(vec!["x.yml"]));
        assert_eq!(below("api", "api"), Some(vec![]));
        assert_eq!(below("./api//x.yml", "api/"), Some(vec!["x.yml"]));
        assert_eq!(below("Api/x.yml", "api"), None, "deux dossiers qui ne diffèrent que par la casse sont distincts");
        assert_eq!(below("api2/x.yml", "api"), None);
        assert_eq!(below("e\u{301}te\u{301}/x.yml", "\u{e9}t\u{e9}"), None);
        assert_eq!(below("api", "api/x.yml"), None);
    }
}
