//! Export d'une collection au format Postman v2.1, comme `bruno-to-postman.js` de Bruno : dossiers d'abord (par nom,
//! puis par `seq`), requêtes ensuite (par `seq`), authentification, corps, variables `{{…}}` repérées dans la
//! collection, scripts traduits vers `pm.*`.
//!
//! Ne sont pas exportés : les exemples de réponse, les requêtes qui ne sont ni HTTP ni GraphQL, les types
//! d'authentification que Postman ne connaît pas ici (NTLM, OAuth 1, EdgeGrid, WSSE) et les valeurs des secrets
//! d'environnement (aucun environnement n'est exporté).

use std::path::Path;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map as JsonMap, Value};
use xc_core::oauth2::{OAuth2, AUTHORIZATION_CODE, CLIENT_CREDENTIALS, IMPLICIT, PASSWORD};
use xc_core::yaml::{Map, Value as Yaml};
use xc_core::{
    open_collection, read_collection_file, read_folder_file, read_request, Auth, Body, CoreError, KeyValue,
    MultipartValue, Param, ParamKind, TreeItem,
};

use super::script::translate;
use super::Exported;

const SCHEMA: &str = "https://schema.getpostman.com/json/collection/v2.1.0/collection.json";

/// Exporte la collection du dossier `root`.
pub fn collection(root: &Path) -> Result<Exported, CoreError> {
    let info = open_collection(root)?;
    let file = read_collection_file(root)?;
    let mut issues = Vec::new();
    let items = items(root, &info.items, "", &mut issues)?;

    let mut out = JsonMap::new();
    let mut header = JsonMap::new();
    header.insert("name".into(), json!(info.name));
    if let Some(docs) = docs_of(&file) {
        header.insert("description".into(), json!(docs));
    }
    header.insert("schema".into(), json!(SCHEMA));
    out.insert("info".into(), Value::Object(header));
    out.insert("item".into(), Value::Array(items));
    let section = file.map("request");
    let auth = section
        .and_then(|r| authentication(&xc_core::request::auth_from(r.get("auth")), true, &mut issues, &info.name));
    let events = section.map(|r| events_of_scripts(&scripts_in(r))).unwrap_or_default();
    if let Some(auth) = &auth {
        out.insert("auth".into(), auth.clone());
    }
    if !events.is_empty() {
        out.insert("event".into(), Value::Array(events));
    }
    let variables = collection_variables(&out, &file);
    let mut ordered = JsonMap::new();
    for key in ["info", "item"] {
        if let Some(value) = out.remove(key) {
            ordered.insert(key.into(), value);
        }
    }
    ordered.insert("variable".into(), Value::Array(variables));
    ordered.extend(out);
    let out = ordered;
    let text = serde_json::to_string_pretty(&Value::Object(out))
        .map_err(|e| CoreError::Io { path: root.display().to_string(), message: e.to_string() })?;
    Ok(Exported { text, issues })
}

pub(super) fn docs_of(map: &Map) -> Option<String> {
    let text = match map.get("docs")? {
        Yaml::Map(m) => m.get("content").and_then(Yaml::scalar),
        other => other.scalar(),
    };
    text.filter(|t| !t.is_empty())
}

fn valid_seq(seq: Option<i64>) -> Option<i64> {
    seq.filter(|n| *n > 0)
}

enum Slot<'a> {
    One(&'a TreeItem),
    Group(Vec<&'a TreeItem>),
}

impl<'a> Slot<'a> {
    fn seq(&self) -> Option<i64> {
        match self {
            Self::One(item) => item.seq(),
            Self::Group(items) => items.first().and_then(|item| item.seq()),
        }
    }

    fn into_items(self) -> Vec<&'a TreeItem> {
        match self {
            Self::One(item) => vec![item],
            Self::Group(items) => items,
        }
    }
}

/// `sortByNameThenSequence` de Bruno : par nom, puis chaque élément qui a un `seq` valable est inséré à la place
/// `seq - 1` ; deux éléments de même `seq` restent côte à côte.
fn sorted_by_name_then_seq(folders: Vec<&TreeItem>) -> Vec<&TreeItem> {
    let mut alphabetical = folders;
    alphabetical.sort_by_key(|item| item.name().to_lowercase());
    let (mut with_seq, without): (Vec<&TreeItem>, Vec<&TreeItem>) =
        alphabetical.into_iter().partition(|item| valid_seq(item.seq()).is_some());
    with_seq.sort_by_key(|item| item.seq());
    let mut slots: Vec<Slot> = without.into_iter().map(Slot::One).collect();
    for item in with_seq {
        let position = usize::try_from(item.seq().unwrap_or(1) - 1).unwrap_or(0);
        match slots.get_mut(position) {
            Some(existing) if existing.seq() == item.seq() => {
                let mut group = std::mem::replace(existing, Slot::Group(Vec::new())).into_items();
                group.push(item);
                *existing = Slot::Group(group);
            }
            _ => slots.insert(position.min(slots.len()), Slot::One(item)),
        }
    }
    slots.into_iter().flat_map(Slot::into_items).collect()
}

pub(super) fn sorted_for_export(items: &[TreeItem]) -> Vec<&TreeItem> {
    let folders: Vec<&TreeItem> = items.iter().filter(|i| matches!(i, TreeItem::Folder { .. })).collect();
    let mut requests: Vec<&TreeItem> = items.iter().filter(|i| matches!(i, TreeItem::Request { .. })).collect();
    requests.sort_by_key(|item| item.seq().unwrap_or(i64::MAX));
    let mut sorted = sorted_by_name_then_seq(folders);
    sorted.extend(requests);
    sorted
}

fn items(root: &Path, tree: &[TreeItem], parent: &str, issues: &mut Vec<String>) -> Result<Vec<Value>, CoreError> {
    let mut out = Vec::new();
    for item in sorted_for_export(tree) {
        let at = if parent.is_empty() { item.name().to_owned() } else { format!("{parent} / {}", item.name()) };
        match item {
            TreeItem::Folder { path, name, children, .. } => {
                let file = read_folder_file(root, path)?;
                let mut folder = JsonMap::new();
                folder.insert("name".into(), json!(if name.is_empty() { "Untitled Folder" } else { name }));
                folder.insert("item".into(), Value::Array(items(root, children, &at, issues)?));
                let section = file.map("request");
                if let Some(auth) = section
                    .and_then(|r| authentication(&xc_core::request::auth_from(r.get("auth")), false, issues, &at))
                {
                    folder.insert("auth".into(), auth);
                }
                let events = section.map(|r| events_of_scripts(&scripts_in(r))).unwrap_or_default();
                if !events.is_empty() {
                    folder.insert("event".into(), Value::Array(events));
                }
                out.push(Value::Object(folder));
            }
            TreeItem::Request { path, name, .. } => {
                if let Some(request) = request_item(root, path, name, &at, issues)? {
                    out.push(request);
                }
            }
        }
    }
    Ok(out)
}

fn request_item(
    root: &Path,
    path: &str,
    name: &str,
    at: &str,
    issues: &mut Vec<String>,
) -> Result<Option<Value>, CoreError> {
    let doc = read_request(root, path)?;
    if doc.request_type != "http" && doc.request_type != "graphql" {
        issues.push(format!("{at} : requête {} ignorée, Postman ne la connaît pas ici", doc.request_type));
        return Ok(None);
    }
    let mut request = JsonMap::new();
    request.insert("method".into(), json!(if doc.method.is_empty() { "GET" } else { &doc.method }));
    request.insert("header".into(), Value::Array(headers(&doc.headers)));
    if let Some(auth) = authentication(&doc.auth, false, issues, at) {
        request.insert("auth".into(), auth);
    }
    request.insert("description".into(), json!(doc.docs.clone().unwrap_or_default()));
    request.insert("url".into(), transform_url(&sanitize_url(&doc.url), &doc.params));
    let has_body = doc.body != Body::None;
    if has_body {
        request.insert("body".into(), body(&doc.body));
    }

    let mut item = JsonMap::new();
    item.insert("name".into(), json!(if name.is_empty() { "Untitled Request" } else { name }));
    if has_body && matches!(doc.method.to_uppercase().as_str(), "GET" | "HEAD" | "OPTIONS") {
        item.insert("protocolProfileBehavior".into(), json!({ "disableBodyPruning": true }));
    }
    item.insert("request".into(), Value::Object(request));
    let own: Vec<(String, String)> = doc.scripts.iter().map(|s| (s.kind.clone(), s.code.clone())).collect();
    let events = events_of_scripts(&own);
    if !events.is_empty() {
        item.insert("event".into(), Value::Array(events));
    }
    Ok(Some(Value::Object(item)))
}

fn scripts_in(section: &Map) -> Vec<(String, String)> {
    section
        .seq("scripts")
        .iter()
        .filter_map(Yaml::as_map)
        .filter_map(|s| Some((s.str("type")?.to_owned(), s.get("code").and_then(Yaml::scalar)?)))
        .collect()
}

fn event(listen: &str, lines: &str) -> Value {
    json!({
        "listen": listen,
        "script": { "type": "text/javascript", "packages": {}, "requests": {}, "exec": lines.split('\n').collect::<Vec<_>>() },
    })
}

/// Les événements Postman : le script avant la requête, puis le script après la réponse suivi des tests, que
/// Postman réunit dans l'événement `test`.
fn events_of_scripts(scripts: &[(String, String)]) -> Vec<Value> {
    let of = |kind: &str| -> Vec<String> {
        scripts.iter().filter(|(k, c)| k == kind && !c.is_empty()).map(|(_, c)| translate(c)).collect()
    };
    let mut events = Vec::new();
    let before = of("before-request");
    if !before.is_empty() {
        events.push(event("prerequest", &before.join("\n")));
    }
    let (after, tests) = (of("after-response"), of("tests"));
    let mut exec = after.join("\n");
    if !tests.is_empty() {
        if !exec.is_empty() {
            exec.push_str("\n\n");
        }
        exec.push_str("// Tests\n");
        exec.push_str(&tests.join("\n"));
    }
    if !exec.is_empty() {
        events.push(event("test", &exec));
    }
    events
}

fn headers(headers: &[KeyValue]) -> Vec<Value> {
    headers
        .iter()
        .map(|h| {
            json!({
                "key": h.name,
                "value": h.value,
                "description": h.description.clone().unwrap_or_default(),
                "disabled": !h.enabled,
                "type": "default",
            })
        })
        .collect()
}

fn body(body: &Body) -> Value {
    let raw = |text: &str, language: &str| json!({ "mode": "raw", "raw": text, "options": { "raw": { "language": language } } });
    match body {
        Body::Json { data } => raw(data, "json"),
        Body::Xml { data } => raw(data, "xml"),
        Body::Text { data } => raw(data, "text"),
        Body::FormUrlEncoded { fields } => json!({
            "mode": "urlencoded",
            "urlencoded": fields.iter().map(|f| json!({
                "key": f.name,
                "value": f.value,
                "disabled": !f.enabled,
                "type": "default",
                "description": f.description.clone().unwrap_or_default(),
            })).collect::<Vec<_>>(),
        }),
        Body::MultipartForm { fields } => json!({
            "mode": "formdata",
            "formdata": fields.iter().map(|f| {
                let mut part = JsonMap::new();
                part.insert("key".into(), json!(f.name));
                part.insert("disabled".into(), json!(!f.enabled));
                part.insert("description".into(), json!(f.description.clone().unwrap_or_default()));
                match &f.value {
                    MultipartValue::File(paths) => {
                        part.insert("type".into(), json!("file"));
                        part.insert("src".into(), match paths.as_slice() {
                            [] => Value::Null,
                            [one] => json!(one),
                            many => json!(many),
                        });
                    }
                    MultipartValue::Text(text) => {
                        part.insert("type".into(), json!("text"));
                        part.insert("value".into(), json!(text));
                    }
                }
                if let Some(content_type) = f.content_type.as_ref().filter(|c| !c.is_empty()) {
                    part.insert("contentType".into(), json!(content_type));
                }
                Value::Object(part)
            }).collect::<Vec<_>>(),
        }),
        Body::Graphql { query, variables } => {
            json!({ "mode": "graphql", "graphql": { "query": query, "variables": variables } })
        }
        Body::None | Body::Other { .. } => json!({ "mode": "raw", "raw": "" }),
    }
}

fn entry(key: &str, value: impl Into<Value>) -> Value {
    json!({ "key": key, "value": value.into(), "type": "string" })
}

/// L'authentification dans la forme de Postman ; `None` quand elle est héritée (ou, pour la collection, absente) ou
/// que Postman ne la connaît pas ici.
fn authentication(auth: &Auth, collection: bool, issues: &mut Vec<String>, at: &str) -> Option<Value> {
    match auth {
        Auth::Inherit => None,
        Auth::Bearer { token } => Some(json!({ "type": "bearer", "bearer": [entry("token", token.as_str())] })),
        Auth::Basic { username, password } => Some(json!({
            "type": "basic",
            "basic": [entry("password", password.as_str()), entry("username", username.as_str())],
        })),
        Auth::Apikey { key, value, placement } => Some(json!({
            "type": "apikey",
            "apikey": [
                entry("key", key.as_str()),
                entry("value", value.as_str()),
                entry("in", if placement == "query" { "query" } else { "header" }),
            ],
        })),
        Auth::Digest { username, password } => Some(json!({
            "type": "digest",
            "digest": [entry("password", password.as_str()), entry("username", username.as_str())],
        })),
        Auth::Awsv4 { access_key_id, secret_access_key, session_token, service, region, .. } => Some(json!({
            "type": "awsv4",
            "awsv4": [
                entry("sessionToken", session_token.as_str()),
                entry("service", service.as_str()),
                entry("region", region.as_str()),
                entry("secretKey", secret_access_key.as_str()),
                entry("accessKey", access_key_id.as_str()),
            ],
        })),
        Auth::Oauth2(config) => Some(oauth2(config)),
        Auth::None => (!collection).then(|| json!({ "type": "noauth" })),
        Auth::Other { label, .. } => {
            issues.push(format!("{at} : authentification {label} non exportée"));
            (!collection).then(|| json!({ "type": "noauth" }))
        }
    }
}

fn oauth2(config: &OAuth2) -> Value {
    let grant = match config.flow.as_str() {
        AUTHORIZATION_CODE if config.pkce => "authorization_code_with_pkce",
        AUTHORIZATION_CODE => "authorization_code",
        PASSWORD => "password_credentials",
        IMPLICIT => "implicit",
        CLIENT_CREDENTIALS => "client_credentials",
        _ => "client_credentials",
    };
    let wanted: [(&str, String); 15] = [
        ("grant_type", grant.into()),
        ("accessTokenUrl", config.access_token_url.clone()),
        ("refreshTokenUrl", config.refresh_token_url.clone()),
        ("clientId", config.client_id.clone()),
        ("clientSecret", config.client_secret.clone()),
        ("scope", config.scope.clone()),
        ("state", config.state.clone()),
        ("tokenName", config.token_id.clone()),
        ("addTokenTo", if config.token_placement == "query" { "queryParams" } else { "header" }.into()),
        ("headerPrefix", config.token_prefix.clone()),
        ("client_authentication", if config.credentials_placement == "body" { "body" } else { "header" }.into()),
        ("authUrl", config.authorization_url.clone()),
        ("redirect_uri", config.callback_url.clone()),
        ("username", config.username.clone()),
        ("password", config.password.clone()),
    ];
    let mut params: Vec<Value> =
        wanted.into_iter().filter(|(_, value)| !value.is_empty()).map(|(key, value)| entry(key, value)).collect();
    for (stage, key) in
        [("authorization", "authRequestParams"), ("token", "tokenRequestParams"), ("refresh", "refreshRequestParams")]
    {
        let list: Vec<Value> = config
            .parameters
            .iter()
            .filter(|p| p.stage == stage)
            .map(|p| {
                let send_as = match p.placement.as_str() {
                    "query" => "request_url",
                    "body" => "request_body",
                    _ => "request_header",
                };
                json!({ "key": p.name, "value": p.value, "enabled": true, "send_as": send_as })
            })
            .collect();
        if !list.is_empty() {
            params.push(json!({ "key": key, "value": list, "type": "any" }));
        }
    }
    json!({ "type": "oauth2", "oauth2": params })
}

/// `\` devient `//`, puis toute suite de `/` se réduit à un seul, sauf après le `:` du schéma.
fn sanitize_url(url: &str) -> String {
    let url = url.replace('\\', "//");
    let chars: Vec<char> = url.chars().collect();
    let mut out = String::with_capacity(url.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '/' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i] == '/' {
            i += 1;
        }
        let run = i - start;
        let after_scheme = start > 0 && chars[start - 1] == ':';
        let kept = if after_scheme { run.min(2) } else { 1 };
        out.extend(std::iter::repeat_n('/', kept));
    }
    out
}

/// `transformUrl` de Bruno : le texte brut, puis le schéma, l'hôte coupé aux points, le chemin coupé aux barres, les
/// paramètres de requête et de chemin. Une adresse qui contient plusieurs `://` donne un objet vide.
fn transform_url(url: &str, params: &[Param]) -> Value {
    let parts: Vec<&str> = url.split("://").collect();
    let (protocol, raw_host_and_path) = match parts.as_slice() {
        [whole] => (String::new(), (*whole).to_owned()),
        [scheme, rest] => ((*scheme).to_owned(), rest.split('?').next().unwrap_or_default().to_owned()),
        _ => return json!({}),
    };
    let slash =
        raw_host_and_path.char_indices().find(|(i, c)| *c == '/' && *i + 1 < raw_host_and_path.len()).map(|(i, _)| i);
    let (host, path) = match slash {
        Some(i) => (&raw_host_and_path[..i], &raw_host_and_path[i + 1..]),
        None => (raw_host_and_path.as_str(), ""),
    };
    let pair = |p: &Param| {
        let mut entry = JsonMap::new();
        entry.insert("key".into(), json!(p.name));
        entry.insert("value".into(), json!(p.value));
        if let Some(description) = &p.description {
            entry.insert("description".into(), json!(description));
        }
        Value::Object(entry)
    };
    let of_kind = |kind: ParamKind| params.iter().filter(|p| p.kind == kind).map(pair).collect::<Vec<_>>();
    json!({
        "raw": url,
        "protocol": protocol,
        "host": if host.is_empty() { Vec::new() } else { host.split('.').collect() },
        "path": if path.is_empty() { Vec::new() } else { path.split('/').collect() },
        "query": of_kind(ParamKind::Query),
        "variable": of_kind(ParamKind::Path),
    })
}

/// Les variables de la collection : celles du fichier de la collection, puis chaque `{{nom}}` rencontré dans
/// l'export, sans doublon (la première occurrence l'emporte).
fn collection_variables(exported: &JsonMap<String, Value>, file: &Map) -> Vec<Value> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    let pattern = PATTERN.get_or_init(|| Regex::new(r"\{\{[^{}]+\}\}").expect("motif valide"));
    let mut found: Vec<(String, Value)> = Vec::new();
    let declared = xc_core::request::key_values(file.map("request").map(|r| r.seq("variables")).unwrap_or_default());
    for v in declared {
        let mut entry = json!({ "key": v.name, "value": v.value, "type": "default" });
        if !v.enabled {
            entry["disabled"] = json!(true);
        }
        found.push((v.name, entry));
    }
    fn walk(value: &Value, pattern: &Regex, found: &mut Vec<(String, Value)>) {
        match value {
            Value::String(text) => {
                for m in pattern.find_iter(text) {
                    let key = m.as_str().trim_start_matches("{{").trim_end_matches("}}").to_owned();
                    found.push((key.clone(), json!({ "key": key, "value": "", "type": "default" })));
                }
            }
            Value::Array(items) => items.iter().for_each(|v| walk(v, pattern, found)),
            Value::Object(map) => map.values().for_each(|v| walk(v, pattern, found)),
            _ => {}
        }
    }
    for key in ["item", "auth", "event"] {
        if let Some(value) = exported.get(key) {
            walk(value, pattern, &mut found);
        }
    }
    let mut seen = std::collections::HashSet::new();
    found.into_iter().filter(|(key, _)| seen.insert(key.clone())).map(|(_, entry)| entry).collect()
}
