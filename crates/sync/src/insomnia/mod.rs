//! Import d'une collection Insomnia (export v4 en JSON, export v5 en YAML) : port de `insomnia-to-bruno.js` et
//! `env-utils.js` des convertisseurs de Bruno. Le résultat est la collection JSON de Bruno, que
//! `import::write_plain_collection` écrit en OpenCollection YAML.
//!
//! Écarts voulus avec Bruno : les requêtes d'un dossier de l'export v4 ne sont pas écrites deux fois (Bruno les
//! duplique, et son test l'entérine) ; les éléments d'un même dossier gardent l'ordre d'Insomnia (`metaSortKey`) ;
//! l'auth Digest, API Key et OAuth 2 est convertie (Bruno ne garde que Basic et Bearer) ; ce qui n'a pas pu l'être est
//! signalé au lieu d'être perdu en silence.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::js::{entries, js_order, json_stringify, string, truthy};
use crate::openapi::{load_spec, OpenApiError};
use crate::postman::Issue;

#[derive(Debug, thiserror::Error)]
pub enum InsomniaError {
    #[error("export Insomnia illisible : {0}")]
    Syntax(String),
    #[error("aucune collection (workspace) dans l'export Insomnia")]
    NoWorkspace,
    #[error("format d'export Insomnia non reconnu : un objet est attendu")]
    Format,
}

pub struct Converted {
    pub collection: Value,
    pub issues: Vec<Issue>,
}

/// Convertit le texte d'un export Insomnia (JSON ou YAML).
pub fn collection_from_text(text: &str) -> Result<Converted, InsomniaError> {
    let value = load_spec(text).map_err(|e| match e {
        OpenApiError::Empty => InsomniaError::Syntax("le fichier est vide".into()),
        other => InsomniaError::Syntax(other.to_string()),
    })?;
    collection(&value)
}

pub fn collection(input: &Value) -> Result<Converted, InsomniaError> {
    if !input.is_object() {
        return Err(InsomniaError::Format);
    }
    let mut issues = Vec::new();
    let mut collection =
        if input.get("type").and_then(Value::as_str).is_some_and(|t| t.starts_with("collection.insomnia.rest/5")) {
            v5(input, &mut issues)
        } else {
            v4(input, &mut issues)?
        };
    collection["version"] = json!("1");
    Ok(Converted { collection, issues })
}

// ---------------------------------------------------------------------------------------------------------------
// Variables, noms

/// `{{ _.base_url }}` devient `{{base_url}}` : Insomnia préfixe ses variables d'environnement et tolère les espaces.
fn normalize_variables(value: &str) -> String {
    static VARIABLE: OnceLock<Regex> = OnceLock::new();
    let pattern = VARIABLE.get_or_init(|| Regex::new(r"\{\{.*?\}\}").expect("motif de variable"));
    pattern
        .replace_all(value, |caps: &regex::Captures<'_>| {
            let variable = &caps[0];
            variable.replacen("_.", "", 1).replace(' ', "")
        })
        .into_owned()
}

fn text(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(other) => string(other),
    }
}

fn normalized(v: Option<&Value>) -> String {
    normalize_variables(&text(v))
}

/// Le nom, suivi de `_n` quand `n` éléments plus haut dans la liste portent déjà le même.
fn with_suffix(name: &str, index: usize, siblings: &[&Value]) -> String {
    let before = siblings.iter().take(index).filter(|s| s.get("name").and_then(Value::as_str) == Some(name)).count();
    if before == 0 {
        name.to_owned()
    } else {
        format!("{name}_{before}")
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Requêtes

fn empty_body() -> Value {
    json!({ "mode": "none", "json": null, "text": null, "xml": null, "formUrlEncoded": [], "multipartForm": [] })
}

fn pair(entry: &Value, kind: Option<&str>) -> Value {
    let mut out = json!({
        "name": text(entry.get("name")),
        "value": normalized(entry.get("value")),
        "description": entry.get("description").cloned().unwrap_or(Value::Null),
        "enabled": !truthy(entry.get("disabled")),
    });
    if let Some(kind) = kind {
        out["type"] = json!(kind);
    }
    out
}

fn graphql(text_body: &str) -> Value {
    let parsed: Option<Value> = serde_json::from_str(text_body).ok();
    let Some(parsed) = parsed.filter(Value::is_object) else { return json!({ "query": "", "variables": "" }) };
    let variables =
        parsed.get("variables").filter(|v| !v.is_null()).map_or(Value::Null, |v| json!(json_stringify(v, true)));
    json!({ "query": normalize_variables(&text(parsed.get("query"))), "variables": variables })
}

fn auth(request: &Value, name: &str, issues: &mut Vec<Issue>) -> Value {
    let mut out = json!({ "mode": "none", "basic": null, "bearer": null, "digest": null });
    let Some(authentication) = request.get("authentication").filter(|a| a.is_object()) else { return out };
    if truthy(authentication.get("disabled")) {
        return out;
    }
    let kind = authentication.get("type").and_then(Value::as_str).unwrap_or_default();
    let get = |key: &str| normalized(authentication.get(key));
    match kind {
        "" | "none" => {}
        "basic" => {
            out["mode"] = json!("basic");
            out["basic"] = json!({ "username": get("username"), "password": get("password") });
        }
        "bearer" => {
            out["mode"] = json!("bearer");
            out["bearer"] = json!({ "token": get("token") });
        }
        "digest" => {
            out["mode"] = json!("digest");
            out["digest"] = json!({ "username": get("username"), "password": get("password") });
        }
        "iam" => {
            out["mode"] = json!("awsv4");
            out["awsv4"] = json!({
                "accessKeyId": get("accessKeyId"), "secretAccessKey": get("secretAccessKey"), "sessionToken": get("sessionToken"),
                "service": get("service"), "region": get("region"), "profileName": "",
            });
        }
        "apikey" => {
            out["mode"] = json!("apikey");
            let placement = if authentication.get("addTo").and_then(Value::as_str) == Some("queryParams") {
                "queryparams"
            } else {
                "header"
            };
            out["apikey"] = json!({ "key": get("key"), "value": get("value"), "placement": placement });
        }
        "oauth2" => match oauth2(authentication) {
            Some(config) => {
                out["mode"] = json!("oauth2");
                out["oauth2"] = config;
            }
            None => issues.push(Issue {
                path: name.to_owned(),
                severity: "warning",
                message: "Flux OAuth 2 non pris en charge : auth non importée".into(),
            }),
        },
        other => issues.push(Issue {
            path: name.to_owned(),
            severity: "warning",
            message: format!("Auth « {other} » non prise en charge : non importée"),
        }),
    }
    out
}

fn oauth2(a: &Value) -> Option<Value> {
    let get = |key: &str| normalized(a.get(key));
    let grant = match a.get("grantType").and_then(Value::as_str)? {
        "authorization_code" => "authorization_code",
        "client_credentials" => "client_credentials",
        "password" => "password",
        "implicit" => "implicit",
        _ => return None,
    };
    let mut config = json!({
        "grantType": grant,
        "accessTokenUrl": get("accessTokenUrl"),
        "clientId": get("clientId"),
        "clientSecret": get("clientSecret"),
        "scope": get("scope"),
        "state": get("state"),
        "credentialsPlacement": if truthy(a.get("credentialsInBody")) { "body" } else { "basic_auth_header" },
        "tokenPlacement": "header",
        "tokenHeaderPrefix": a.get("tokenPrefix").map_or_else(|| "Bearer".to_owned(), |_| get("tokenPrefix")),
        "tokenQueryKey": "access_token",
    });
    if matches!(grant, "authorization_code" | "implicit") {
        config["authorizationUrl"] = json!(get("authorizationUrl"));
        config["callbackUrl"] = json!(get("redirectUrl"));
    }
    if grant == "authorization_code" {
        config["pkce"] = json!(truthy(a.get("usePkce")));
    }
    if grant == "password" {
        config["username"] = json!(get("username"));
        config["password"] = json!(get("password"));
    }
    Some(config)
}

fn body(request: &Value, kind: &mut &'static str) -> Value {
    let mut out = empty_body();
    let source = request.get("body").filter(|b| b.is_object());
    let mime_type = source.and_then(|b| b.get("mimeType")).and_then(Value::as_str);
    let essence = mime_type.unwrap_or_default().split(';').next().unwrap_or_default().to_owned();
    let raw = || normalized(source.and_then(|b| b.get("text")));
    let params = || source.and_then(|b| b.get("params")).and_then(Value::as_array).cloned().unwrap_or_default();
    match essence.as_str() {
        "application/json" => {
            out["mode"] = json!("json");
            out["json"] = json!(raw());
        }
        "application/x-www-form-urlencoded" => {
            out["mode"] = json!("formUrlEncoded");
            out["formUrlEncoded"] = Value::Array(params().iter().map(|p| pair(p, None)).collect());
        }
        "multipart/form-data" => {
            out["mode"] = json!("multipartForm");
            out["multipartForm"] = Value::Array(
                params()
                    .iter()
                    .map(|p| {
                        let mut entry = pair(p, None);
                        entry["type"] = json!("text");
                        entry
                    })
                    .collect(),
            );
        }
        "text/xml" | "application/xml" => {
            out["mode"] = json!("xml");
            out["xml"] = json!(raw());
        }
        "application/graphql" => {
            *kind = "graphql-request";
            out["mode"] = json!("graphql");
            out["graphql"] = graphql(&text(source.and_then(|b| b.get("text"))));
        }
        _ if essence == "text/plain" || (mime_type == Some("") && truthy(source.and_then(|b| b.get("text")))) => {
            out["mode"] = json!("text");
            out["text"] = json!(raw());
        }
        _ => {}
    }
    out
}

/// Une requête Insomnia (v4 : ressource `request` ; v5 : élément avec `method` et `url`) en requête Bruno.
fn request(
    source: &Value,
    index: usize,
    siblings: &[&Value],
    seq: usize,
    issues: &mut Vec<Issue>,
    parent: &str,
) -> Value {
    let name = with_suffix(non_empty(source.get("name")).unwrap_or("Untitled Request"), index, siblings);
    let path = if parent.is_empty() { name.clone() } else { format!("{parent} / {name}") };
    let mut kind = "http-request";
    let body = body(source, &mut kind);

    let mut params: Vec<Value> = source
        .get("parameters")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| pair(p, Some("query")))
        .collect();
    params.extend(source.get("pathParameters").and_then(Value::as_array).into_iter().flatten().map(|p| {
        json!({ "name": text(p.get("name")), "value": normalized(p.get("value")), "description": "", "type": "path", "enabled": true })
    }));
    let encode_url = source.pointer("/settings/encodeUrl") != Some(&Value::Bool(false))
        && source.get("settingEncodeUrl") != Some(&Value::Bool(false));
    json!({
        "name": name,
        "type": kind,
        "seq": seq,
        "request": {
            "url": normalized(source.get("url")),
            "method": source.get("method").cloned().unwrap_or(Value::Null),
            "auth": auth(source, &path, issues),
            "headers": source.get("headers").and_then(Value::as_array).into_iter().flatten().map(|h| pair(h, None)).collect::<Vec<_>>(),
            "params": params,
            "body": body,
        },
        "settings": { "encodeUrl": encode_url, "forwardAuthorizationHeader": false },
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Environnements

/// `a.b[0].c` : les clés aplaties d'un objet, comme `flattenObject` ; seules les valeurs simples restent.
fn flatten(value: &Value, prefix: &str, out: &mut Vec<(String, Value)>) {
    match value {
        Value::Array(list) => {
            for (i, item) in list.iter().enumerate() {
                let path = if prefix.is_empty() { format!("[{i}]") } else { format!("{prefix}[{i}]") };
                flatten(item, &path, out);
            }
        }
        Value::Object(map) => {
            for (key, item) in entries(map) {
                let path = if prefix.is_empty() { key.clone() } else { format!("{prefix}.{key}") };
                flatten(item, &path, out);
            }
        }
        scalar => match out.iter_mut().find(|(k, _)| k == prefix) {
            Some(slot) => slot.1 = scalar.clone(),
            None => out.push((prefix.to_owned(), scalar.clone())),
        },
    }
}

fn flat(data: Option<&Value>) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    if let Some(data) = data.filter(|d| d.is_object() || d.is_array()) {
        flatten(data, "", &mut out);
    }
    js_order(out)
}

/// Fusion superficielle : les clés de `over` remplacent celles de `base`, les nouvelles s'ajoutent.
fn merged(base: &[(String, Value)], over: &[(String, Value)]) -> Vec<(String, Value)> {
    let mut out = base.to_vec();
    for (key, value) in over {
        match out.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1.clone_from(value),
            None => out.push((key.clone(), value.clone())),
        }
    }
    out
}

fn environment(name: Option<&str>, variables: &[(String, Value)], index: usize) -> Value {
    let name = name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map_or_else(|| format!("Environment {}", index + 1), str::to_owned);
    json!({
        "name": name,
        "variables": js_order(variables.to_vec()).iter().map(|(k, v)| json!({ "name": k, "value": string(v), "type": "text", "enabled": true, "secret": false })).collect::<Vec<_>>(),
    })
}

/// Environnements d'un export v5 : l'environnement de base, puis chaque sous-environnement fusionné sur lui.
fn v5_environments(base: &Value) -> Vec<Value> {
    if !base.is_object() {
        return Vec::new();
    }
    let base_flat = flat(base.get("data"));
    let mut out = vec![environment(base.get("name").and_then(Value::as_str), &base_flat, 0)];
    for (i, sub) in base.get("subEnvironments").and_then(Value::as_array).into_iter().flatten().enumerate() {
        out.push(environment(
            sub.get("name").and_then(Value::as_str),
            &merged(&base_flat, &flat(sub.get("data"))),
            i + 1,
        ));
    }
    out
}

/// Environnements d'un export v4 : celui dont le parent est le workspace est la base, les autres s'y fusionnent.
fn v4_environments(resources: &[Value], workspace: &str) -> Vec<Value> {
    let envs: Vec<&Value> =
        resources.iter().filter(|r| r.get("_type").and_then(Value::as_str) == Some("environment")).collect();
    let is_base = |e: &&Value| e.get("parentId").and_then(Value::as_str) == Some(workspace);
    let base = envs.iter().find(|e| is_base(e));
    let base_flat = base.map(|b| flat(b.get("data"))).unwrap_or_default();
    let mut out = Vec::new();
    if let Some(base) = base {
        out.push(environment(base.get("name").and_then(Value::as_str), &base_flat, 0));
    }
    for (i, sub) in envs.iter().filter(|e| !is_base(e)).enumerate() {
        out.push(environment(
            sub.get("name").and_then(Value::as_str),
            &merged(&base_flat, &flat(sub.get("data"))),
            i + 1,
        ));
    }
    out
}

// ---------------------------------------------------------------------------------------------------------------
// v4

fn sort_key(v: &Value) -> f64 {
    v.get("metaSortKey").and_then(Value::as_f64).unwrap_or(0.0)
}

fn children<'a>(resources: &'a [Value], kind: &str, parent: &str) -> Vec<&'a Value> {
    let mut found: Vec<&Value> = resources
        .iter()
        .filter(|r| {
            r.get("_type").and_then(Value::as_str) == Some(kind)
                && r.get("parentId").and_then(Value::as_str) == Some(parent)
        })
        .collect();
    found.sort_by(|a, b| sort_key(a).partial_cmp(&sort_key(b)).unwrap_or(std::cmp::Ordering::Equal));
    found
}

fn folder(name: &str, items: Vec<Value>) -> Value {
    json!({ "name": name, "type": "folder", "items": items, "root": { "meta": { "name": name } } })
}

fn v4_items(resources: &[Value], parent: &str, parent_path: &str, issues: &mut Vec<Issue>) -> Vec<Value> {
    let groups = children(resources, "request_group", parent);
    let requests = children(resources, "request", parent);
    let mut out: Vec<Value> = groups
        .iter()
        .enumerate()
        .map(|(i, group)| {
            let name = with_suffix(&text(group.get("name")), i, &groups);
            let path = if parent_path.is_empty() { name.clone() } else { format!("{parent_path} / {name}") };
            folder(&name, v4_items(resources, &text(group.get("_id")), &path, issues))
        })
        .collect();
    out.extend(requests.iter().enumerate().map(|(i, r)| request(r, i, &requests, i + 1, issues, parent_path)));
    out
}

fn v4(data: &Value, issues: &mut Vec<Issue>) -> Result<Value, InsomniaError> {
    let resources: Vec<Value> = data.get("resources").and_then(Value::as_array).cloned().unwrap_or_default();
    let workspace = resources
        .iter()
        .find(|r| r.get("_type").and_then(Value::as_str) == Some("workspace"))
        .ok_or(InsomniaError::NoWorkspace)?;
    let id = text(workspace.get("_id"));
    Ok(json!({
        "name": workspace.get("name").cloned().unwrap_or(Value::Null),
        "items": v4_items(&resources, &id, "", issues),
        "environments": v4_environments(&resources, &id),
    }))
}

// ---------------------------------------------------------------------------------------------------------------
// v5

fn v5_items(list: &[Value], parent_path: &str, issues: &mut Vec<Issue>) -> Vec<Value> {
    let siblings: Vec<&Value> = list.iter().collect();
    let mut seq = 0;
    let mut out = Vec::new();
    for (index, item) in list.iter().enumerate() {
        if item.is_null() {
            continue;
        }
        if truthy(item.get("method")) && truthy(item.get("url")) {
            seq += 1;
            out.push(request(item, index, &siblings, seq, issues, parent_path));
        } else if let Some(children) = item.get("children").and_then(Value::as_array) {
            let name = non_empty(item.get("name")).unwrap_or("Untitled Folder");
            let path = if parent_path.is_empty() { name.to_owned() } else { format!("{parent_path} / {name}") };
            out.push(folder(name, v5_items(children, &path, issues)));
        } else {
            let name = non_empty(item.get("name")).map_or_else(|| format!("Item {}", index + 1), str::to_owned);
            let path = if parent_path.is_empty() { name } else { format!("{parent_path} / {name}") };
            issues.push(Issue {
                path,
                severity: "error",
                message: "Élément ignoré : ni requête (method et url), ni dossier (children)".into(),
            });
        }
    }
    out
}

fn v5(data: &Value, issues: &mut Vec<Issue>) -> Value {
    let name = non_empty(data.get("name")).unwrap_or("Untitled Collection");
    let items =
        data.get("collection").and_then(Value::as_array).map(|list| v5_items(list, "", issues)).unwrap_or_default();
    let environments = data.get("environments").map(v5_environments).unwrap_or_default();
    json!({ "name": name, "items": items, "environments": environments })
}

fn non_empty(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}
