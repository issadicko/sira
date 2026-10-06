//! Imports vers une collection : écriture d'une collection OpenAPI convertie (gestionnaire
//! `renderer:import-collection` de Bruno) avec son stockage `.oc-sync/openapi/` (voir `store`),
//! récupération de la spec (fichier ou URL) et requêtes créées depuis une commande cURL.
//!
//! Écarts voulus avec Bruno, tous au profit du principe « non destructif » : deux éléments dont les noms
//! assainis se confondent (sans tenir compte de la casse ni de la normalisation Unicode) reçoivent un suffixe
//! ` 1`, ` 2`… au lieu de s'écraser, les noms réservés de la collection (`opencollection.yml`, `folder.yml`,
//! `environments`, `.oc-sync`), les noms que l'arbre cacherait (`node_modules`, `mocks` à la racine, un point de tête
//! qui est retiré) et les noms de périphériques Windows (`con`, `nul`, `com1`…) sont évités de même, et les noms trop
//! longs sont tronqués aussi en octets. Un nom réduit à des caractères interdits devient `Untitled Request`,
//! `Untitled Folder`, `Untitled Environment` ou `Untitled Collection`. L'import se construit dans un dossier de
//! préparation caché, renommé à la fin ; `source.yml` ne contient ni identifiants d'URL ni paramètres secrets.

mod from_curl;
mod naming;
mod source;
mod write;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Serialize;
use serde_json::Value;
use xc_core::CoreError;

use crate::curl::MAX_COMMAND_BYTES;
use crate::insomnia::{self, InsomniaError};
use crate::openapi::{load_spec, summary, to_bruno, GroupBy, OpenApiError, SpecSummary};
use crate::postman::{self, Issue, PostmanError};

pub use from_curl::{create_request_from_curl, request_doc_from_curl};
pub(crate) use naming::{
    fit, fold, folder_dir_name, is_device_name, request_file_name, sanitize_name, stem, validate_name, Directory, Slot,
};
pub use source::{fetch_spec, is_url, source_value};
pub(crate) use write::{folder_file, REQUEST_TYPES};
pub use write::{write_collection, write_plain_collection};

#[derive(Debug, thiserror::Error)]
pub enum ImportError {
    #[error("source de la spécification illisible : {0}")]
    Source(String),
    #[error(transparent)]
    Spec(#[from] OpenApiError),
    #[error("dossier parent introuvable : {0}")]
    Location(String),
    #[error("regroupement inconnu : {0} (tags ou path attendu)")]
    GroupBy(String),
    #[error("commande cURL invalide")]
    InvalidCurl,
    #[error("commande cURL trop volumineuse : {} Mo au plus", MAX_COMMAND_BYTES >> 20)]
    CurlTooLarge,
    #[error("nom invalide : {0}")]
    InvalidName(String),
    #[error("dossier introuvable dans la collection : {0}")]
    FolderNotFound(String),
    #[error("{0} existe déjà")]
    AlreadyExists(String),
    #[error(transparent)]
    Postman(#[from] PostmanError),
    #[error(transparent)]
    Insomnia(#[from] InsomniaError),
    #[error(transparent)]
    Manage(#[from] crate::manage::ManageError),
    #[error(transparent)]
    Core(#[from] CoreError),
}

pub(crate) fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

use text as text_of;

pub(crate) fn join(relative: &str, name: &str) -> String {
    if relative.is_empty() {
        name.to_owned()
    } else {
        format!("{relative}/{name}")
    }
}

impl ImportError {
    /// L'entrée (source de la spec, dossier parent, spec illisible) est en cause, pas l'import lui-même.
    pub fn is_input(&self) -> bool {
        matches!(
            self,
            Self::Source(_)
                | Self::Location(_)
                | Self::GroupBy(_)
                | Self::Spec(OpenApiError::Syntax(_) | OpenApiError::Empty)
                | Self::Postman(_)
                | Self::Insomnia(_)
        )
    }
}

impl GroupBy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tags => "tags",
            Self::Path => "path",
        }
    }
}

impl FromStr for GroupBy {
    type Err = ImportError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tags" => Ok(Self::Tags),
            "path" => Ok(Self::Path),
            other => Err(ImportError::GroupBy(other.to_owned())),
        }
    }
}

/// Convertit la spec puis écrit la collection dans un nouveau dossier de `location` ; renvoie sa racine.
pub fn import_spec(text: &str, source: &str, location: &Path, group_by: GroupBy) -> Result<PathBuf, ImportError> {
    let collection = to_bruno(&load_spec(text)?, group_by)?;
    write_collection(&collection, location, text, source, group_by)
}

/// Importe une collection Postman (v2.0 ou v2.1) dans un nouveau dossier de `location` ; renvoie sa racine et ce qui
/// n'a pas pu être converti.
pub fn import_postman(text: &str, location: &Path) -> Result<(PathBuf, Vec<Issue>), ImportError> {
    let converted = postman::collection_from_text(text)?;
    let root = write_plain_collection(&converted.collection, location)?;
    Ok((root, converted.issues))
}

/// Importe un export Insomnia (v4 JSON ou v5 YAML), environnements compris, dans un nouveau dossier de `location` ;
/// renvoie sa racine et ce qui n'a pas pu être converti.
pub fn import_insomnia(text: &str, location: &Path) -> Result<(PathBuf, Vec<Issue>), ImportError> {
    let converted = insomnia::collection_from_text(text)?;
    let root = write_plain_collection(&converted.collection, location)?;
    Ok((root, converted.issues))
}

/// Importe un environnement Postman dans la collection `root` ; renvoie le nom de l'environnement créé.
pub fn import_postman_environment(text: &str, root: &Path) -> Result<String, ImportError> {
    let env = postman::environment_from_text(text)?;
    let name = text_of(&env, "name");
    let name = if name.is_empty() { "Untitled Environment" } else { name };
    Ok(crate::manage::import_environment(root, name, env.get("variables").unwrap_or(&Value::Null))?)
}

/// Aperçu d'une spec avant import.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiPreview {
    pub summary: SpecSummary,
    /// Nom du dossier que créerait l'import, avant résolution des doublons dans le dossier parent.
    pub folder_name: String,
}

/// Résume la spec et la convertit pour les deux regroupements, afin de renvoyer dès l'aperçu les erreurs que
/// l'import rencontrerait ; `folder_name` est le dossier qu'il crée dans un dossier parent où rien ne le gêne.
pub fn preview(spec_text: &str) -> Result<OpenApiPreview, ImportError> {
    let spec = load_spec(spec_text)?;
    let collection = to_bruno(&spec, GroupBy::Tags)?;
    to_bruno(&spec, GroupBy::Path)?;
    let (folder_name, _) = naming::collection_folder(text(&collection, "name"), |_| false);
    Ok(OpenApiPreview { summary: summary(&spec), folder_name })
}
