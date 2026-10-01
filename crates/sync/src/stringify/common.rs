//! Blocs communs aux requêtes, dossiers et collections (`formats/yml/common/*` de Bruno).

use serde_json::Value as Json;
use xc_core::yaml::{Map, Value};

use super::js::{
    ensure_string, get, has_length, is_false, json_pretty, non_empty, non_empty_array, nullish, or, string, trim,
    truthy, yaml,
};

pub fn put(m: &mut Map, key: &str, value: Value) {
    m.set(key, value, &[]);
}

pub fn put_some(m: &mut Map, key: &str, value: Option<Value>) {
    if let Some(v) = value {
        put(m, key, v);
    }
}

pub fn map<const N: usize>(entries: [(&str, Value); N]) -> Map {
    Map(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn describe(m: &mut Map, entry: &Json) {
    if let Some(d) = non_empty(entry.get("description")) {
        put(m, "description", Value::str(d));
    }
}

fn disable(m: &mut Map, entry: &Json) {
    if is_false(entry.get("enabled")) {
        put(m, "disabled", Value::Bool(true));
    }
}

fn name_value(entry: &Json) -> Map {
    map([("name", or(entry.get("name"), Value::str(""))), ("value", or(entry.get("value"), Value::str("")))])
}

fn seq_of(list: Option<&Json>, convert: impl Fn(&Json) -> Map) -> Option<Value> {
    non_empty_array(list).map(|l| Value::Seq(l.iter().map(|e| Value::Map(convert(e))).collect()))
}

pub fn headers(list: Option<&Json>) -> Option<Value> {
    seq_of(list, |h| {
        let mut m = name_value(h);
        describe(&mut m, h);
        disable(&mut m, h);
        m
    })
}

pub fn response_headers(list: Option<&Json>) -> Option<Value> {
    seq_of(list, name_value)
}

pub fn params(list: Option<&Json>) -> Option<Value> {
    seq_of(list, |p| {
        let mut m = name_value(p);
        put_some(&mut m, "type", p.get("type").map(yaml));
        describe(&mut m, p);
        disable(&mut m, p);
        m
    })
}

pub fn body(body: Option<&Json>) -> Option<Value> {
    if !truthy(body) {
        return None;
    }
    let raw =
        |ty: &str, key: &str| Value::Map(map([("type", Value::str(ty)), ("data", or(get(body, key), Value::str("")))]));
    let entries = |ty: &str, key: &str, convert: &dyn Fn(&Json) -> Map| {
        let mut m = map([("type", Value::str(ty))]);
        put_some(&mut m, "data", seq_of(get(body, key), convert));
        Value::Map(m)
    };
    Some(match get(body, "mode").and_then(Json::as_str) {
        Some("json") => raw("json", "json"),
        Some("text") => raw("text", "text"),
        Some("xml") => raw("xml", "xml"),
        Some("sparql") => raw("sparql", "sparql"),
        Some("formUrlEncoded") => entries("form-urlencoded", "formUrlEncoded", &|e| {
            let mut m = name_value(e);
            describe(&mut m, e);
            disable(&mut m, e);
            m
        }),
        Some("multipartForm") => entries("multipart-form", "multipartForm", &multipart_entry),
        Some("file") => entries("file", "file", &|f| {
            let mut m = map([
                ("filePath", or(f.get("filePath"), Value::str(""))),
                ("contentType", or(f.get("contentType"), Value::str(""))),
                ("selected", nullish(f.get("selected"), Value::Bool(false))),
            ]);
            describe(&mut m, f);
            m
        }),
        _ => return None,
    })
}

fn multipart_entry(e: &Json) -> Map {
    let is_file = e.get("type").and_then(Json::as_str) == Some("file");
    let mut m = map([("name", or(e.get("name"), Value::str("")))]);
    put_some(&mut m, "type", e.get("type").map(yaml));
    put(&mut m, "value", or(e.get("value"), if is_file { Value::Seq(vec![]) } else { Value::str("") }));
    if let Some(ct) = non_empty(e.get("contentType")) {
        put(&mut m, "contentType", Value::str(ct));
    }
    describe(&mut m, e);
    disable(&mut m, e);
    m
}

/// `serializeVariableValue`.
pub fn variable_value(v: Option<&Json>) -> String {
    match v {
        Some(o @ (Json::Object(_) | Json::Array(_))) => json_pretty(o),
        Some(Json::Null) | None => String::new(),
        Some(other) => string(other),
    }
}

/// Valeur typée `{ type, data }` quand `dataType` n'est pas `string`, sinon la chaîne.
pub fn typed_value(v: &Json, data: String) -> Value {
    match v.get("dataType") {
        Some(t) if truthy(Some(t)) && t.as_str() != Some("string") => {
            Value::Map(map([("type", yaml(t)), ("data", Value::Str(data))]))
        }
        _ => Value::Str(data),
    }
}

pub fn variables(vars: Option<&Json>) -> Option<Value> {
    let list = match vars {
        Some(Json::Object(o)) if o.contains_key("req") => o.get("req"),
        other => other,
    };
    seq_of(list, |v| {
        let mut m = map([
            ("name", or(v.get("name"), Value::str(""))),
            ("value", typed_value(v, variable_value(v.get("value")))),
        ]);
        describe(&mut m, v);
        disable(&mut m, v);
        m
    })
}

pub fn actions(res: Option<&Json>) -> Option<Value> {
    seq_of(res, |v| {
        let scope = if truthy(v.get("local")) { "request" } else { "runtime" };
        let mut m = map([
            ("type", Value::str("set-variable")),
            ("phase", Value::str("after-response")),
            (
                "selector",
                Value::Map(map([
                    ("expression", Value::Str(ensure_string(v.get("value")))),
                    ("method", Value::str("jsonq")),
                ])),
            ),
            ("variable", Value::Map(map([("name", or(v.get("name"), Value::str(""))), ("scope", Value::str(scope))]))),
        ]);
        describe(&mut m, v);
        disable(&mut m, v);
        m
    })
}

pub fn has_scripts(request: Option<&Json>) -> bool {
    let script = get(request, "script");
    truthy(get(script, "req")) || truthy(get(script, "res")) || truthy(get(request, "tests"))
}

pub fn scripts(request: Option<&Json>) -> Option<Value> {
    let script = get(request, "script");
    let sources = [
        ("before-request", get(script, "req")),
        ("after-response", get(script, "res")),
        ("tests", get(request, "tests")),
    ];
    let list: Vec<Value> = sources
        .into_iter()
        .filter_map(|(ty, code)| non_empty(code).map(|c| (ty, c)))
        .map(|(ty, code)| Value::Map(map([("type", Value::str(ty)), ("code", Value::str(trim(code)))])))
        .collect();
    (!list.is_empty()).then_some(Value::Seq(list))
}

const UNARY_OPERATORS: [&str; 12] = [
    "isEmpty",
    "isNotEmpty",
    "isNull",
    "isUndefined",
    "isDefined",
    "isTruthy",
    "isFalsy",
    "isJson",
    "isNumber",
    "isString",
    "isBoolean",
    "isArray",
];

const BINARY_OPERATORS: [&str; 16] = [
    "eq",
    "neq",
    "gt",
    "gte",
    "lt",
    "lte",
    "in",
    "notIn",
    "contains",
    "notContains",
    "length",
    "matches",
    "notMatches",
    "startsWith",
    "endsWith",
    "between",
];

fn assertion_operator(value: Value) -> (String, Option<Value>) {
    let Value::Str(s) = &value else { return ("eq".into(), Some(value)) };
    if s.is_empty() {
        return ("eq".into(), Some(value));
    }
    let (first, rest) = trim(s).split_once(' ').unwrap_or((trim(s), ""));
    if UNARY_OPERATORS.contains(&first) {
        (first.into(), None)
    } else if BINARY_OPERATORS.contains(&first) {
        (first.into(), Some(Value::str(rest)))
    } else {
        ("eq".into(), Some(value))
    }
}

pub fn assertions(list: Option<&Json>) -> Option<Value> {
    seq_of(list, |a| {
        let (operator, value) = assertion_operator(or(a.get("value"), Value::str("")));
        let mut m = map([("expression", or(a.get("name"), Value::str(""))), ("operator", Value::Str(operator))]);
        put_some(&mut m, "value", value);
        describe(&mut m, a);
        disable(&mut m, a);
        m
    })
}

/// En-têtes, auth, variables, actions et scripts par défaut d'un dossier ou d'une collection.
pub fn request_defaults(request: Option<&Json>, with_auth: bool) -> Option<Value> {
    let vars = get(request, "vars");
    let has_defaults = has_length(get(request, "headers"))
        || has_length(get(vars, "req"))
        || has_length(get(vars, "res"))
        || has_scripts(request)
        || with_auth;
    if !has_defaults {
        return None;
    }
    let mut m = Map::default();
    if has_length(get(request, "headers")) {
        put_some(&mut m, "headers", headers(get(request, "headers")));
    }
    if with_auth {
        put_some(&mut m, "auth", super::auth::auth(get(request, "auth")));
    }
    if has_length(get(vars, "req")) {
        put_some(&mut m, "variables", variables(vars));
    }
    if has_length(get(vars, "res")) {
        put_some(&mut m, "actions", actions(get(vars, "res")));
    }
    if has_scripts(request) {
        put_some(&mut m, "scripts", scripts(request));
    }
    Some(Value::Map(m))
}

/// `{ content, type: text/markdown }` quand la documentation n'est pas vide.
pub fn markdown_docs(docs: Option<&Json>) -> Option<Value> {
    non_empty(docs).map(|d| Value::Map(map([("content", Value::str(d)), ("type", Value::str("text/markdown"))])))
}
