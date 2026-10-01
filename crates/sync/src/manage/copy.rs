//! Copie d'une arborescence, pour dupliquer un dossier et pour déplacer entre deux volumes.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;

use xc_core::CoreError;

use super::ManageError;

/// Copie le contenu du dossier `from` dans le dossier existant `to`, tous fichiers compris. Un lien symbolique n'est
/// ni suivi ni copié : il est refusé, plutôt que de lire hors de la collection ou de le perdre en silence.
pub(super) fn copy_tree(from: &Path, to: &Path) -> Result<(), ManageError> {
    for entry in fs::read_dir(from).map_err(|e| CoreError::io(from, e))? {
        let entry = entry.map_err(|e| CoreError::io(from, e))?;
        let (source, copy) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().map_err(|e| CoreError::io(&source, e))?;
        if kind.is_symlink() {
            return Err(ManageError::Symlink(source.display().to_string()));
        }
        if kind.is_dir() {
            fs::create_dir(&copy).map_err(|e| CoreError::io(&copy, e))?;
            copy_tree(&source, &copy)?;
        } else {
            fs::copy(&source, &copy).map_err(|e| CoreError::io(&copy, e))?;
        }
    }
    Ok(())
}

/// Les deux éléments ont les mêmes noms et la même taille de fichier, à tous les niveaux.
fn same_tree(a: &Path, b: &Path) -> io::Result<bool> {
    let (meta_a, meta_b) = (fs::symlink_metadata(a)?, fs::symlink_metadata(b)?);
    if meta_a.is_dir() != meta_b.is_dir() {
        return Ok(false);
    }
    if !meta_a.is_dir() {
        return Ok(meta_a.len() == meta_b.len());
    }
    let names = |dir: &Path| -> io::Result<Vec<_>> {
        let mut names = fs::read_dir(dir)?.map(|e| e.map(|e| e.file_name())).collect::<io::Result<Vec<_>>>()?;
        names.sort();
        Ok(names)
    };
    let (names_a, names_b) = (names(a)?, names(b)?);
    if names_a != names_b {
        return Ok(false);
    }
    for name in names_a {
        if !same_tree(&a.join(&name), &b.join(&name))? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn remove(path: &Path) -> io::Result<()> {
    match path.is_dir() {
        true => fs::remove_dir_all(path),
        false => fs::remove_file(path),
    }
}

/// Copie `from` vers `to`, vérifie la copie, puis supprime la source ; la copie incomplète ou différente est
/// supprimée et la source conservée.
fn copy_across(from: &Path, to: &Path) -> Result<(), ManageError> {
    let copied = if from.is_dir() {
        fs::create_dir(to).map_err(|e| CoreError::io(to, e).into()).and_then(|()| copy_tree(from, to))
    } else {
        fs::copy(from, to).map(drop).map_err(|e| CoreError::io(to, e).into())
    };
    let checked = copied.and_then(|()| match same_tree(from, to) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ManageError::CopyMismatch(from.display().to_string())),
        Err(e) => Err(CoreError::io(to, e).into()),
    });
    if let Err(e) = checked {
        remove(to).ok();
        return Err(e);
    }
    Ok(remove(from).map_err(|e| CoreError::io(from, e))?)
}

/// Déplace `from` vers `to` par un renommage atomique ; si les deux sont sur des volumes différents, par copie
/// vérifiée puis suppression de la source.
pub(super) fn relocate(from: &Path, to: &Path) -> Result<(), ManageError> {
    match fs::rename(from, to) {
        Err(e) if e.kind() == ErrorKind::CrossesDevices => copy_across(from, to),
        moved => Ok(moved.map_err(|e| CoreError::io(from, e))?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(dir: &Path) {
        fs::create_dir_all(dir.join("sous")).unwrap();
        fs::write(dir.join("folder.yml"), "info:\n  name: F\n").unwrap();
        fs::write(dir.join("sous/notes.md"), "# notes").unwrap();
        fs::write(dir.join(".env"), "SECRET=1").unwrap();
    }

    #[test]
    fn ef_col_01_copy_across_moves_a_folder_after_checking_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        tree(&from);
        copy_across(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(fs::read_to_string(to.join("sous/notes.md")).unwrap(), "# notes");
        assert_eq!(fs::read_to_string(to.join(".env")).unwrap(), "SECRET=1");
        assert_eq!(fs::read_to_string(to.join("folder.yml")).unwrap(), "info:\n  name: F\n");
    }

    #[test]
    fn ef_col_01_copy_across_moves_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a.yml"), dir.path().join("b.yml"));
        fs::write(&from, "info:\n  name: A\n").unwrap();
        copy_across(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(fs::read_to_string(&to).unwrap(), "info:\n  name: A\n");
    }

    #[test]
    fn ef_col_01_copy_across_keeps_the_source_and_leaves_no_copy_when_it_fails() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a");
        tree(&from);
        let to = dir.path().join("absent/b");
        assert!(copy_across(&from, &to).is_err());
        assert!(from.join("sous/notes.md").is_file() && !to.exists());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(from.join("folder.yml"), from.join("lien.yml")).unwrap();
            let to = dir.path().join("b");
            assert!(matches!(copy_across(&from, &to), Err(ManageError::Symlink(_))));
            assert!(from.join("lien.yml").exists() && !to.exists(), "la copie incomplète est supprimée");
        }
    }

    #[test]
    fn ef_col_01_same_tree_compares_names_kinds_and_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        tree(&a);
        tree(&b);
        assert!(same_tree(&a, &b).unwrap());
        fs::write(b.join("sous/notes.md"), "# notes modifiées").unwrap();
        assert!(!same_tree(&a, &b).unwrap(), "taille différente");
        fs::write(b.join("sous/notes.md"), "# notes").unwrap();
        fs::rename(b.join("sous/notes.md"), b.join("sous/autre.md")).unwrap();
        assert!(!same_tree(&a, &b).unwrap(), "nom différent");
        fs::remove_file(b.join("sous/autre.md")).unwrap();
        fs::create_dir(b.join("sous/notes.md")).unwrap();
        assert!(!same_tree(&a, &b).unwrap(), "nature différente");
    }
}
