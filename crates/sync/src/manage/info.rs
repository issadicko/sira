//! Réécriture des champs `info` d'une requête ou d'un `folder.yml`, vérifiée juste avant d'écrire.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use xc_core::collection::{info_type, with_info, write_atomic, write_new, REQUEST_KINDS};
use xc_core::yaml::Value;
use xc_core::CoreError;

use super::{io_error, ManageError};
use crate::sync::write_parallel;

/// Ce qu'un fichier est censé être : seules les requêtes (`info.type` connu) et les `folder.yml` sont réécrits.
#[derive(Clone, Copy)]
pub(super) enum Kind {
    Request,
    Folder,
}

/// Écriture préparée d'un fichier : son texte à la lecture (`None` : le fichier n'existait pas) et son nouveau texte.
pub(super) struct Edit {
    file: PathBuf,
    before: Option<String>,
    after: String,
}

/// Ce qu'une écriture ferait d'un fichier.
pub(super) enum Update {
    Write(Edit),
    Keep,
    Link,
    Foreign,
}

pub(super) fn symlink_error(file: &Path) -> ManageError {
    CoreError::Symlink(file.display().to_string()).into()
}

/// Prépare le changement de `fields` dans `info` de `file`. `Link` : c'est un lien symbolique, jamais réécrit.
/// `Foreign` : un fichier illisible, de plusieurs documents, dont `info` n'est pas une table, ou qui n'est pas une
/// requête (`info.type` inconnu) : il n'est pas touché. Un `folder.yml` absent prend le texte `created`.
pub(super) fn plan(file: &Path, kind: Kind, fields: &[(&str, Value)], created: Option<String>) -> Update {
    match fs::symlink_metadata(file) {
        Ok(meta) if meta.is_symlink() => return Update::Link,
        Ok(_) => {}
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let edit = |after| Edit { file: file.to_path_buf(), before: None, after };
            return created.map_or(Update::Foreign, |after| Update::Write(edit(after)));
        }
        Err(_) => return Update::Foreign,
    }
    let Ok(text) = fs::read_to_string(file) else { return Update::Foreign };
    let known = match kind {
        Kind::Folder => true,
        Kind::Request => {
            info_type(file, &text).ok().flatten().is_some_and(|kind| REQUEST_KINDS.contains(&kind.as_str()))
        }
    };
    match with_info(file, &text, fields) {
        Ok(after) if known && after == text => Update::Keep,
        Ok(after) if known => Update::Write(Edit { file: file.to_path_buf(), before: Some(text), after }),
        _ => Update::Foreign,
    }
}

impl Edit {
    pub fn into_text(self) -> String {
        self.after
    }

    fn current(&self) -> Result<Option<String>, ManageError> {
        match fs::symlink_metadata(&self.file) {
            Ok(meta) if meta.is_symlink() => Err(symlink_error(&self.file)),
            Ok(_) => fs::read_to_string(&self.file).map(Some).map_err(|e| io_error(&self.file, e)),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error(&self.file, e)),
        }
    }

    fn verify(&self) -> Result<(), ManageError> {
        match self.current()? == self.before {
            true => Ok(()),
            false => Err(ManageError::Changed(self.file.display().to_string())),
        }
    }

    fn write(&self, root: &Path) -> Result<(), ManageError> {
        self.verify()?;
        match self.before {
            Some(_) => Ok(write_atomic(root, &self.file, &self.after)?),
            None => write_new(&self.file, &self.after).map_err(|e| io_error(&self.file, e)),
        }
    }
}

/// Écrit les fichiers, qui doivent tous être encore tels qu'à la lecture : chacun est relu juste avant d'être écrit,
/// et un fichier qui a changé entre-temps annule l'action. Chaque écriture est synchronisée sur le disque (écart de
/// vitesse assumé : la sûreté prime).
pub(super) fn commit(root: &Path, edits: &[Edit]) -> Result<(), ManageError> {
    edits.iter().try_for_each(Edit::verify)?;
    write_parallel(edits, |edit| edit.write(root))
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: &str = "info:\n  name: A\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"\"\n";
    const SEQ: fn(i64) -> [(&'static str, Value); 1] = |n| [("seq", Value::Int(n))];

    fn written(update: Update) -> Edit {
        match update {
            Update::Write(edit) => edit,
            _ => panic!("une écriture était attendue"),
        }
    }

    #[test]
    fn ef_col_04_plan_rewrites_only_requests_and_folders() {
        let dir = tempfile::tempdir().unwrap();
        let file = |name: &str, text: &str| {
            let path = dir.path().join(name);
            fs::write(&path, text).unwrap();
            path
        };
        let edit = written(plan(&file("a.yml", REQUEST), Kind::Request, &SEQ(2), None));
        assert_eq!(edit.into_text(), REQUEST.replace("seq: 1", "seq: 2"));
        assert!(matches!(plan(&file("b.yml", REQUEST), Kind::Request, &SEQ(1), None), Update::Keep));
        for kind in ["graphql", "grpc", "websocket"] {
            let typed = REQUEST.replace("type: http", &format!("type: {kind}"));
            assert!(matches!(plan(&file("t.yml", &typed), Kind::Request, &SEQ(2), None), Update::Write(_)), "{kind}");
        }
        let foreign = [
            "openapi: 3.0.0\ninfo:\n  title: Petstore\n  version: 1.0.0\n",
            "info:\n  name: Sans type\n",
            "info:\n  name: X\n  type: folder\n",
            "info: [non fermé\n",
            "info: texte\n",
            "info:\n  name: A\n  type: http\n---\nautre: document\n",
            "- a\n- b\n",
        ];
        for text in foreign {
            let path = file("f.yml", text);
            assert!(matches!(plan(&path, Kind::Request, &SEQ(2), None), Update::Foreign), "{text}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
        fs::write(dir.path().join("octets.yml"), [0xff_u8, 0xfe, 0x00]).unwrap();
        assert!(matches!(plan(&dir.path().join("octets.yml"), Kind::Request, &SEQ(2), None), Update::Foreign));
        let folder = file("folder.yml", "info:\n  name: F\n");
        assert!(matches!(plan(&folder, Kind::Folder, &SEQ(2), None), Update::Write(_)));
        let missing = dir.path().join("absent/folder.yml");
        assert!(matches!(plan(&missing, Kind::Folder, &SEQ(2), None), Update::Foreign));
        assert_eq!(written(plan(&missing, Kind::Folder, &SEQ(2), Some("créé".into()))).into_text(), "créé");
    }

    #[cfg(unix)]
    #[test]
    fn enf_sec_01_plan_never_rewrites_a_symbolic_link() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("real.yml"), REQUEST).unwrap();
        std::os::unix::fs::symlink("real.yml", dir.path().join("lien.yml")).unwrap();
        assert!(matches!(plan(&dir.path().join("lien.yml"), Kind::Request, &SEQ(2), None), Update::Link));
        let edit = written(plan(&dir.path().join("real.yml"), Kind::Request, &SEQ(2), None));
        fs::remove_file(dir.path().join("real.yml")).unwrap();
        std::os::unix::fs::symlink("lien.yml", dir.path().join("real.yml")).unwrap();
        let error = commit(dir.path(), &[edit]).unwrap_err();
        assert!(matches!(error, ManageError::Core(CoreError::Symlink(_))), "{error}");
    }

    #[test]
    fn ef_col_04_commit_refuses_a_file_that_changed_since_it_was_read_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.yml"), dir.path().join("b.yml"));
        fs::write(&a, REQUEST).unwrap();
        fs::write(&b, REQUEST).unwrap();
        let edits = [written(plan(&a, Kind::Request, &SEQ(2), None)), written(plan(&b, Kind::Request, &SEQ(3), None))];
        let edited = REQUEST.replace("name: A", "name: modifié ailleurs");
        fs::write(&b, &edited).unwrap();
        let error = commit(dir.path(), &edits).unwrap_err();
        assert!(matches!(&error, ManageError::Changed(file) if file.ends_with("b.yml")), "{error}");
        assert!(error.is_input() && error.to_string().contains("a changé depuis sa lecture"), "{error}");
        assert_eq!(fs::read_to_string(&a).unwrap(), REQUEST, "rien n'est écrit quand un fichier a changé");
        assert_eq!(fs::read_to_string(&b).unwrap(), edited);
        fs::write(&b, REQUEST).unwrap();
        commit(dir.path(), &edits).unwrap();
        assert_eq!(fs::read_to_string(&a).unwrap(), REQUEST.replace("seq: 1", "seq: 2"));
        assert_eq!(fs::read_to_string(&b).unwrap(), REQUEST.replace("seq: 1", "seq: 3"));
    }

    #[test]
    fn ef_col_04_a_missing_folder_yml_is_created_exclusively() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("folder.yml");
        let edit = written(plan(&file, Kind::Folder, &SEQ(1), Some("info:\n  name: F\n".into())));
        fs::write(&file, "créé entre-temps").unwrap();
        assert!(matches!(commit(dir.path(), &[edit]), Err(ManageError::Changed(_))));
        assert_eq!(fs::read_to_string(&file).unwrap(), "créé entre-temps");
        fs::remove_file(&file).unwrap();
        let edit = written(plan(&file, Kind::Folder, &SEQ(1), Some("info:\n  name: F\n".into())));
        commit(dir.path(), &[edit]).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), "info:\n  name: F\n");
    }
}
