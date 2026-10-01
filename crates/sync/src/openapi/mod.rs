//! Import OpenAPI 3.0 / 3.1 et Swagger 2.0 : port fidèle de `openApiToBruno`
//! (`@usebruno/converters`), qui produit le JSON de collection de Bruno.
//!
//! Le port reproduit la sémantique JavaScript dont dépend le résultat : ordre des clés des
//! objets, rendu des nombres, `String(x)`, `JSON.stringify`, véracité et résolution des `$ref`
//! avec ses caches. Chaque requête porte en plus une clé d'opération (`operationKey`).

mod common;
mod js;
mod resolve;
mod v2;
mod v3;
mod validate;
mod yaml;

use serde::Serialize;
use serde_json::Value;

use js::{Heap, Js};

#[derive(Debug, thiserror::Error)]
pub enum OpenApiError {
    #[error("spécification illisible : {0}")]
    Syntax(String),
    #[error("la spécification est vide")]
    Empty,
    #[error("spécification impossible à convertir : {0}")]
    Invalid(String),
    #[error("la collection produite est invalide : {0}")]
    Schema(String),
}

type R<T> = Result<T, OpenApiError>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GroupBy {
    #[default]
    Tags,
    Path,
}

/// Lit une spec JSON ou YAML comme `js-yaml` 4 (`load`) le fait pour `openApiToBruno`.
pub fn load_spec(text: &str) -> Result<Value, OpenApiError> {
    yaml::load(text)
}

/// Convertit une spec en JSON de collection Bruno, identique à `openApiToBruno` aux identifiants
/// près (`uid`, `itemUid` absents) ; chaque requête porte en dernier sa clé `operationKey`.
pub fn to_bruno(spec: &Value, group_by: GroupBy) -> Result<Value, OpenApiError> {
    if spec.is_null() {
        return Err(js::type_error("la spécification est nulle"));
    }
    let heap = Heap::default();
    let collection = if is_swagger2(&heap, spec) {
        v2::convert(&heap, spec, group_by)?
    } else {
        v3::convert(&heap, spec, group_by)?
    };
    validate::collection(&collection)?;
    Ok(collection)
}

fn is_swagger2(heap: &Heap, spec: &Value) -> bool {
    let swagger = spec.get("swagger").map_or(Js::Undef, |v| heap.import(v));
    swagger.truthy() && heap.to_string(&swagger).starts_with('2')
}

/// Aperçu d'une spec avant import.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpecSummary {
    pub title: String,
    pub version: Option<String>,
    /// `openapi` ou `swagger`.
    pub format: String,
    pub format_version: Option<String>,
    pub operation_count: usize,
    pub tags: Vec<String>,
    pub servers: Vec<String>,
}

/// Résume une spec sans la convertir ; n'échoue jamais.
pub fn summary(spec: &Value) -> SpecSummary {
    let heap = Heap::default();
    let swagger = is_swagger2(&heap, spec);
    let text = |v: Option<&Value>| match v {
        Some(Value::String(s)) => Some(s.trim().to_owned()),
        Some(Value::Number(n)) => n.as_f64().map(js::number_to_string),
        _ => None,
    };
    let info = spec.get("info");
    let methods: &[&str] = if swagger { &v2::METHODS } else { &v3::METHODS };
    let mut tags: Vec<String> = Vec::new();
    let mut add_tag = |t: &str| {
        let t = t.trim();
        if !t.is_empty() && !tags.iter().any(|x| x == t) {
            tags.push(t.to_owned());
        }
    };
    for tag in spec.get("tags").and_then(Value::as_array).into_iter().flatten() {
        if let Some(name) = tag.get("name").and_then(Value::as_str) {
            add_tag(name);
        }
    }
    let mut operation_count = 0;
    for item in spec.get("paths").and_then(Value::as_object).into_iter().flat_map(|p| p.values()) {
        let item = path_item(spec, item);
        for (method, op) in item.as_object().into_iter().flatten() {
            if !methods.contains(&method.to_lowercase().as_str()) {
                continue;
            }
            let variants = op.get("x-bruno-variants").and_then(Value::as_array).filter(|_| !swagger);
            operation_count += 1 + variants.map_or(0, |v| v.iter().filter(|x| x.is_object() || x.is_array()).count());
            for tag in op.get("tags").and_then(Value::as_array).into_iter().flatten() {
                if let Some(name) = tag.as_str() {
                    add_tag(name);
                }
            }
        }
    }
    let servers = if swagger {
        let resolved = heap.import(spec);
        v2::server_urls(&heap, &resolved).unwrap_or_default()
    } else {
        let urls = spec.get("servers").and_then(Value::as_array).into_iter().flatten();
        urls.filter_map(|s| s.get("url").and_then(Value::as_str).map(str::to_owned)).collect()
    };
    let title = info.and_then(|i| i.get("title")).and_then(Value::as_str).map(str::trim).unwrap_or_default();
    SpecSummary {
        title: if title.is_empty() { "Untitled Collection".into() } else { title.to_owned() },
        version: text(info.and_then(|i| i.get("version"))),
        format: if swagger { "swagger" } else { "openapi" }.into(),
        format_version: text(spec.get(if swagger { "swagger" } else { "openapi" })),
        operation_count,
        tags,
        servers,
    }
}

fn path_item<'a>(spec: &'a Value, item: &'a Value) -> &'a Value {
    let target = item
        .get("$ref")
        .and_then(Value::as_str)
        .and_then(|r| r.strip_prefix("#/"))
        .and_then(|path| path.split('/').try_fold(spec, |cur, key| cur.get(key)));
    target.unwrap_or(item)
}
