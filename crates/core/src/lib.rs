pub mod assert;
pub mod collection;
pub mod prepare;
pub mod pretty;
pub mod request;
pub mod vars;
pub mod yaml;

use std::io::ErrorKind;
use std::path::Path;

pub use collection::{
    list_environments, mark_deprecated, normalize, open_collection, read_environment, read_request, restyle,
    save_environment, save_request, set_default_environment, CollectionInfo, EnvVar, TreeItem,
};
pub use prepare::{prepare, Prepared};
pub use request::{Assertion, Auth, Body, KeyValue, MultipartField, MultipartValue, Param, ParamKind, RequestDoc};

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{path} : {message}")]
    Io { path: String, message: String },
    #[error("{path} : {message}")]
    Yaml { path: String, message: String },
    #[error("{0} n'est pas une collection OpenCollection (opencollection.yml absent)")]
    NotACollection(String),
    #[error("chemin hors de la collection : {0}")]
    OutsideCollection(String),
    #[error("chemin caché refusé : {0}")]
    HiddenPath(String),
    #[error("{0} est un lien symbolique : il n'est jamais suivi ni réécrit")]
    Symlink(String),
    #[error("{path} : le corps de la requête dépasse {max_mb} Mo")]
    BodyTooLarge { path: String, max_mb: u64 },
    #[error("type de requête non pris en charge pour l'instant : {0}")]
    UnsupportedRequestType(String),
    #[error("{path} n'est pas une requête : {reason}")]
    NotARequest { path: String, reason: String },
    #[error("nom d'environnement invalide : {0}")]
    InvalidEnvironment(String),
}

impl CoreError {
    pub fn io(path: &Path, e: std::io::Error) -> Self {
        Self::Io { path: path.display().to_string(), message: io_message(&e) }
    }
}

/// Raison en français d'une erreur du système, pour les erreurs courantes ; les autres sont données par leur code.
pub fn io_message(e: &std::io::Error) -> String {
    let known = match e.kind() {
        ErrorKind::NotFound => "introuvable",
        ErrorKind::PermissionDenied => "permission refusée",
        ErrorKind::AlreadyExists => "existe déjà",
        ErrorKind::DirectoryNotEmpty => "dossier non vide",
        ErrorKind::NotADirectory => "n'est pas un dossier",
        ErrorKind::IsADirectory => "est un dossier",
        ErrorKind::ReadOnlyFilesystem => "volume en lecture seule",
        ErrorKind::StorageFull => "volume plein",
        ErrorKind::QuotaExceeded => "quota du volume dépassé",
        ErrorKind::CrossesDevices => "volumes différents",
        ErrorKind::InvalidFilename => "nom de fichier invalide ou trop long",
        ErrorKind::InvalidData => "contenu illisible (texte UTF-8 attendu)",
        ErrorKind::ResourceBusy => "ressource occupée",
        ErrorKind::Interrupted => "opération interrompue",
        ErrorKind::TimedOut => "délai dépassé",
        _ => {
            return e.raw_os_error().map_or_else(|| "erreur d'entrée-sortie".into(), |c| format!("erreur système {c}"))
        }
    };
    known.into()
}
