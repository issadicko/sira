//! Port du sérialiseur OpenCollection YAML de Bruno (`bruno-filestore/src/formats/yml`).
//!
//! Chaque fonction prend le JSON de Bruno, tel que le produisent ses convertisseurs, et rend le
//! fichier YAML identique à l'octet près à celui qu'écrit Bruno.

mod access;
mod auth;
mod collection;
mod common;
mod environment;
mod request;

use serde_json::Value as Json;
use xc_core::request::BLANK_BEFORE;
use xc_core::yaml::{emit, Map, Value};

fn yml(root: Map) -> String {
    emit(&Value::Map(root), BLANK_BEFORE)
}

/// Fichier d'une requête. Les `graphql-request` sont écrites en GraphQL, tout autre type en HTTP :
/// les imports ne produisent que ces deux types (gRPC, WebSocket, scripts et apps ne sont pas portés).
pub fn item(item: &Json) -> String {
    match item.get("type").and_then(Json::as_str) {
        Some("graphql-request") => yml(request::graphql(item)),
        _ => yml(request::http(item)),
    }
}

/// `folder.yml` à partir de la racine d'un dossier (`item.root`, avec `meta.seq` renseigné).
pub fn folder(root: &Json) -> String {
    yml(collection::folder(root))
}

/// `opencollection.yml` à partir de la racine de collection et de sa configuration (`brunoConfig`).
pub fn collection(root: &Json, bruno_config: &Json) -> String {
    yml(collection::collection(root, bruno_config))
}

/// Fichier d'environnement.
pub fn environment(env: &Json) -> String {
    yml(environment::environment(env))
}
