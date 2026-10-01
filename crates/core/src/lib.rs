pub mod assert;
pub mod collection;
pub mod prepare;
pub mod pretty;
pub mod request;
pub mod vars;
pub mod yaml;

use std::path::Path;

pub use collection::{
    list_environments, normalize, open_collection, read_environment, read_request, save_request, CollectionInfo,
    EnvVar, TreeItem,
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
    #[error("type de requête non pris en charge pour l'instant : {0}")]
    UnsupportedRequestType(String),
}

impl CoreError {
    pub(crate) fn io(path: &Path, e: std::io::Error) -> Self {
        Self::Io { path: path.display().to_string(), message: e.to_string() }
    }
}
