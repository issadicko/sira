//! Exports d'une collection : Postman v2.1 et OpenAPI.

mod openapi_doc;
pub mod postman;
mod script;

pub use openapi_doc::openapi;

/// Le fichier exporté, et ce qui n'a pas pu l'être.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exported {
    pub text: String,
    /// Les éléments écartés ou simplifiés, un message chacun.
    pub issues: Vec<String>,
}
