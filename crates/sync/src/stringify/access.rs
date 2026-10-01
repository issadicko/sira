//! Lecture du JSON de Bruno avec la sémantique JavaScript du sérialiseur : `?.`, `||`, `??`, `x?.length`,
//! `isNonEmptyString` et conversion en valeur YAML.

use serde_json::Value as Json;
use xc_core::yaml::{Map, Value};

use crate::js::{entries, number_to_string, string, trim, truthy};

pub fn get<'a>(v: Option<&'a Json>, key: &str) -> Option<&'a Json> {
    v.and_then(|v| v.get(key))
}

/// `x?.length` est vrai.
pub fn has_length(v: Option<&Json>) -> bool {
    match v {
        Some(Json::Array(a)) => !a.is_empty(),
        Some(Json::String(s)) => !s.is_empty(),
        _ => false,
    }
}

pub fn non_empty_array(v: Option<&Json>) -> Option<&Vec<Json>> {
    v.and_then(Json::as_array).filter(|a| !a.is_empty())
}

/// `isNonEmptyString(x)` : la chaîne d'origine si elle contient autre chose que des blancs.
pub fn non_empty(v: Option<&Json>) -> Option<&str> {
    v.and_then(Json::as_str).filter(|s| !trim(s).is_empty())
}

pub fn is_false(v: Option<&Json>) -> bool {
    v == Some(&Json::Bool(false))
}

pub fn is_true(v: Option<&Json>) -> bool {
    v == Some(&Json::Bool(true))
}

/// `x || fallback`.
pub fn or(v: Option<&Json>, fallback: Value) -> Value {
    match v {
        Some(v) if truthy(v) => yaml(v),
        _ => fallback,
    }
}

/// `x ?? fallback`.
pub fn nullish(v: Option<&Json>, fallback: Value) -> Value {
    match v {
        None | Some(Json::Null) => fallback,
        Some(v) => yaml(v),
    }
}

pub fn yaml(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::Float(number_to_string(n.as_f64().unwrap_or(0.0))),
        Json::String(s) => Value::str(s),
        Json::Array(a) => Value::Seq(a.iter().map(yaml).collect()),
        Json::Object(o) => Value::Map(Map(entries(o).into_iter().map(|(k, v)| (k.clone(), yaml(v))).collect())),
    }
}

/// `ensureString(x)` : chaîne vide pour `null` et `undefined`.
pub fn ensure_string(v: Option<&Json>) -> String {
    match v {
        None | Some(Json::Null) => String::new(),
        Some(v) => string(v),
    }
}
