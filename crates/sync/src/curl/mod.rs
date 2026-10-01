//! Import d'une commande cURL : port fidèle de `getRequestFromCurlCommand` de Bruno
//! (`bruno-app/src/utils/curl`), avec le comportement des bibliothèques qu'il utilise (`shell-quote`,
//! `query-string`, `cookie`, `url` historique de Node, `jsonc-parser`).
//!
//! Le JSON produit est celui de Bruno, clé pour clé : `{ url, method, body, headers?, auth? }`.

mod content_type;
mod domain;
mod json;
mod node_url;
mod parse;
mod prettify;
mod query;
mod shell;
mod to_json;

use serde_json::{json, Map, Value};

pub use crate::js::JsError;
pub use parse::{parse_curl_command, ParsedCurl};
pub use to_json::{curl_to_json, CurlJson};

use crate::js::{json_stringify, truthy};
use content_type::{is_json_like, is_plain_text, is_xml_like};
use json::JsonValue;
use shell::Token;

/// `getRequestFromCurlCommand(command)` : requête HTTP de Bruno, ou `None` si la commande n'est pas reconnue.
pub fn request_from_curl(command: &str) -> Option<Value> {
    request_from_curl_typed(command, "http-request")
}

/// `getRequestFromCurlCommand(command, requestType)` ; `graphql-request` produit un corps GraphQL.
pub fn request_from_curl_typed(command: &str, request_type: &str) -> Option<Value> {
    if command.is_empty() {
        return None;
    }
    let request = curl_to_json(command).ok()??;
    Some(bruno_request(request, request_type))
}

fn bruno_request(request: CurlJson, request_type: &str) -> Value {
    let content_type = request
        .headers
        .iter()
        .flat_map(|headers| headers.iter())
        .find(|(name, _)| name.to_lowercase() == "content-type")
        .and_then(|(_, value)| value.as_ref())
        .and_then(Token::as_word)
        .filter(|ct| !ct.is_empty());
    let mut body = Map::new();
    body.insert("mode".into(), json!("none"));
    for key in ["json", "text", "xml", "sparql", "multipartForm", "formUrlEncoded", "graphql", "file"] {
        body.insert(key.into(), Value::Null);
    }
    if let Some(parsed) = request.data.as_ref().filter(|d| truthy(*d)) {
        if let Some((mode, value)) = body_for(parsed, content_type, request_type, request.is_data_binary) {
            body.insert("mode".into(), json!(mode));
            body.insert(mode.into(), value);
        }
    }
    let mut out = Map::new();
    out.insert("url".into(), json!(request.url));
    out.insert("method".into(), json!(request.method));
    out.insert("body".into(), Value::Object(body));
    if let Some(headers) = &request.headers {
        let list = headers
            .iter()
            .map(|(name, value)| {
                let mut header = Map::new();
                header.insert("name".into(), json!(name));
                if let Some(value) = value {
                    header.insert("value".into(), value.to_json());
                }
                header.insert("enabled".into(), json!(true));
                Value::Object(header)
            })
            .collect();
        out.insert("headers".into(), Value::Array(list));
    }
    if let Some(auth) = &request.auth {
        out.insert("auth".into(), auth.to_json());
    }
    Value::Object(out)
}

fn body_for(
    parsed: &Value,
    content_type: Option<&str>,
    request_type: &str,
    is_data_binary: bool,
) -> Option<(&'static str, Value)> {
    let Some(content_type) = content_type else { return Some(("formUrlEncoded", form_data(parsed))) };
    let normalized = content_type.to_lowercase();
    let ct = Some(content_type);
    let mode = if request_type == "graphql-request" && (is_json_like(ct) || normalized.contains("application/graphql"))
    {
        return Some(("graphql", graphql(parsed)));
    } else if normalized.contains("application/x-ndjson") || normalized.contains("application/ndjson") {
        "text"
    } else if request_type == "http-request" && is_data_binary && parsed.is_array() {
        "file"
    } else if is_json_like(ct) {
        let json = match parsed {
            Value::String(text) => json!(prettify::prettify_json_string(text)),
            other => other.clone(),
        };
        return Some(("json", json));
    } else if is_xml_like(ct) || normalized.contains("xml") {
        "xml"
    } else if normalized.contains("application/x-www-form-urlencoded") {
        return Some(("formUrlEncoded", form_data(parsed)));
    } else if normalized.contains("multipart/form-data") {
        "multipartForm"
    } else if is_plain_text(ct) {
        "text"
    } else {
        return None;
    };
    Some((mode, parsed.clone()))
}

/// `parseFormData` : `forOwn` de lodash sur le corps (objet, tableau ou chaîne).
fn form_data(parsed: &Value) -> Value {
    let entries: Vec<(String, Value)> = match parsed {
        Value::Object(map) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Value::Array(items) => items.iter().enumerate().map(|(i, v)| (i.to_string(), v.clone())).collect(),
        Value::String(s) => {
            s.encode_utf16().enumerate().map(|(i, u)| (i.to_string(), json!(String::from_utf16_lossy(&[u])))).collect()
        }
        _ => Vec::new(),
    };
    entries.into_iter().map(|(name, value)| json!({ "name": name, "value": value, "enabled": true })).collect()
}

/// `parseGraphQL` : `{ query, variables }` lus dans le JSON du corps.
fn graphql(parsed: &Value) -> Value {
    let empty = json!({ "query": "", "variables": "" });
    let Some(document) = parsed.as_str().and_then(json::parse) else { return empty };
    if document == JsonValue::Null {
        return empty;
    }
    let mut map = Map::new();
    if let Some(query) = document.get("query") {
        map.insert("query".into(), query.to_value());
    }
    if let Some(variables) = document.get("variables") {
        map.insert("variables".into(), json!(json_stringify(&variables.to_value(), true)));
    }
    Value::Object(map)
}
