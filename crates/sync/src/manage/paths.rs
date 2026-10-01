//! Chemins relatifs reçus de l'interface : confinement à la collection et élément visé.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use xc_core::collection::{
    ensure_collection, ignored_names, is_hidden, resolve_path, COLLECTION_FILE, ENV_DIR, FOLDER_FILE, MOCKS_DIR,
    REQUEST_EXT,
};
use xc_core::CoreError;

use super::info::Kind;
use super::names::taken_names;
use super::ManageError;
use crate::import::{fold, Directory};
use crate::store::SYNC_DIR;

/// Requête (fichier `.yml`) ou dossier visé par une action, avec son chemin relatif.
pub(super) struct Item {
    pub relative: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_link: bool,
}

impl Item {
    /// Fichier qui porte `info` : la requête, ou le `folder.yml` d'un dossier.
    pub fn info_file(&self) -> PathBuf {
        if self.is_dir {
            self.path.join(FOLDER_FILE)
        } else {
            self.path.clone()
        }
    }

    pub fn kind(&self) -> Kind {
        if self.is_dir {
            Kind::Folder
        } else {
            Kind::Request
        }
    }

    /// Un lien symbolique n'est ni renommé, ni déplacé, ni dupliqué : il pourrait mener hors de la collection.
    pub fn refuse_link(&self) -> Result<(), ManageError> {
        match self.is_link {
            true => Err(CoreError::Symlink(self.relative.clone()).into()),
            false => Ok(()),
        }
    }
}

/// Sépare le dossier parent (`""` pour la racine) et le nom d'un chemin relatif.
pub(super) fn split(relative: &str) -> (&str, &str) {
    relative.rsplit_once('/').unwrap_or(("", relative))
}

/// Collection sur laquelle agit une action : sa racine, les noms que `extensions.bruno.ignore` retire de l'arbre, et
/// les éléments réservés de la racine, reconnus par leur identité de fichier et non par leur seul nom.
pub(super) struct Scope<'a> {
    pub root: &'a Path,
    ignore: Vec<String>,
    protected: Vec<PathBuf>,
}

impl<'a> Scope<'a> {
    /// Refuse un `root` qui ne contient pas `opencollection.yml`. Les éléments réservés sont ceux que la racine porte
    /// sous ce nom exact : sur un volume insensible à la casse, un dossier `Mocks` que l'arbre montre n'est pas
    /// `mocks`.
    pub fn open(root: &'a Path) -> Result<Self, ManageError> {
        ensure_collection(root)?;
        let entries = fs::read_dir(root).map_err(|e| CoreError::io(root, e))?;
        let reserved = [COLLECTION_FILE, ENV_DIR, MOCKS_DIR, SYNC_DIR];
        let protected = entries
            .flatten()
            .filter(|entry| reserved.iter().any(|name| entry.file_name() == **name))
            .map(|entry| entry.path())
            .collect();
        Ok(Self { root, ignore: ignored_names(root)?, protected })
    }

    /// Les noms d'un dossier de `dir` à ne pas attribuer : déjà pris (`except` : l'élément qu'on renomme), cachés ou
    /// ignorés.
    pub fn directory(&self, dir: &Path, at_root: bool, except: Option<&str>) -> Result<Directory, ManageError> {
        Ok(taken_names(dir, except)?.listed(at_root).ignoring(&self.ignore))
    }

    /// L'arbre montre `name` : ni caché ni réservé (même prédicat que l'arbre, casse comprise), ni ignoré.
    fn is_listed(&self, name: &str, at_root: bool) -> bool {
        !is_hidden(name, at_root) && !self.ignore.iter().any(|ignored| ignored == name)
    }

    /// Refuse un nom à attribuer que l'arbre ne montrerait pas parce qu'il est dans la liste `ignore`, sans tenir
    /// compte de la casse ni de la normalisation Unicode, comme pour les noms cachés.
    pub fn check_name(&self, name: &str) -> Result<(), ManageError> {
        let folded = fold(name);
        match self.ignore.iter().any(|ignored| fold(ignored) == folded) {
            true => Err(ManageError::InvalidName(format!("« {name} » est dans la liste ignore de la collection"))),
            false => Ok(()),
        }
    }

    fn is_protected(&self, path: &Path) -> bool {
        self.protected.iter().any(|reserved| same_file::is_same_file(path, reserved).unwrap_or(false))
    }

    /// Chemin d'un dossier ou d'un élément que l'arbre montre : canonique (segments non vides, ni `.` ni `\`, pas de
    /// `/` final), ni hors de la collection (`..`, chemin absolu, lien symbolique qui sort), ni caché, réservé ou
    /// ignoré à aucun niveau du chemin, ni à travers un dossier lien symbolique, et sans être, par son identité de
    /// fichier, `opencollection.yml`, `environments`, `mocks` ou `.oc-sync` (un alias Unicode, un nom 8.3 ou un point
    /// final sous Windows). `""` désigne la racine.
    pub fn visible(&self, relative: &str) -> Result<PathBuf, ManageError> {
        let path = resolve_path(self.root, relative)?;
        if relative.is_empty() {
            return Ok(path);
        }
        let mut at = self.root.to_path_buf();
        let last = relative.split('/').count() - 1;
        for (depth, name) in relative.split('/').enumerate() {
            if name.is_empty() || name == "." || name.contains('\\') {
                return Err(ManageError::InvalidPath(relative.to_owned()));
            }
            at.push(name);
            if !self.is_listed(name, depth == 0) || self.is_protected(&at) {
                return Err(ManageError::Forbidden(relative.to_owned()));
            }
            if depth < last && fs::symlink_metadata(&at).is_ok_and(|meta| meta.is_symlink()) {
                return Err(CoreError::Symlink(relative.to_owned()).into());
            }
        }
        Ok(path)
    }

    /// La requête ou le dossier `relative`, qui doit exister ; la racine n'est jamais une cible.
    pub fn locate(&self, relative: &str) -> Result<Item, ManageError> {
        if relative.is_empty() {
            return Err(ManageError::Forbidden("la racine de la collection".into()));
        }
        let path = self.visible(relative)?;
        let meta = fs::symlink_metadata(&path).map_err(|e| match e.kind() {
            ErrorKind::NotFound => ManageError::NotFound(relative.to_owned()),
            _ => CoreError::io(&path, e).into(),
        })?;
        if !meta.is_dir() && !relative.ends_with(REQUEST_EXT) {
            return Err(ManageError::Forbidden(relative.to_owned()));
        }
        Ok(Item { relative: relative.to_owned(), path, is_dir: meta.is_dir(), is_link: meta.is_symlink() })
    }
}
