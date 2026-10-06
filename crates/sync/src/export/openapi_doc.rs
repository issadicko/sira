//! Export d'une collection au format OpenAPI 3.0.3 (JSON). Postman et Bruno n'en ont pas d'équivalent : la forme est
//! la nôtre.
//!
//! - `info` : nom, documentation et version de la collection (`1.0.0` à défaut).
//! - `servers` : le préfixe de chaque adresse (`{{baseUrl}}` ou l'origine écrite en clair) devient un serveur. Les
//!   valeurs d'une variable dans l'environnement ouvert par défaut, les autres environnements et la collection sont
//!   autant de serveurs ; le préfixe le plus employé est global, les autres sont portés par leurs opérations.
//! - `paths` : `:id` et `{{id}}` deviennent `{id}`, le dossier parent est l'étiquette, le nom de la requête le
//!   résumé, sa documentation la description. Le statut attendu est celui de l'assertion `res.status eq N`, 200 sinon.
//! - `security` : l'authentification effective de chaque requête (héritage compris) ; elle n'est répétée sur une
//!   opération que si elle diffère de celle de la collection.
//!
//! Ne sont pas exportés : les requêtes qui ne sont ni HTTP ni GraphQL, les secrets (aucun n'est écrit), un corps sur
//! `GET`, `HEAD`, `OPTIONS` ou `TRACE`, les en-têtes `Accept`, `Content-Type` et `Authorization` (OpenAPI les ignore
//! comme paramètres), les paramètres désactivés et les doublons méthode + chemin.

use std::collections::HashSet;
use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map as JsonMap, Value};
use xc_core::oauth2::{OAuth2, AUTHORIZATION_CODE, CLIENT_CREDENTIALS, IMPLICIT, PASSWORD};
use xc_core::yaml::Map;
use xc_core::{
    open_collection, read_collection_file, read_environment, read_folder_file, read_request, Assertion, Auth, Body,
    CoreError, KeyValue, MultipartValue, Param, ParamKind, TreeItem,
};

use super::postman::{docs_of, sorted_for_export};
use super::Exported;

const VERSION: &str = "3.0.3";
const NO_BODY_METHODS: [&str; 4] = ["get", "head", "options", "trace"];
const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

/// Exporte la collection du dossier `root`.
pub fn openapi(root: &Path) -> Result<Exported, CoreError> {
    let info = open_collection(root)?;
    let file = read_collection_file(root)?;
    let variables = Variables::load(root, &info.environments, info.default_environment.as_deref(), &file)?;
    let top_auth = file.map("request").map(|r| xc_core::request::auth_from(r.get("auth"))).unwrap_or(Auth::None);

    let mut export = Export::new(root, &variables);
    let top_security = export.security(&top_auth, &info.name);
    export.top_security = top_security.clone();
    export.walk(&info.items, "", &Scope { tag: None, auth: top_auth })?;

    let mut out = JsonMap::new();
    out.insert("openapi".into(), json!(VERSION));
    let mut header = JsonMap::new();
    header.insert("title".into(), json!(info.name));
    if let Some(docs) = docs_of(&file) {
        header.insert("description".into(), json!(docs));
    }
    let version = file.map("info").and_then(|i| i.str("version")).filter(|v| !v.is_empty());
    header.insert("version".into(), json!(version.unwrap_or("1.0.0")));
    out.insert("info".into(), Value::Object(header));
    let (global, servers) = export.servers();
    if !global.is_empty() {
        out.insert("servers".into(), Value::Array(global));
    }
    if let Some(security) = top_security {
        out.insert("security".into(), security);
    }
    if !export.tags.is_empty() {
        out.insert("tags".into(), Value::Array(std::mem::take(&mut export.tags)));
    }
    out.insert("paths".into(), Value::Object(export.paths(servers)));
    if !export.schemes.is_empty() {
        out.insert("components".into(), json!({ "securitySchemes": Value::Object(export.schemes) }));
    }
    let text = serde_json::to_string_pretty(&Value::Object(out))
        .map_err(|e| CoreError::Io { path: root.display().to_string(), message: e.to_string() })?;
    Ok(Exported { text, issues: export.issues })
}

/// Les valeurs de variables, de la plus prioritaire à la moins prioritaire : l'environnement ouvert par défaut, les
/// autres environnements, la collection.
struct Variables {
    sources: Vec<(String, Vec<KeyValue>)>,
}

impl Variables {
    fn load(root: &Path, environments: &[String], default: Option<&str>, file: &Map) -> Result<Self, CoreError> {
        let mut names: Vec<&String> = environments.iter().collect();
        names.sort_by_key(|name| Some(name.as_str()) != default);
        let mut sources = Vec::new();
        for name in names {
            let values = read_environment(root, name)?
                .into_iter()
                .filter(|v| v.enabled && !v.secret)
                .filter_map(|v| Some(KeyValue { name: v.name, value: v.value?, enabled: true, description: None }))
                .collect();
            sources.push((name.clone(), values));
        }
        let own = file.map("request").map(|r| xc_core::request::key_values(r.seq("variables"))).unwrap_or_default();
        sources.push(("Collection".to_owned(), own.into_iter().filter(|v| v.enabled).collect()));
        Ok(Self { sources })
    }

    /// Les valeurs de `name`, une seule fois chacune, avec ce qui les définit.
    fn alternatives(&self, name: &str) -> Vec<(String, String)> {
        let mut found: Vec<(String, String)> = Vec::new();
        for (source, values) in &self.sources {
            if let Some(v) = values.iter().find(|v| v.name == name).filter(|v| !v.value.is_empty()) {
                if !found.iter().any(|(value, _)| *value == v.value) {
                    found.push((v.value.clone(), source.clone()));
                }
            }
        }
        found
    }

    fn value(&self, name: &str) -> Option<String> {
        self.alternatives(name).into_iter().next().map(|(value, _)| value)
    }

    /// `text` où chaque `{{nom}}` connu est remplacé par sa valeur.
    fn expand(&self, text: &str) -> String {
        placeholder()
            .replace_all(text, |c: &regex::Captures| self.value(&c[1]).unwrap_or_else(|| c[0].to_owned()))
            .into()
    }

    /// Un serveur : `{nom}` pour chaque variable de l'adresse, déclarée avec sa valeur connue pour défaut.
    fn server(&self, template: &str, description: Option<String>) -> Value {
        let mut names = Vec::new();
        let url = placeholder().replace_all(template.trim_end_matches('/'), |c: &regex::Captures| {
            names.push(c[1].to_owned());
            format!("{{{}}}", &c[1])
        });
        let url = if url.contains("://") || url.starts_with('{') { url.into_owned() } else { format!("http://{url}") };
        let mut server = JsonMap::new();
        server.insert("url".into(), json!(url));
        if let Some(description) = description {
            server.insert("description".into(), json!(description));
        }
        let mut declared = JsonMap::new();
        for name in names {
            declared.entry(name.clone()).or_insert_with(|| json!({ "default": self.value(&name).unwrap_or_default() }));
        }
        if !declared.is_empty() {
            server.insert("variables".into(), Value::Object(declared));
        }
        Value::Object(server)
    }
}

fn placeholder() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"\{\{\s*([^{}\s]+)\s*\}\}").expect("motif valide"))
}

/// Ce que les éléments d'un dossier héritent de lui.
struct Scope {
    tag: Option<String>,
    auth: Auth,
}

/// Le début d'une adresse : une variable (`{{baseUrl}}`) ou une origine écrite en clair.
#[derive(Debug, Clone, PartialEq)]
enum Origin {
    Variable(String),
    Literal(String),
}

impl Origin {
    fn key(&self) -> String {
        match self {
            Self::Variable(name) => format!("{{{{{name}}}}}"),
            Self::Literal(origin) => origin.clone(),
        }
    }
}

struct Operation {
    path: String,
    method: String,
    origin: Option<Origin>,
    value: JsonMap<String, Value>,
}

struct Export<'a> {
    root: &'a Path,
    variables: &'a Variables,
    top_security: Option<Value>,
    operations: Vec<Operation>,
    tags: Vec<Value>,
    schemes: JsonMap<String, Value>,
    identifiers: HashSet<String>,
    issues: Vec<String>,
}

impl<'a> Export<'a> {
    fn new(root: &'a Path, variables: &'a Variables) -> Self {
        Self {
            root,
            variables,
            top_security: None,
            operations: Vec::new(),
            tags: Vec::new(),
            schemes: JsonMap::new(),
            identifiers: HashSet::new(),
            issues: Vec::new(),
        }
    }

    fn walk(&mut self, items: &[TreeItem], parent: &str, scope: &Scope) -> Result<(), CoreError> {
        let root = self.root;
        for item in sorted_for_export(items) {
            let at = if parent.is_empty() { item.name().to_owned() } else { format!("{parent} / {}", item.name()) };
            match item {
                TreeItem::Folder { path, children, .. } => {
                    let file = read_folder_file(root, path)?;
                    let auth = match file.map("request").map(|r| xc_core::request::auth_from(r.get("auth"))) {
                        Some(Auth::Inherit) | None => scope.auth.clone(),
                        Some(own) => own,
                    };
                    let inner = Scope { tag: Some(at.clone()), auth };
                    let before = self.operations.len();
                    self.walk(children, &at, &inner)?;
                    if self.operations.len() > before {
                        let mut tag = JsonMap::new();
                        tag.insert("name".into(), json!(at));
                        if let Some(docs) = docs_of(&file) {
                            tag.insert("description".into(), json!(docs));
                        }
                        self.tags.push(Value::Object(tag));
                    }
                }
                TreeItem::Request { path, name, .. } => self.request(path, name, &at, scope)?,
            }
        }
        Ok(())
    }

    fn request(&mut self, path: &str, name: &str, at: &str, scope: &Scope) -> Result<(), CoreError> {
        let doc = read_request(self.root, path)?;
        if doc.request_type != "http" && doc.request_type != "graphql" {
            self.issues.push(format!("{at} : requête {} ignorée, OpenAPI ne la décrit pas", doc.request_type));
            return Ok(());
        }
        let method = if doc.method.is_empty() { "get".to_owned() } else { doc.method.to_lowercase() };
        if !METHODS.contains(&method.as_str()) {
            self.issues.push(format!("{at} : méthode {} ignorée, OpenAPI ne la décrit pas", doc.method));
            return Ok(());
        }
        let (origin, rest) = split_url(&doc.url);
        let (route, path_names) = route_of(&rest, &doc.params);
        if self.operations.iter().any(|o| o.path == route && o.method == method) {
            self.issues.push(format!("{at} : doublon {} {route} ignoré", method.to_uppercase()));
            return Ok(());
        }

        let mut value = JsonMap::new();
        if let Some(tag) = &scope.tag {
            value.insert("tags".into(), json!([tag]));
        }
        value.insert("summary".into(), json!(name));
        if let Some(docs) = doc.docs.as_deref().filter(|d| !d.is_empty()) {
            value.insert("description".into(), json!(docs));
        }
        let id = self.identifier(name);
        value.insert("operationId".into(), json!(id));
        let parameters = parameters(&doc.params, &doc.headers, &path_names);
        if !parameters.is_empty() {
            value.insert("parameters".into(), Value::Array(parameters));
        }
        if doc.body != Body::None {
            if NO_BODY_METHODS.contains(&method.as_str()) {
                self.issues
                    .push(format!("{at} : corps ignoré, OpenAPI n'en décrit pas pour {}", method.to_uppercase()));
            } else if let Some(body) = self.body(&doc.body, at) {
                value.insert("requestBody".into(), body);
            }
        }
        let status = expected_status(&doc.assertions);
        value.insert("responses".into(), json!({ status.to_string(): { "description": format!("Réponse {status}") } }));
        let auth = if doc.auth == Auth::Inherit { scope.auth.clone() } else { doc.auth.clone() };
        self.operations.push(Operation { path: route, method, origin, value });
        let security = self.security(&auth, at);
        if security != self.top_security {
            let own = security.unwrap_or_else(|| json!([]));
            self.operations.last_mut().expect("opération ajoutée").value.insert("security".into(), own);
        }
        Ok(())
    }

    fn identifier(&mut self, name: &str) -> String {
        let mut id = String::new();
        for (i, word) in name.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).enumerate() {
            let lower = word.to_lowercase();
            if i == 0 {
                id.push_str(&lower);
            } else {
                let mut chars = lower.chars();
                id.extend(chars.next().into_iter().flat_map(char::to_uppercase));
                id.push_str(chars.as_str());
            }
        }
        if id.is_empty() {
            id.push_str("request");
        }
        let mut candidate = id.clone();
        let mut n = 1;
        while !self.identifiers.insert(candidate.clone()) {
            n += 1;
            candidate = format!("{id}_{n}");
        }
        candidate
    }

    fn body(&mut self, body: &Body, at: &str) -> Option<Value> {
        let content = match body {
            Body::Json { data } => match serde_json::from_str::<Value>(data) {
                Ok(example) => json!({ "application/json": { "schema": infer(&example), "example": example } }),
                Err(_) => {
                    self.issues.push(format!("{at} : corps JSON illisible, gardé tel quel en exemple"));
                    json!({ "application/json": { "schema": {}, "example": data } })
                }
            },
            Body::Xml { data } => json!({ "application/xml": { "schema": { "type": "string" }, "example": data } }),
            Body::Text { data } => json!({ "text/plain": { "schema": { "type": "string" }, "example": data } }),
            Body::FormUrlEncoded { fields } => {
                let entries = fields
                    .iter()
                    .filter(|f| f.enabled)
                    .map(|f| (&f.name, field_schema(&f.value, f.description.as_deref())));
                json!({ "application/x-www-form-urlencoded": { "schema": form_object(entries) } })
            }
            Body::MultipartForm { fields } => {
                let enabled = fields.iter().filter(|f| f.enabled);
                let mut encoding = JsonMap::new();
                let entries = enabled.map(|f| {
                    if let Some(kind) = f.content_type.as_ref().filter(|c| !c.is_empty()) {
                        encoding.insert(f.name.clone(), json!({ "contentType": kind }));
                    }
                    let schema = match &f.value {
                        MultipartValue::File(paths) if paths.len() > 1 => {
                            json!({ "type": "array", "items": { "type": "string", "format": "binary" } })
                        }
                        MultipartValue::File(_) => json!({ "type": "string", "format": "binary" }),
                        MultipartValue::Text(text) => field_schema(text, f.description.as_deref()),
                    };
                    (&f.name, schema)
                });
                let mut media = JsonMap::new();
                media.insert("schema".into(), form_object(entries));
                if !encoding.is_empty() {
                    media.insert("encoding".into(), Value::Object(encoding));
                }
                json!({ "multipart/form-data": Value::Object(media) })
            }
            Body::Graphql { query, variables } => {
                let mut example = JsonMap::new();
                example.insert("query".into(), json!(query));
                if let Ok(parsed @ Value::Object(_)) = serde_json::from_str::<Value>(variables) {
                    example.insert("variables".into(), parsed);
                }
                json!({ "application/json": {
                    "schema": {
                        "type": "object",
                        "properties": { "query": { "type": "string" }, "variables": { "type": "object" } },
                        "required": ["query"],
                    },
                    "example": Value::Object(example),
                } })
            }
            Body::None => return None,
            Body::Other { label, .. } => {
                self.issues.push(format!("{at} : corps {label} ignoré, OpenAPI ne le décrit pas ici"));
                return None;
            }
        };
        Some(json!({ "required": true, "content": content }))
    }

    /// L'exigence de sécurité de `auth`, après avoir déclaré son schéma ; `None` sans authentification.
    fn security(&mut self, auth: &Auth, at: &str) -> Option<Value> {
        let vars = self.variables;
        let (base, scheme, scopes) = match auth {
            Auth::Inherit | Auth::None => return None,
            Auth::Bearer { .. } => ("bearerAuth".to_owned(), json!({ "type": "http", "scheme": "bearer" }), Vec::new()),
            Auth::Basic { .. } => ("basicAuth".to_owned(), json!({ "type": "http", "scheme": "basic" }), Vec::new()),
            Auth::Digest { .. } => ("digestAuth".to_owned(), json!({ "type": "http", "scheme": "digest" }), Vec::new()),
            Auth::Apikey { key, placement, .. } => {
                let place = if placement == "query" { "query" } else { "header" };
                let slug: String =
                    key.chars().map(|c| if c.is_ascii_alphanumeric() || "-._".contains(c) { c } else { '_' }).collect();
                (format!("apiKey_{slug}"), json!({ "type": "apiKey", "in": place, "name": key }), Vec::new())
            }
            Auth::Awsv4 { .. } => (
                "awsSigV4".to_owned(),
                json!({
                    "type": "apiKey",
                    "in": "header",
                    "name": "Authorization",
                    "description": "AWS Signature Version 4",
                    "x-amazon-apigateway-authtype": "awsSigv4",
                }),
                Vec::new(),
            ),
            Auth::Oauth2(config) => {
                let (key, flow, scopes) = oauth2_flow(config, vars);
                let unresolved =
                    flow.as_object().is_some_and(|f| f.values().any(|v| v.as_str().is_some_and(|s| s.contains("{{"))));
                if unresolved {
                    self.issues.push(format!("{at} : une adresse OAuth2 contient une variable sans valeur"));
                }
                let base = format!("oauth2{}{}", key[..1].to_uppercase(), &key[1..]);
                (base, json!({ "type": "oauth2", "flows": { key: flow } }), scopes)
            }
            Auth::Other { label, .. } => {
                self.issues.push(format!("{at} : authentification {label} non exportée"));
                return None;
            }
        };
        let name = self.register(&base, scheme);
        Some(json!([{ name: scopes }]))
    }

    /// Le nom sous lequel `scheme` est déclaré : celui d'un schéma identique déjà déclaré, sinon un nom libre.
    fn register(&mut self, base: &str, scheme: Value) -> String {
        let mut n = 1;
        loop {
            let name = if n == 1 { base.to_owned() } else { format!("{base}_{n}") };
            match self.schemes.get(&name) {
                Some(existing) if *existing == scheme => return name,
                Some(_) => n += 1,
                None => {
                    self.schemes.insert(name.clone(), scheme);
                    return name;
                }
            }
        }
    }

    /// Les serveurs globaux (ceux du préfixe le plus employé) et, par préfixe, les serveurs des autres opérations.
    fn servers(&mut self) -> (Vec<Value>, Vec<(String, Vec<Value>)>) {
        let mut counts: Vec<(Origin, usize)> = Vec::new();
        for origin in self.operations.iter().filter_map(|o| o.origin.clone()) {
            match counts.iter_mut().find(|(known, _)| *known == origin) {
                Some((_, n)) => *n += 1,
                None => counts.push((origin, 1)),
            }
        }
        let mut top: Option<usize> = None;
        for (i, (_, n)) in counts.iter().enumerate() {
            if top.is_none_or(|t| *n > counts[t].1) {
                top = Some(i);
            }
        }
        let (mut global, mut others) = (Vec::new(), Vec::new());
        for (i, (origin, _)) in counts.iter().enumerate() {
            let servers = self.servers_of(origin);
            if Some(i) == top {
                global = servers;
            } else {
                others.push((origin.key(), servers));
            }
        }
        (global, others)
    }

    fn servers_of(&mut self, origin: &Origin) -> Vec<Value> {
        let vars = self.variables;
        match origin {
            Origin::Literal(text) => vec![vars.server(text, None)],
            Origin::Variable(name) => {
                let alternatives = vars.alternatives(name);
                if alternatives.is_empty() {
                    let issue = format!("la variable {{{{{name}}}}} n'a aucune valeur : le serveur reste à renseigner");
                    if !self.issues.contains(&issue) {
                        self.issues.push(issue);
                    }
                    return vec![vars.server(&format!("{{{{{name}}}}}"), None)];
                }
                let several = alternatives.len() > 1;
                alternatives.iter().map(|(value, source)| vars.server(value, several.then(|| source.clone()))).collect()
            }
        }
    }

    fn paths(&mut self, servers: Vec<(String, Vec<Value>)>) -> JsonMap<String, Value> {
        let mut paths: JsonMap<String, Value> = JsonMap::new();
        for mut operation in std::mem::take(&mut self.operations) {
            if let Some(origin) = &operation.origin {
                if let Some((_, list)) = servers.iter().find(|(key, _)| *key == origin.key()) {
                    operation.value.insert("servers".into(), Value::Array(list.clone()));
                }
            }
            let item = paths.entry(operation.path).or_insert_with(|| json!({}));
            item[operation.method.as_str()] = Value::Object(operation.value);
        }
        paths
    }
}

fn split_url(url: &str) -> (Option<Origin>, String) {
    let url = url.split(['?', '#']).next().unwrap_or_default().trim();
    if url.starts_with('/') {
        return (None, url.to_owned());
    }
    if let Some((name, after)) = url.strip_prefix("{{").and_then(|rest| rest.split_once("}}")) {
        if after.is_empty() || after.starts_with('/') {
            return (Some(Origin::Variable(name.trim().to_owned())), after.to_owned());
        }
    }
    let (scheme, rest) = url.split_once("://").unwrap_or(("", url));
    let end = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..end];
    let origin = match (authority.is_empty(), scheme.is_empty()) {
        (true, _) => None,
        (false, true) => Some(authority.to_owned()),
        (false, false) => Some(format!("{scheme}://{authority}")),
    };
    (origin.map(Origin::Literal), rest[end..].to_owned())
}

/// Le chemin OpenAPI (`/users/{id}`) et les noms de ses paramètres : `:nom` quand la requête déclare ce paramètre de
/// chemin, `{{nom}}` toujours.
fn route_of(rest: &str, params: &[Param]) -> (String, Vec<String>) {
    let mut names: Vec<String> = Vec::new();
    let segments: Vec<String> = rest
        .split('/')
        .map(|segment| {
            let declared = segment
                .strip_prefix(':')
                .filter(|name| params.iter().any(|p| p.kind == ParamKind::Path && p.enabled && p.name == *name));
            if let Some(name) = declared {
                names.push(name.to_owned());
                return format!("{{{name}}}");
            }
            placeholder()
                .replace_all(segment, |c: &regex::Captures| {
                    names.push(c[1].to_owned());
                    format!("{{{}}}", &c[1])
                })
                .into_owned()
        })
        .collect();
    let path = segments.join("/");
    let path = if path.starts_with('/') { path } else { format!("/{path}") };
    (path, names)
}

/// Le type JSON d'une valeur texte et, quand elle est écrite en clair, son exemple.
fn typed(value: &str) -> (Value, Option<Value>) {
    let string = || json!({ "type": "string" });
    if value.is_empty() || value.contains("{{") {
        return (string(), None);
    }
    if let Some(n) = value.parse::<i64>().ok().filter(|n| n.to_string() == value) {
        return (json!({ "type": "integer" }), Some(json!(n)));
    }
    if let Some(n) = value.parse::<f64>().ok().filter(|n| n.is_finite()).and_then(serde_json::Number::from_f64) {
        return (json!({ "type": "number" }), Some(Value::Number(n)));
    }
    match value {
        "true" | "false" => (json!({ "type": "boolean" }), Some(json!(value == "true"))),
        _ => (string(), Some(json!(value))),
    }
}

fn parameter(place: &str, name: &str, value: &str, description: Option<&str>) -> Value {
    let mut parameter = JsonMap::new();
    parameter.insert("name".into(), json!(name));
    parameter.insert("in".into(), json!(place));
    if place == "path" {
        parameter.insert("required".into(), json!(true));
    }
    if let Some(description) = description.filter(|d| !d.is_empty()) {
        parameter.insert("description".into(), json!(description));
    }
    let (schema, example) = typed(value);
    parameter.insert("schema".into(), schema);
    if let Some(example) = example {
        parameter.insert("example".into(), example);
    }
    Value::Object(parameter)
}

fn parameters(params: &[Param], headers: &[KeyValue], path_names: &[String]) -> Vec<Value> {
    let mut list = Vec::new();
    let mut seen: HashSet<(&str, String)> = HashSet::new();
    for name in path_names {
        if seen.insert(("path", name.clone())) {
            let declared = params.iter().find(|p| p.kind == ParamKind::Path && p.enabled && p.name == *name);
            list.push(parameter(
                "path",
                name,
                declared.map_or("", |p| p.value.as_str()),
                declared.and_then(|p| p.description.as_deref()),
            ));
        }
    }
    for p in params.iter().filter(|p| p.kind == ParamKind::Query && p.enabled) {
        if seen.insert(("query", p.name.clone())) {
            list.push(parameter("query", &p.name, &p.value, p.description.as_deref()));
        }
    }
    let ignored = ["accept", "content-type", "authorization"];
    for h in headers.iter().filter(|h| h.enabled && !ignored.contains(&h.name.to_lowercase().as_str())) {
        if seen.insert(("header", h.name.to_lowercase())) {
            list.push(parameter("header", &h.name, &h.value, h.description.as_deref()));
        }
    }
    list
}

fn expected_status(assertions: &[Assertion]) -> u16 {
    assertions
        .iter()
        .filter(|a| a.enabled && a.expression.trim() == "res.status" && a.operator == "eq")
        .find_map(|a| a.value.as_deref()?.trim().parse::<u16>().ok().filter(|status| (100..=599).contains(status)))
        .unwrap_or(200)
}

/// Le schéma que décrit un exemple JSON : objets et listes sont parcourus, les valeurs donnent leur type.
fn infer(value: &Value) -> Value {
    match value {
        Value::Null => json!({ "nullable": true }),
        Value::Bool(_) => json!({ "type": "boolean" }),
        Value::Number(n) if n.is_f64() => json!({ "type": "number" }),
        Value::Number(_) => json!({ "type": "integer" }),
        Value::String(_) => json!({ "type": "string" }),
        Value::Array(items) => json!({ "type": "array", "items": items.first().map_or_else(|| json!({}), infer) }),
        Value::Object(fields) => {
            let properties: JsonMap<String, Value> = fields.iter().map(|(k, v)| (k.clone(), infer(v))).collect();
            json!({ "type": "object", "properties": Value::Object(properties) })
        }
    }
}

fn field_schema(value: &str, description: Option<&str>) -> Value {
    let (mut schema, example) = typed(value);
    if let Some(example) = example {
        schema["example"] = example;
    }
    if let Some(description) = description.filter(|d| !d.is_empty()) {
        schema["description"] = json!(description);
    }
    schema
}

fn form_object<'a>(fields: impl Iterator<Item = (&'a String, Value)>) -> Value {
    let properties: JsonMap<String, Value> = fields.map(|(name, schema)| (name.clone(), schema)).collect();
    json!({ "type": "object", "properties": Value::Object(properties) })
}

/// Le type de flux OpenAPI, sa définition et les portées demandées.
fn oauth2_flow(config: &OAuth2, vars: &Variables) -> (&'static str, Value, Vec<String>) {
    let key = match config.flow.as_str() {
        AUTHORIZATION_CODE => "authorizationCode",
        IMPLICIT => "implicit",
        PASSWORD => "password",
        CLIENT_CREDENTIALS => "clientCredentials",
        _ => "clientCredentials",
    };
    let scopes: Vec<String> = vars.expand(&config.scope).split_whitespace().map(str::to_owned).collect();
    let mut flow = JsonMap::new();
    if matches!(key, "authorizationCode" | "implicit") {
        flow.insert("authorizationUrl".into(), json!(vars.expand(&config.authorization_url)));
    }
    if key != "implicit" {
        flow.insert("tokenUrl".into(), json!(vars.expand(&config.access_token_url)));
        if !config.refresh_token_url.is_empty() {
            flow.insert("refreshUrl".into(), json!(vars.expand(&config.refresh_token_url)));
        }
    }
    let described: JsonMap<String, Value> = scopes.iter().map(|s| (s.clone(), json!(""))).collect();
    flow.insert("scopes".into(), Value::Object(described));
    (key, Value::Object(flow), scopes)
}
