//! Imports (cURL, OpenAPI) et, plus tard, synchronisation OpenAPI à 3 voies.
//!
//! Les convertisseurs produisent le JSON de collection de Bruno (`serde_json::Value`), le même que
//! celui de `@usebruno/converters`, puis `stringify` l'écrit en OpenCollection YAML comme Bruno.

pub mod curl;
pub mod openapi;
pub mod stringify;
