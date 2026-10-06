//! Imports (cURL, OpenAPI), synchronisation OpenAPI à 3 voies et gestion de collection (création, renommage,
//! déplacement, suppression).
//!
//! Les convertisseurs produisent le JSON de collection de Bruno (`serde_json::Value`), le même que
//! celui de `@usebruno/converters`, puis `stringify` l'écrit en OpenCollection YAML comme Bruno.

pub mod bru;
pub mod curl;
pub mod import;
pub mod insomnia;
mod js;
pub mod manage;
pub mod merge;
pub mod openapi;
pub mod postman;
pub mod store;
pub mod stringify;
pub mod sync;
