use serde::{Deserialize, Serialize};

use crate::yaml::{Map, Value};

pub const TOP_ORDER: &[&str] =
    &["info", "http", "graphql", "grpc", "websocket", "runtime", "settings", "examples", "docs", "app"];
pub const BLANK_BEFORE: &[&str] = &[
    "info",
    "http",
    "graphql",
    "grpc",
    "websocket",
    "mock",
    "routes",
    "runtime",
    "settings",
    "examples",
    "docs",
    "items",
    "request",
];
const INFO_ORDER: &[&str] = &["name", "type", "seq", "tags", "description"];
pub const HTTP_ORDER: &[&str] = &["method", "url", "headers", "params", "body", "auth"];
const RUNTIME_ORDER: &[&str] = &["variables", "scripts", "assertions", "actions"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValue {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    Query,
    Path,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub name: String,
    pub value: String,
    pub kind: ParamKind,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Valeur d'un champ multipart : texte, ou liste de chemins de fichiers comme l'écrit Bruno.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum MultipartValue {
    Text(String),
    File(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultipartField {
    pub name: String,
    #[serde(flatten)]
    pub value: MultipartValue,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Body {
    None,
    Json {
        data: String,
    },
    Text {
        data: String,
    },
    Xml {
        data: String,
    },
    #[serde(rename = "form-urlencoded")]
    FormUrlEncoded {
        fields: Vec<KeyValue>,
    },
    MultipartForm {
        fields: Vec<MultipartField>,
    },
    Other {
        label: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Auth {
    Inherit,
    None,
    Bearer { token: String },
    Basic { username: String, password: String },
    Apikey { key: String, value: String, placement: String },
    Other { label: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assertion {
    pub expression: String,
    pub operator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub kind: String,
    pub code: String,
}

/// Vue typée d'un fichier de requête. Tout ce qui n'y figure pas reste intact dans l'arbre YAML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDoc {
    pub name: String,
    pub request_type: String,
    pub seq: Option<i64>,
    pub method: String,
    pub url: String,
    pub params: Vec<Param>,
    pub headers: Vec<KeyValue>,
    pub body: Body,
    pub auth: Auth,
    pub assertions: Vec<Assertion>,
    pub variables: Vec<KeyValue>,
    pub scripts: Vec<Script>,
    pub docs: Option<String>,
    pub timeout_ms: Option<u64>,
}

pub(crate) fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::Map(m)) => m.get("data").and_then(Value::scalar).unwrap_or_default(),
        Some(other) => other.scalar().unwrap_or_default(),
        None => String::new(),
    }
}

fn opt_text(m: &Map, key: &str) -> Option<String> {
    m.get(key).and_then(Value::scalar)
}

pub(crate) fn key_values(items: &[Value]) -> Vec<KeyValue> {
    items
        .iter()
        .filter_map(Value::as_map)
        .map(|m| KeyValue {
            name: text(m.get("name")),
            value: text(m.get("value")),
            enabled: !m.get("disabled").is_some_and(Value::is_true),
            description: opt_text(m, "description"),
        })
        .collect()
}

fn multipart_fields(items: &[Value]) -> Vec<MultipartField> {
    items
        .iter()
        .filter_map(Value::as_map)
        .map(|m| MultipartField {
            name: text(m.get("name")),
            value: match m.str("type") {
                Some("file") => MultipartValue::File(match m.get("value") {
                    Some(Value::Seq(paths)) => paths.iter().filter_map(Value::scalar).collect(),
                    other => other.and_then(Value::scalar).into_iter().collect(),
                }),
                _ => MultipartValue::Text(text(m.get("value"))),
            },
            enabled: !m.get("disabled").is_some_and(Value::is_true),
            content_type: opt_text(m, "contentType"),
            description: opt_text(m, "description"),
        })
        .collect()
}

pub(crate) fn auth_from(value: Option<&Value>) -> Auth {
    let Some(value) = value else { return Auth::None };
    if value.as_str() == Some("inherit") {
        return Auth::Inherit;
    }
    let Some(m) = value.as_map() else { return Auth::Other { label: value.scalar().unwrap_or_default() } };
    let field = |k: &str| text(m.get(k));
    match m.str("type").unwrap_or_default() {
        "bearer" => Auth::Bearer { token: field("token") },
        "basic" => Auth::Basic { username: field("username"), password: field("password") },
        "apikey" => Auth::Apikey {
            key: field("key"),
            value: field("value"),
            placement: m.str("placement").unwrap_or("header").into(),
        },
        "inherit" => Auth::Inherit,
        other => Auth::Other { label: other.into() },
    }
}

impl RequestDoc {
    pub fn from_tree(root: &Map) -> Self {
        let empty = Map::default();
        let info = root.map("info").unwrap_or(&empty);
        let request_type = info.str("type").unwrap_or("http").to_owned();
        let http = root.map("http").or_else(|| root.map(&request_type)).unwrap_or(&empty);
        let runtime = root.map("runtime").unwrap_or(&empty);
        let settings = root.map("settings").unwrap_or(&empty);

        let params = http
            .seq("params")
            .iter()
            .filter_map(Value::as_map)
            .map(|m| Param {
                name: text(m.get("name")),
                value: text(m.get("value")),
                kind: if m.str("type") == Some("path") { ParamKind::Path } else { ParamKind::Query },
                enabled: !m.get("disabled").is_some_and(Value::is_true),
                description: opt_text(m, "description"),
            })
            .collect();

        let body = match http.map("body") {
            None => Body::None,
            Some(b) => match b.str("type").unwrap_or_default() {
                "json" => Body::Json { data: text(b.get("data")) },
                "text" => Body::Text { data: text(b.get("data")) },
                "xml" => Body::Xml { data: text(b.get("data")) },
                "form-urlencoded" => Body::FormUrlEncoded { fields: key_values(b.seq("data")) },
                "multipart-form" => Body::MultipartForm { fields: multipart_fields(b.seq("data")) },
                other => Body::Other { label: other.into() },
            },
        };

        let assertions = runtime
            .seq("assertions")
            .iter()
            .filter_map(Value::as_map)
            .map(|m| Assertion {
                expression: text(m.get("expression")),
                operator: text(m.get("operator")),
                value: opt_text(m, "value"),
                enabled: !m.get("disabled").is_some_and(Value::is_true),
                description: opt_text(m, "description"),
            })
            .collect();

        let scripts = runtime
            .seq("scripts")
            .iter()
            .filter_map(Value::as_map)
            .map(|m| Script { kind: text(m.get("type")), code: text(m.get("code")) })
            .collect();

        Self {
            name: info.str("name").unwrap_or_default().to_owned(),
            request_type,
            seq: info.get("seq").and_then(Value::as_i64),
            method: http.str("method").unwrap_or("GET").to_uppercase(),
            url: text(http.get("url")),
            params,
            headers: key_values(http.seq("headers")),
            body,
            auth: auth_from(http.get("auth")),
            assertions,
            variables: key_values(runtime.seq("variables")),
            scripts,
            docs: root.get("docs").and_then(Value::scalar),
            timeout_ms: settings.get("timeout").and_then(Value::as_i64).filter(|t| *t > 0).map(|t| t as u64),
        }
    }

    /// Écrit dans l'arbre uniquement les sections qui diffèrent de `previous`,
    /// pour qu'un fichier écrit à la main ne soit pas renormalisé sans raison.
    pub fn apply(&self, root: &mut Map, previous: &RequestDoc) {
        if self.name != previous.name {
            root.map_mut_or_insert("info", TOP_ORDER).set("name", Value::str(&self.name), INFO_ORDER);
        }
        let changed_http = self.method != previous.method
            || self.url != previous.url
            || self.headers != previous.headers
            || self.params != previous.params
            || self.body != previous.body
            || self.auth != previous.auth;
        if changed_http {
            let http = root.map_mut_or_insert("http", TOP_ORDER);
            if self.method != previous.method {
                http.set("method", Value::str(&self.method), HTTP_ORDER);
            }
            if self.url != previous.url {
                http.set("url", Value::str(&self.url), HTTP_ORDER);
            }
            if self.headers != previous.headers {
                set_list(http, "headers", self.headers.iter().map(key_value_entry).collect(), HTTP_ORDER);
            }
            if self.params != previous.params {
                set_list(http, "params", self.params.iter().map(param_value).collect(), HTTP_ORDER);
            }
            if self.body != previous.body {
                write_body(http, &self.body);
            }
            if self.auth != previous.auth {
                write_auth(http, &self.auth);
            }
        }
        if self.assertions != previous.assertions {
            let runtime = root.map_mut_or_insert("runtime", TOP_ORDER);
            set_list(runtime, "assertions", self.assertions.iter().map(assertion_value).collect(), RUNTIME_ORDER);
            if runtime.is_empty() {
                root.remove("runtime");
            }
        }
        if self.docs != previous.docs {
            match self.docs.as_deref().filter(|d| !d.is_empty()) {
                Some(d) => root.set("docs", Value::str(d), TOP_ORDER),
                None => {
                    root.remove("docs");
                }
            }
        }
    }
}

fn set_list(map: &mut Map, key: &str, items: Vec<Value>, order: &[&str]) {
    if items.is_empty() {
        map.remove(key);
    } else {
        map.set(key, Value::Seq(items), order);
    }
}

fn entry(pairs: Vec<(&str, Value)>) -> Value {
    Value::Map(Map(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()))
}

fn non_blank(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|v| !v.trim().is_empty())
}

fn key_value_entry(h: &KeyValue) -> Value {
    let mut pairs = vec![("name", Value::str(&h.name)), ("value", Value::str(&h.value))];
    if let Some(d) = non_blank(&h.description) {
        pairs.push(("description", Value::str(d)));
    }
    if !h.enabled {
        pairs.push(("disabled", Value::Bool(true)));
    }
    entry(pairs)
}

fn param_value(p: &Param) -> Value {
    let kind = match p.kind {
        ParamKind::Query => "query",
        ParamKind::Path => "path",
    };
    let mut pairs = vec![("name", Value::str(&p.name)), ("value", Value::str(&p.value)), ("type", Value::str(kind))];
    if let Some(d) = &p.description {
        pairs.push(("description", Value::str(d)));
    }
    if !p.enabled {
        pairs.push(("disabled", Value::Bool(true)));
    }
    entry(pairs)
}

fn assertion_value(a: &Assertion) -> Value {
    let mut pairs = vec![("expression", Value::str(&a.expression)), ("operator", Value::str(&a.operator))];
    if let Some(v) = &a.value {
        pairs.push(("value", Value::str(v)));
    }
    if let Some(d) = &a.description {
        pairs.push(("description", Value::str(d)));
    }
    if !a.enabled {
        pairs.push(("disabled", Value::Bool(true)));
    }
    entry(pairs)
}

fn multipart_entry(f: &MultipartField) -> Value {
    let (kind, value) = match &f.value {
        MultipartValue::Text(text) => ("text", Value::str(text)),
        MultipartValue::File(paths) => ("file", Value::Seq(paths.iter().map(Value::str).collect())),
    };
    let mut pairs = vec![("name", Value::str(&f.name)), ("type", Value::str(kind)), ("value", value)];
    for (key, field) in [("contentType", &f.content_type), ("description", &f.description)] {
        if let Some(v) = non_blank(field) {
            pairs.push((key, Value::str(v)));
        }
    }
    if !f.enabled {
        pairs.push(("disabled", Value::Bool(true)));
    }
    entry(pairs)
}

fn write_body(http: &mut Map, body: &Body) {
    let typed = |t: &str, data: Value| entry(vec![("type", Value::str(t)), ("data", data)]);
    let listed = |t: &str, items: Vec<Value>| {
        if items.is_empty() {
            entry(vec![("type", Value::str(t))])
        } else {
            typed(t, Value::Seq(items))
        }
    };
    let value = match body {
        Body::None => {
            http.remove("body");
            return;
        }
        Body::Other { .. } => return,
        Body::Json { data } => typed("json", Value::str(data)),
        Body::Text { data } => typed("text", Value::str(data)),
        Body::Xml { data } => typed("xml", Value::str(data)),
        Body::FormUrlEncoded { fields } => listed("form-urlencoded", fields.iter().map(key_value_entry).collect()),
        Body::MultipartForm { fields } => listed("multipart-form", fields.iter().map(multipart_entry).collect()),
    };
    http.set("body", value, HTTP_ORDER);
}

fn write_auth(http: &mut Map, auth: &Auth) {
    let value = match auth {
        Auth::None => {
            http.remove("auth");
            return;
        }
        Auth::Other { .. } => return,
        Auth::Inherit => Value::str("inherit"),
        Auth::Bearer { token } => entry(vec![("type", Value::str("bearer")), ("token", Value::str(token))]),
        Auth::Basic { username, password } => entry(vec![
            ("type", Value::str("basic")),
            ("username", Value::str(username)),
            ("password", Value::str(password)),
        ]),
        Auth::Apikey { key, value, placement } => entry(vec![
            ("type", Value::str("apikey")),
            ("key", Value::str(key)),
            ("value", Value::str(value)),
            ("placement", Value::str(placement)),
        ]),
    };
    http.set("auth", value, HTTP_ORDER);
}
