//! Imports vers une collection : écriture d'une collection OpenAPI convertie (gestionnaire
//! `renderer:import-collection` de Bruno), snapshot `.oc-sync/openapi/` pour la future synchro,
//! récupération de la spec (fichier ou URL) et requêtes créées depuis une commande cURL.
//!
//! Écarts voulus avec Bruno, tous au profit du principe « non destructif » : deux éléments dont les noms
//! assainis se confondent (sans tenir compte de la casse) reçoivent un suffixe ` 1`, ` 2`… au lieu de
//! s'écraser, les noms réservés de la collection (`opencollection.yml`, `folder.yml`, `environments`,
//! `.oc-sync`) sont évités de même, et les noms trop longs sont tronqués aussi en octets. Un nom réduit à des
//! caractères interdits devient `Untitled Request`, `Untitled Folder` ou `Untitled Environment`.

mod from_curl;
mod naming;
mod source;
mod write;

use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Serialize;
use serde_json::Value;
use xc_core::CoreError;

use crate::openapi::{load_spec, summary, to_bruno, GroupBy, OpenApiError, SpecSummary};

pub use from_curl::{create_request_from_curl, request_doc_from_curl};
pub use source::fetch_spec;
pub use write::write_collection;

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
    #[error("nom invalide : {0}")]
    InvalidName(String),
    #[error("dossier introuvable dans la collection : {0}")]
    FolderNotFound(String),
    #[error("{0} existe déjà")]
    AlreadyExists(String),
    #[error("{path} : {message}")]
    Io { path: String, message: String },
    #[error(transparent)]
    Core(#[from] CoreError),
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_default()
}

fn join(relative: &str, name: &str) -> String {
    if relative.is_empty() {
        name.to_owned()
    } else {
        format!("{relative}/{name}")
    }
}

impl ImportError {
    fn io(path: &Path, e: std::io::Error) -> Self {
        Self::Io { path: path.display().to_string(), message: e.to_string() }
    }

    /// L'entrée (source de la spec, dossier parent, spec illisible) est en cause, pas l'import lui-même.
    pub fn is_input(&self) -> bool {
        matches!(
            self,
            Self::Source(_)
                | Self::Location(_)
                | Self::GroupBy(_)
                | Self::Spec(OpenApiError::Syntax(_) | OpenApiError::Empty)
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
    write_collection(&collection, location, source, group_by)
}

/// Aperçu d'une spec avant import.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiPreview {
    pub summary: SpecSummary,
    /// Nom du dossier que créerait l'import, avant résolution des doublons dans le dossier parent.
    pub folder_name: String,
}

pub fn preview(text: &str) -> Result<OpenApiPreview, ImportError> {
    let summary = summary(&load_spec(text)?);
    let folder_name = naming::folder_name(&summary.title);
    Ok(OpenApiPreview { summary, folder_name })
}
