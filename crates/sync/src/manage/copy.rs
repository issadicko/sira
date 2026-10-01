//! Copie d'une arborescence, pour dupliquer un dossier et pour déplacer entre deux volumes.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read};
use std::path::Path;

use xc_core::CoreError;

use super::exclusive::rename_new;
use super::info::symlink_error;
use super::{io_error, ManageError};

/// Copie le fichier `from` vers `to`, qui ne doit pas exister (création exclusive : un fichier existant n'est jamais
/// remplacé). Une copie interrompue est supprimée.
pub(super) fn copy_file(from: &Path, to: &Path) -> Result<(), ManageError> {
    let mut source = File::open(from).map_err(|e| CoreError::io(from, e))?;
    let mut copy = OpenOptions::new().write(true).create_new(true).open(to).map_err(|e| io_error(to, e))?;
    let copied = (|| {
        io::copy(&mut source, &mut copy)?;
        copy.set_permissions(source.metadata()?.permissions())?;
        copy.sync_all()
    })();
    copied.map_err(|e| {
        fs::remove_file(to).ok();
        CoreError::io(to, e).into()
    })
}

/// Copie le contenu du dossier `from` dans le dossier existant `to`, tous fichiers compris, sans rien remplacer. Un
/// lien symbolique n'est ni suivi ni copié : il est refusé, plutôt que de lire hors de la collection ou de le perdre
/// en silence ; il en va de même de tout ce qui n'est ni un dossier ni un fichier.
pub(super) fn copy_tree(from: &Path, to: &Path) -> Result<(), ManageError> {
    for entry in fs::read_dir(from).map_err(|e| CoreError::io(from, e))? {
        let entry = entry.map_err(|e| CoreError::io(from, e))?;
        let (source, copy) = (entry.path(), to.join(entry.file_name()));
        let kind = entry.file_type().map_err(|e| CoreError::io(&source, e))?;
        if kind.is_symlink() {
            return Err(symlink_error(&source));
        } else if kind.is_dir() {
            fs::create_dir(&copy).map_err(|e| io_error(&copy, e))?;
            copy_tree(&source, &copy)?;
        } else if kind.is_file() {
            copy_file(&source, &copy)?;
        } else {
            return Err(ManageError::Forbidden(source.display().to_string()));
        }
    }
    Ok(())
}

/// Les deux fichiers ont exactement le même contenu.
fn same_content(a: &Path, b: &Path) -> io::Result<bool> {
    let (mut file_a, mut file_b) = (File::open(a)?, File::open(b)?);
    let mut remaining = file_a.metadata()?.len();
    if remaining != file_b.metadata()?.len() {
        return Ok(false);
    }
    let (mut buffer_a, mut buffer_b) = (vec![0; 1 << 16], vec![0; 1 << 16]);
    while remaining > 0 {
        let size = usize::try_from(remaining).map_or(buffer_a.len(), |left| left.min(buffer_a.len()));
        file_a.read_exact(&mut buffer_a[..size])?;
        file_b.read_exact(&mut buffer_b[..size])?;
        if buffer_a[..size] != buffer_b[..size] {
            return Ok(false);
        }
        remaining -= size as u64;
    }
    Ok(true)
}

/// Les deux éléments ont les mêmes noms, la même nature et le même contenu de fichier, à tous les niveaux.
fn same_tree(a: &Path, b: &Path) -> io::Result<bool> {
    let (meta_a, meta_b) = (fs::symlink_metadata(a)?, fs::symlink_metadata(b)?);
    if meta_a.is_dir() != meta_b.is_dir() {
        return Ok(false);
    }
    if !meta_a.is_dir() {
        return same_content(a, b);
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

fn check(from: &Path, to: &Path) -> Result<(), ManageError> {
    match same_tree(from, to) {
        Ok(true) => Ok(()),
        Ok(false) => Err(ManageError::CopyMismatch(from.display().to_string())),
        Err(e) => Err(CoreError::io(to, e).into()),
    }
}

/// Copie `from` vers `to`, qui ne doit pas exister, vérifie la copie octet par octet, puis supprime la source. La
/// copie incomplète ou différente est supprimée et la source conservée ; seul ce que cette copie a créé est supprimé,
/// jamais une destination qui existait déjà.
fn copy_across(from: &Path, to: &Path) -> Result<(), ManageError> {
    if from.is_dir() {
        fs::create_dir(to).map_err(|e| io_error(to, e))?;
        copy_tree(from, to).and_then(|()| check(from, to)).inspect_err(|_| {
            fs::remove_dir_all(to).ok();
        })?;
        fs::remove_dir_all(from).map_err(|e| CoreError::io(from, e))?;
    } else {
        copy_file(from, to)?;
        check(from, to).inspect_err(|_| {
            fs::remove_file(to).ok();
        })?;
        fs::remove_file(from).map_err(|e| CoreError::io(from, e))?;
    }
    Ok(())
}

/// Déplace `from` vers `to` par un renommage atomique qui ne remplace rien ; si les deux sont sur des volumes
/// différents, par copie vérifiée puis suppression de la source. Une cible déjà prise donne `ManageError::Exists`.
pub(super) fn relocate(from: &Path, to: &Path) -> Result<(), ManageError> {
    match rename_new(from, to) {
        Err(e) if e.kind() == ErrorKind::CrossesDevices => copy_across(from, to),
        moved => moved.map_err(|e| io_error(from, e)),
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
            assert!(matches!(copy_across(&from, &to), Err(ManageError::Core(CoreError::Symlink(_)))));
            assert!(from.join("lien.yml").exists() && !to.exists(), "la copie incomplète est supprimée");
        }
    }

    #[test]
    fn ef_col_01_copy_across_never_touches_an_existing_destination() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a");
        tree(&from);
        let to = dir.path().join("b");
        fs::create_dir_all(to.join("sous")).unwrap();
        fs::write(to.join("sous/mien.txt"), "à moi").unwrap();
        let error = copy_across(&from, &to).unwrap_err();
        assert!(matches!(error, ManageError::Exists(_)), "{error}");
        assert_eq!(fs::read_to_string(to.join("sous/mien.txt")).unwrap(), "à moi");
        assert_eq!(fs::read_dir(&to).unwrap().count(), 1, "rien n'y a été copié");
        assert!(from.join("sous/notes.md").is_file(), "la source est conservée");

        let (file, taken) = (dir.path().join("a.yml"), dir.path().join("b.yml"));
        fs::write(&file, "source").unwrap();
        fs::write(&taken, "à moi").unwrap();
        assert!(matches!(copy_across(&file, &taken), Err(ManageError::Exists(_))));
        assert_eq!(
            (fs::read_to_string(&file).unwrap(), fs::read_to_string(&taken).unwrap()),
            ("source".into(), "à moi".into())
        );
    }

    #[test]
    fn ef_col_01_copy_tree_never_replaces_a_file_that_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        let (from, to) = (dir.path().join("a"), dir.path().join("b"));
        tree(&from);
        fs::create_dir_all(to.join("sous")).unwrap();
        fs::write(to.join("sous/notes.md"), "déjà là").unwrap();
        assert!(matches!(copy_tree(&from, &to), Err(ManageError::Exists(_))));
        assert_eq!(fs::read_to_string(to.join("sous/notes.md")).unwrap(), "déjà là");
    }

    #[test]
    fn ef_col_01_same_tree_compares_the_content_and_not_only_the_size() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        tree(&a);
        tree(&b);
        fs::write(b.join("sous/notes.md"), "# votes").unwrap();
        assert!(!same_tree(&a, &b).unwrap(), "même taille, contenu différent");
        let (big, other) = (dir.path().join("big.bin"), dir.path().join("other.bin"));
        let mut bytes = vec![7_u8; 200_000];
        fs::write(&big, &bytes).unwrap();
        bytes[150_000] = 8;
        fs::write(&other, &bytes).unwrap();
        assert!(!same_tree(&big, &other).unwrap(), "différence au-delà du premier bloc");
        fs::write(&other, vec![7_u8; 200_000]).unwrap();
        assert!(same_tree(&big, &other).unwrap());
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
