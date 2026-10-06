//! Lecture des collections au format `.bru` et conversion en OpenCollection YAML.
//!
//! Port de bruno-lang v2 (`bruToJson.js`, `collectionBruToJson.js`, `envToJson.js`) et de `parseBruRequest`,
//! `parseBruCollection` et `parseBruEnvironment` de bruno-filestore : un fichier `.bru` devient l'élément de collection
//! JSON de Bruno, que le sérialiseur YAML des autres imports écrit en OpenCollection.

mod convert;
mod environment;
mod example;
mod scanner;
mod semantics;

pub use convert::{collection_from_dir, is_bru_collection, BruError, Converted, CONFIG_FILE};
