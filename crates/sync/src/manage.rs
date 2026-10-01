//! Gestion de collection (`docs/docs/gestion-collection.md`, EF-COL-01, EF-COL-04, EF-SYN-01) : ouvrir un dossier qui
//! n'est pas une collection, créer une collection, une requête ou un dossier, renommer, dupliquer, supprimer, réordonner
//! et déplacer.
//!
//! Les fichiers créés sont ceux de Bruno (`stringify`). Les fichiers modifiés ne changent que sur les lignes concernées
//! (`info.name`, `info.seq`) : le reste de l'arbre YAML, clés inconnues comprises, est conservé, et seuls une requête
//! (`info.type` connu) et un `folder.yml` sont réécrits. Écarts voulus avec Bruno, au profit du principe « non
//! destructif » : la suppression passe par la corbeille du système, le renommage et le déplacement ne remplacent jamais
//! une cible, un dossier dupliqué est copié en entier et un lien symbolique n'est jamais suivi ni réécrit. Quand la
//! collection est connectée à une spec OpenAPI, chaque action met `.oc-sync/openapi/source.yml` à jour en dernier.

mod collection;
mod copy;
mod exclusive;
mod info;
mod item;
mod names;
mod paths;
mod place;
mod track;

use std::io::{self, ErrorKind};
use std::path::Path;

use serde::{Deserialize, Serialize};
use xc_core::CoreError;

pub use collection::{create_collection, init_collection, inspect_folder};
pub use item::{clone_item, create_folder, create_request, delete_item, rename_item};
pub use place::move_item;

/// Ce que contient un dossier choisi pour être ouvert comme collection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FolderKind {
    Collection,
    Empty,
    Bru,
    Other,
}

/// Où déposer un élément par rapport à la cible : avant ou après un frère, ou dans un dossier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DropPosition {
    Before,
    After,
    Inside,
}

#[derive(Debug, thiserror::Error)]
pub enum ManageError {
    #[error("nom invalide : {0}")]
    InvalidName(String),
    #[error("chemin invalide (segment vide, « . », « \\ » ou « / » final) : {0}")]
    InvalidPath(String),
    #[error("dossier introuvable : {0}")]
    FolderNotFound(String),
    #[error("élément introuvable dans la collection : {0}")]
    NotFound(String),
    #[error("action refusée sur {0}")]
    Forbidden(String),
    #[error("{0} contient déjà une collection (opencollection.yml)")]
    AlreadyCollection(String),
    #[error("un dossier ne peut pas être déplacé dans lui-même ni dans un de ses descendants : {0}")]
    IntoItself(String),
    #[error("{0} n'est ni une requête ni un dossier OpenCollection : son ordre ne peut pas être modifié")]
    NotRequest(String),
    #[error("{0} existe déjà")]
    Exists(String),
    #[error("{0} a changé depuis sa lecture : action annulée, rien n'a été écrasé")]
    Changed(String),
    #[error("la copie de {0} diffère de l'original : l'original est conservé")]
    CopyMismatch(String),
    #[error("envoi à la corbeille impossible : {0}")]
    Trash(String),
    #[error("{done}, mais .oc-sync/openapi/source.yml n'a pas pu être mis à jour : {message}")]
    SourceNotUpdated { done: String, message: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

/// Erreur du système sur `path` : « existe déjà » est distinguée, car elle fait choisir le nom suivant.
fn io_error(path: &Path, e: io::Error) -> ManageError {
    match e.kind() {
        ErrorKind::AlreadyExists => ManageError::Exists(path.display().to_string()),
        _ => CoreError::io(path, e).into(),
    }
}

impl ManageError {
    /// L'entrée (nom, chemin, dossier, fichier ou lien de la collection) est en cause, pas une écriture sur le disque.
    pub fn is_input(&self) -> bool {
        matches!(
            self,
            Self::InvalidName(_)
                | Self::InvalidPath(_)
                | Self::FolderNotFound(_)
                | Self::NotFound(_)
                | Self::Forbidden(_)
                | Self::AlreadyCollection(_)
                | Self::IntoItself(_)
                | Self::NotRequest(_)
                | Self::Changed(_)
                | Self::Core(
                    CoreError::NotACollection(_)
                        | CoreError::OutsideCollection(_)
                        | CoreError::HiddenPath(_)
                        | CoreError::Symlink(_)
                        | CoreError::Yaml { .. }
                )
        )
    }
}
