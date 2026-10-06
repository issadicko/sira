//! Import d'une collection ou d'un environnement Postman (v2.0 et v2.1) : port de `postman-to-bruno.js` et de
//! `postman-env-to-bruno-env.js` des convertisseurs de Bruno. Le résultat est la collection JSON de Bruno, que
//! `import::write_plain_collection` écrit en OpenCollection YAML.
//!
//! Écarts avec Bruno : les scripts sont traduits par la seule table de remplacements (voir `script`), les paquets
//! `pm.require` ne sont pas inventoriés, et aucun identifiant interne (`uid`) n'est produit, l'écriture n'en a pas besoin.

mod script;

use std::sync::OnceLock;

use regex::Regex;
use serde::Serialize;
use serde_json::{json, Map, Value};

const SCHEMAS: [&str; 4] = [
    "https://schema.getpostman.com/json/collection/v2.0.0/collection.json",
    "https://schema.getpostman.com/json/collection/v2.1.0/collection.json",
    "https://schema.postman.com/json/collection/v2.0.0/collection.json",
    "https://schema.postman.com/json/collection/v2.1.0/collection.json",
];

#[derive(Debug, thiserror::Error)]
pub enum PostmanError {
    #[error("JSON invalide : {0}")]
    Syntax(String),
    #[error("version de schéma Postman non prise en charge : seules les collections v2.0 et v2.1 le sont")]
    Schema,
    #[error("environnement Postman invalide : {0}")]
    Environment(&'static str),
}

/// Ce que l'import n'a pas pu convertir ou a corrigé en route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Issue {
    /// Chemin de l'élément dans la collection (`Dossier / Requête`).
    pub path: String,
    /// `error` : élément ignoré ; `warning` : élément gardé, un réglage est perdu.
    pub severity: &'static str,
    pub message: String,
}

pub struct Converted {
    /// La collection JSON de Bruno.
    pub collection: Value,
    pub issues: Vec<Issue>,
}

/// Convertit le texte JSON d'une collection Postman.
pub fn collection_from_text(text: &str) -> Result<Converted, PostmanError> {
    let value: Value = serde_json::from_str(text).map_err(|e| PostmanError::Syntax(e.to_string()))?;
    collection(&value)
}

/// Convertit une collection Postman déjà lue ; les exports récents l'enveloppent dans `{ "collection": … }`.
pub fn collection(input: &Value) -> Result<Converted, PostmanError> {
    let root =
        if input.pointer("/collection/info").is_some_and(|i| !i.is_null()) { &input["collection"] } else { input };
    let schema = root.pointer("/info/schema").and_then(Value::as_str).unwrap_or_default();
    if !SCHEMAS.contains(&schema) {
        return Err(PostmanError::Schema);
    }
    let name = non_empty(root.pointer("/info/name")).unwrap_or("Untitled Collection");
    let mut issues = Vec::new();

    let mut request = json!({
        "auth": empty_auth("none"),
        "headers": [],
        "script": {},
        "tests": "",
        "vars": {},
    });
    if let Some(events) = root.get("event") {
        import_scripts(events, &mut request);
    }
    if let Some(variables) = root.get("variable").and_then(Value::as_array) {
        request["vars"]["req"] = Value::Array(variables.iter().filter_map(collection_variable).collect());
    }
    process_auth(root.get("auth"), &mut request);

    let mut collection = json!({
        "name": name,
        "version": "1",
        "items": [],
        "environments": [],
        "root": {
            "docs": description(root.pointer("/info/description")),
            "meta": { "name": name },
            "request": request,
        },
    });
    collection["items"] = Value::Array(items(root.get("item"), "", &mut issues));
    Ok(Converted { collection, issues })
}

/// Convertit le texte JSON d'un environnement Postman en environnement Bruno.
pub fn environment_from_text(text: &str) -> Result<Value, PostmanError> {
    let value: Value = serde_json::from_str(text).map_err(|e| PostmanError::Syntax(e.to_string()))?;
    environment(&value)
}

pub fn environment(input: &Value) -> Result<Value, PostmanError> {
    let object = input.as_object().ok_or(PostmanError::Environment("un objet est attendu"))?;
    let variables: Vec<Value> = object
        .get("values")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|v| !(v.get("key").is_none_or(Value::is_null) && v.get("value").is_none_or(Value::is_null)))
        .map(|v| {
            json!({
                "name": variable_name(v.get("key")),
                "value": v.get("value").map_or_else(|| Value::String(String::new()), |value| if value.is_null() { Value::String(String::new()) } else { value.clone() }),
                "enabled": v.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                "type": "text",
                "secret": v.get("type").and_then(Value::as_str) == Some("secret"),
            })
        })
        .collect();
    Ok(json!({ "name": object.get("name").cloned().unwrap_or(Value::Null), "variables": variables }))
}

/// Un nom de variable : tout caractère hors `[A-Za-z0-9_.-]` devient `_` (comme `invalidVariableCharacterRegex`).
fn variable_name(key: Option<&Value>) -> String {
    key.and_then(Value::as_str)
        .unwrap_or_default()
        .chars()
        .map(|c| if (c.is_ascii() && c.is_alphanumeric()) || matches!(c, '_' | '-' | '.') { c } else { '_' })
        .collect()
}

fn collection_variable(v: &Value) -> Option<Value> {
    if v.get("key").is_none_or(Value::is_null) && v.get("value").is_none_or(Value::is_null) {
        return None;
    }
    let value = match v.get("value") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    };
    Some(json!({
        "name": variable_name(v.get("key")),
        "value": value,
        "enabled": !truthy(v.get("disabled")),
    }))
}

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(_) => true,
    }
}

fn non_empty(v: Option<&Value>) -> Option<&str> {
    v.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// `ensureString` : une valeur absente ou vide est `""`, un objet est sérialisé, le reste est mis en texte.
fn ensure(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

/// `ensureString(valeur, null)` : `null` quand la valeur est absente ou vide.
fn ensure_or_null(v: Option<&Value>) -> Value {
    match ensure(v) {
        s if s.is_empty() => Value::Null,
        s => Value::String(s),
    }
}

/// Une description Postman est un texte ou un objet `{ content, type }`.
fn description(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(o)) => o.get("content").and_then(Value::as_str).unwrap_or_default().to_owned(),
        _ => String::new(),
    }
}

fn empty_auth(mode: &str) -> Value {
    json!({
        "mode": mode,
        "basic": null, "bearer": null, "awsv4": null, "apikey": null,
        "oauth1": null, "oauth2": null, "digest": null, "ntlm": null,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Adresse, en-têtes

fn construct_url(url: Option<&Value>) -> String {
    match url {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(o)) => match o.get("raw").and_then(Value::as_str).filter(|r| !r.is_empty()) {
            Some(raw) => raw.split('#').next().unwrap_or_default().to_owned(),
            None => url_from_parts(o),
        },
        _ => String::new(),
    }
}

fn url_from_parts(url: &Map<String, Value>) -> String {
    let protocol = url.get("protocol").and_then(Value::as_str).unwrap_or("http");
    let host = match url.get("host") {
        Some(Value::Array(parts)) => {
            parts.iter().filter_map(Value::as_str).filter(|s| !s.is_empty()).collect::<Vec<_>>().join(".")
        }
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let path = match url.get("path") {
        Some(Value::Array(parts)) => parts.iter().map(|p| ensure(Some(p))).collect::<Vec<_>>().join("/"),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let port = match url.get("port") {
        Some(p) if truthy(Some(p)) => format!(":{}", ensure(Some(p))),
        _ => String::new(),
    };
    let query = match url.get("query").and_then(Value::as_array) {
        Some(list) if !list.is_empty() => {
            let pairs: Vec<String> = list
                .iter()
                .filter(|q| non_empty(q.get("key")).is_some())
                .map(|q| format!("{}={}", ensure(q.get("key")), ensure(q.get("value"))))
                .collect();
            format!("?{}", pairs.join("&"))
        }
        _ => String::new(),
    };
    let path = if path.is_empty() { String::new() } else { format!("/{path}") };
    format!("{protocol}://{host}{port}{path}{query}")
}

/// `"Clé: valeur"` en `{ key, value }`.
fn parse_string_header(header: &str) -> Value {
    match header.split_once(':') {
        Some((key, value)) => json!({ "key": key.trim(), "value": value.trim() }),
        None => json!({ "key": header.trim(), "value": "" }),
    }
}

/// Les en-têtes d'une requête Postman : une liste d'objets ou de textes, ou un seul texte multi-lignes.
fn normalize_headers(headers: Option<&Value>) -> Vec<Value> {
    match headers {
        Some(Value::String(s)) => s.lines().filter(|l| !l.is_empty()).map(parse_string_header).collect(),
        Some(Value::Array(list)) => list
            .iter()
            .map(|h| match h {
                Value::String(s) => parse_string_header(s),
                other => other.clone(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn is_blank(item: &Value) -> bool {
    item.get("key").is_none_or(Value::is_null) && item.get("value").is_none_or(Value::is_null)
}

fn header_entry(header: &Value) -> Option<Value> {
    if !header.is_object() || is_blank(header) {
        return None;
    }
    Some(json!({
        "name": ensure(header.get("key")),
        "value": ensure(header.get("value")),
        "description": description(header.get("description")),
        "enabled": !truthy(header.get("disabled")),
    }))
}

fn param_entry(param: &Value, kind: &str) -> Option<Value> {
    if !param.is_object() || is_blank(param) {
        return None;
    }
    Some(json!({
        "name": ensure(param.get("key")),
        "value": ensure(param.get("value")),
        "description": description(param.get("description")),
        "type": kind,
        "enabled": !truthy(param.get("disabled")),
    }))
}

fn path_param(param: &Value) -> Option<Value> {
    non_empty(param.get("key"))?;
    Some(json!({
        "name": ensure(param.get("key")),
        "value": ensure(param.get("value")),
        "description": description(param.get("description")),
        "type": "path",
        "enabled": true,
    }))
}

// ---------------------------------------------------------------------------------------------------------------
// Scripts

/// Les scripts des événements `prerequest` (script pré-requête) et `test` (script post-réponse).
fn import_scripts(events: &Value, request: &mut Value) {
    for event in events.as_array().into_iter().flatten() {
        let Some(exec) = event.pointer("/script/exec").filter(|e| !e.is_null()) else { continue };
        let key = match event.get("listen").and_then(Value::as_str) {
            Some("prerequest") => "req",
            Some("test") => "res",
            _ => continue,
        };
        let empty = match exec {
            Value::Array(lines) => lines.is_empty(),
            Value::String(s) => s.is_empty(),
            _ => true,
        };
        if !request["script"].is_object() {
            request["script"] = json!({});
        }
        request["script"][key] = Value::String(if empty { String::new() } else { script::translate(exec) });
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Authentification

fn auth_values(auth: &Value, kind: &str) -> Map<String, Value> {
    match auth.get(kind) {
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(|entry| {
                Some((entry.get("key")?.as_str()?.to_owned(), entry.get("value").cloned().unwrap_or(Value::Null)))
            })
            .collect(),
        Some(Value::Object(o)) => o.clone(),
        _ => Map::new(),
    }
}

fn process_auth(auth: Option<&Value>, request: &mut Value) {
    let Some(auth) = auth.filter(|a| !a.is_null()) else { return };
    let kind = auth.get("type").and_then(Value::as_str).unwrap_or_default();
    if kind == "noauth" {
        request["auth"]["mode"] = json!("none");
        return;
    }
    let values = auth_values(auth, kind);
    let v = |key: &str| ensure(values.get(key));
    request["auth"]["mode"] = json!(kind);
    match kind {
        "basic" => request["auth"]["basic"] = json!({ "username": v("username"), "password": v("password") }),
        "bearer" => request["auth"]["bearer"] = json!({ "token": v("token") }),
        "awsv4" => {
            request["auth"]["awsv4"] = json!({
                "accessKeyId": v("accessKey"), "secretAccessKey": v("secretKey"), "sessionToken": v("sessionToken"),
                "service": v("service"), "region": v("region"), "profileName": "",
            });
        }
        "apikey" => {
            let placement =
                if values.get("in").and_then(Value::as_str) == Some("query") { "queryparams" } else { "header" };
            request["auth"]["apikey"] = json!({ "key": v("key"), "value": v("value"), "placement": placement });
        }
        "digest" => request["auth"]["digest"] = json!({ "username": v("username"), "password": v("password") }),
        "edgegrid" => {
            request["auth"]["mode"] = json!("akamai-edgegrid");
            request["auth"]["akamaiEdgegrid"] = json!({
                "accessToken": v("accessToken"), "clientToken": v("clientToken"), "clientSecret": v("clientSecret"),
                "nonce": v("nonce"), "timestamp": v("timestamp"), "baseURL": v("baseURL"),
                "headersToSign": v("headersToSign"), "maxBodySize": max_body_size(values.get("maxBodySize")),
            });
        }
        "ntlm" => {
            request["auth"]["ntlm"] =
                json!({ "username": v("username"), "password": v("password"), "domain": v("domain") })
        }
        "oauth1" => {
            request["auth"]["oauth1"] = json!({
                "consumerKey": v("consumerKey"), "consumerSecret": v("consumerSecret"),
                "accessToken": v("token"), "accessTokenSecret": v("tokenSecret"),
                "callbackUrl": ensure_or_null(values.get("callback")), "verifier": ensure_or_null(values.get("verifier")),
                "signatureMethod": or_default(&v("signatureMethod"), "HMAC-SHA1"),
                "privateKey": ensure_or_null(values.get("privateKey")), "privateKeyType": "text",
                "timestamp": ensure_or_null(values.get("timestamp")), "nonce": ensure_or_null(values.get("nonce")),
                "version": or_default(&v("version"), "1.0"), "realm": ensure_or_null(values.get("realm")),
                "placement": if values.get("addParamsToHeader") == Some(&Value::Bool(false)) { "query" } else { "header" },
                "includeBodyHash": truthy(values.get("includeBodyHash")),
            });
        }
        "oauth2" => request["auth"]["oauth2"] = oauth2(&values),
        _ => request["auth"]["mode"] = json!("none"),
    }
}

fn or_default(value: &str, default: &str) -> String {
    if value.is_empty() {
        default.to_owned()
    } else {
        value.to_owned()
    }
}

fn max_body_size(v: Option<&Value>) -> Value {
    let number = match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) if !s.is_empty() => s.trim().parse::<f64>().ok(),
        _ => None,
    };
    number.filter(|n| n.is_finite()).map_or(Value::Null, |n| json!(n))
}

fn oauth2(values: &Map<String, Value>) -> Value {
    let v = |key: &str| ensure(values.get(key));
    let postman_grant = v("grant_type");
    let grant = match postman_grant.as_str() {
        "authorization_code_with_pkce" | "authorization_code" => "authorization_code",
        "password_credentials" => "password",
        "implicit" => "implicit",
        _ => "client_credentials",
    };
    let send_in = |param: &Value| match param.get("send_as").and_then(Value::as_str) {
        Some("request_url") => "queryparams",
        Some("request_body") => "body",
        _ => "headers",
    };
    let requests = |key: &str| -> Option<Value> {
        let list = values.get(key)?.as_array()?;
        Some(Value::Array(
            list.iter()
                .map(|p| {
                    json!({
                        "name": ensure(p.get("key")), "value": ensure(p.get("value")),
                        "sendIn": send_in(p), "enabled": p.get("enabled").and_then(Value::as_bool) != Some(false),
                    })
                })
                .collect(),
        ))
    };
    let mut additional = Map::new();
    for (source, target) in
        [("authRequestParams", "authorization"), ("tokenRequestParams", "token"), ("refreshRequestParams", "refresh")]
    {
        if let Some(list) = requests(source) {
            additional.insert(target.to_owned(), list);
        }
    }
    if !additional.is_empty() && grant == "authorization_code" && !additional.contains_key("authorization") {
        additional.insert("authorization".into(), json!([]));
    }
    let mut config = Map::new();
    if !additional.is_empty() {
        config.insert("additionalParameters".into(), Value::Object(additional));
    }
    for (key, value) in [
        ("grantType", json!(grant)),
        ("accessTokenUrl", json!(v("accessTokenUrl"))),
        ("refreshTokenUrl", json!(v("refreshTokenUrl"))),
        ("clientId", json!(v("clientId"))),
        ("clientSecret", json!(v("clientSecret"))),
        ("scope", json!(v("scope"))),
        ("state", json!(v("state"))),
        ("tokenPlacement", json!(if v("addTokenTo") == "header" { "header" } else { "url" })),
        ("tokenHeaderPrefix", json!(v("headerPrefix"))),
        ("tokenQueryKey", json!("access_token")),
        (
            "credentialsPlacement",
            json!(if v("client_authentication") == "body" { "body" } else { "basic_auth_header" }),
        ),
        ("credentialsId", json!(v("tokenName"))),
    ] {
        config.insert(key.to_owned(), value);
    }
    match postman_grant.as_str() {
        "authorization_code" | "authorization_code_with_pkce" => {
            config.insert("authorizationUrl".into(), json!(v("authUrl")));
            config.insert("callbackUrl".into(), json!(v("redirect_uri")));
            config.insert("pkce".into(), json!(postman_grant == "authorization_code_with_pkce"));
        }
        "password_credentials" => {
            config.insert("username".into(), json!(v("username")));
            config.insert("password".into(), json!(v("password")));
        }
        "implicit" => {
            config.insert("authorizationUrl".into(), json!(v("authUrl")));
            config.insert("callbackUrl".into(), json!(v("redirect_uri")));
        }
        _ => {}
    }
    Value::Object(config)
}

// ---------------------------------------------------------------------------------------------------------------
// Corps

/// Le langage que dit l'en-tête `Content-Type` actif : `json` (`application/json`, `…/vnd.api+json`) ou `xml`. Les mêmes
/// motifs que Bruno, sensibles à la casse.
fn language_of_headers(headers: Option<&Value>) -> Option<&'static str> {
    static JSON: OnceLock<Regex> = OnceLock::new();
    static XML: OnceLock<Regex> = OnceLock::new();
    let json = JSON.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]+/([A-Za-z0-9_-]+\+)?json").expect("motif JSON"));
    let xml = XML.get_or_init(|| Regex::new(r"^[A-Za-z0-9_-]+/([A-Za-z0-9_-]+\+)?xml").expect("motif XML"));
    let header = normalize_headers(headers).into_iter().find(|h| {
        h.get("key").and_then(Value::as_str).is_some_and(|k| k.eq_ignore_ascii_case("content-type"))
            && !truthy(h.get("disabled"))
    })?;
    let value = header.get("value").and_then(Value::as_str)?;
    if json.is_match(value) {
        Some("json")
    } else if xml.is_match(value) {
        Some("xml")
    } else {
        None
    }
}

fn inferred_content_type(path: &str) -> &'static str {
    let extension = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match extension.as_str() {
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp3" => "audio/mpeg",
        "mp4" => "video/mp4",
        _ => "application/octet-stream",
    }
}

fn empty_body() -> Value {
    json!({ "mode": "none", "json": null, "text": null, "xml": null, "formUrlEncoded": [], "multipartForm": [], "file": [] })
}

fn multipart_entry(param: &Value) -> Option<Value> {
    if !param.is_object() || is_blank(param) {
        return None;
    }
    let src = param.get("src").filter(|s| truthy(Some(s)));
    let is_file = param.get("type").and_then(Value::as_str) == Some("file")
        || (param.get("type").and_then(Value::as_str) == Some("default") && src.is_some());
    let value = if is_file {
        match src {
            Some(Value::Array(list)) => Value::Array(list.clone()),
            Some(single) => json!([single]),
            None => json!([]),
        }
    } else {
        match param.get("value") {
            Some(Value::Array(parts)) => Value::String(parts.iter().map(|p| ensure(Some(p))).collect()),
            other => Value::String(ensure(other)),
        }
    };
    let mut entry = json!({
        "type": if is_file { "file" } else { "text" },
        "name": ensure(param.get("key")),
        "value": value,
        "description": description(param.get("description")),
        "enabled": !truthy(param.get("disabled")),
    });
    if let Some(content_type) = non_empty(param.get("contentType")) {
        entry["contentType"] = json!(content_type);
    }
    Some(entry)
}

/// Remplit `body` d'après le corps Postman (`mode` : formdata, urlencoded, raw, file).
fn import_body(source: &Value, headers: Option<&Value>, body: &mut Value) {
    let Some(mode) = source.get("mode").and_then(Value::as_str) else { return };
    match mode {
        "formdata" => {
            body["mode"] = json!("multipartForm");
            body["multipartForm"] = Value::Array(
                source
                    .get("formdata")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(multipart_entry)
                    .collect(),
            );
        }
        "urlencoded" => {
            body["mode"] = json!("formUrlEncoded");
            body["formUrlEncoded"] = Value::Array(
                source
                    .get("urlencoded")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter(|p| p.is_object() && !is_blank(p))
                    .map(|p| {
                        json!({
                            "name": ensure(p.get("key")), "value": ensure(p.get("value")),
                            "description": description(p.get("description")), "enabled": !truthy(p.get("disabled")),
                        })
                    })
                    .collect(),
            );
        }
        "raw" => {
            let language = non_empty(source.pointer("/options/raw/language"))
                .map(str::to_owned)
                .or_else(|| language_of_headers(headers).map(str::to_owned));
            let raw = source.get("raw").cloned().unwrap_or(Value::Null);
            match language.as_deref() {
                Some("json") => {
                    body["mode"] = json!("json");
                    body["json"] = raw;
                }
                Some("xml") => {
                    body["mode"] = json!("xml");
                    body["xml"] = raw;
                }
                _ => {
                    body["mode"] = json!("text");
                    body["text"] = raw;
                }
            }
        }
        "file" => {
            body["mode"] = json!("file");
            let path = ensure(source.pointer("/file/src"));
            body["file"] = json!([{ "selected": true, "filePath": path, "contentType": inferred_content_type(&path) }]);
        }
        _ => {}
    }
}

fn graphql_body(source: Option<&Value>) -> Value {
    let parsed = match source {
        Some(Value::String(s)) => serde_json::from_str::<Value>(s).ok(),
        other => other.cloned(),
    };
    let field = |key: &str| parsed.as_ref().and_then(|p| p.get(key)).filter(|v| v.as_str() != Some("")).cloned();
    json!({ "query": field("query").unwrap_or_else(|| json!("")), "variables": field("variables").unwrap_or_else(|| json!("")) })
}

// ---------------------------------------------------------------------------------------------------------------
// Éléments

fn items(list: Option<&Value>, parent_path: &str, issues: &mut Vec<Issue>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut folder_names: Vec<String> = Vec::new();
    let mut request_names: Vec<String> = Vec::new();
    for (index, item) in list.and_then(Value::as_array).into_iter().flatten().enumerate() {
        let path_of =
            |name: &str| if parent_path.is_empty() { name.to_owned() } else { format!("{parent_path} / {name}") };
        if !item.is_object() {
            issues.push(Issue {
                path: path_of(&format!("Item {}", index + 1)),
                severity: "error",
                message: "Malformed collection item (not an object)".into(),
            });
            continue;
        }
        let item_name = non_empty(item.get("name")).map_or_else(|| format!("Item {}", index + 1), str::to_owned);
        let item_path = path_of(&item_name);
        let is_folder = item.get("request").is_none_or(|r| !truthy(Some(r)));
        if is_folder {
            let name = unique(non_empty(item.get("name")).unwrap_or("Untitled Folder"), &mut folder_names);
            out.push(folder(item, &name, index, &item_path, issues));
        } else if let Some(request) = request(item, &mut request_names, index, &item_path, issues) {
            out.push(request);
        }
    }
    out
}

/// Un nom pas encore pris dans le dossier : `Nom`, puis `Nom_1`, `Nom_2`…
fn unique(base: &str, taken: &mut Vec<String>) -> String {
    let mut name = base.to_owned();
    let mut count = 1;
    while taken.contains(&name) {
        name = format!("{base}_{count}");
        count += 1;
    }
    taken.push(name.clone());
    name
}

fn folder(item: &Value, name: &str, index: usize, path: &str, issues: &mut Vec<Issue>) -> Value {
    let mut request = json!({
        "auth": empty_auth("inherit"),
        "headers": [],
        "script": {},
        "tests": "",
        "vars": {},
    });
    process_auth(item.get("auth"), &mut request);
    if let Some(events) = item.get("event") {
        import_scripts(events, &mut request);
    }
    json!({
        "name": name,
        "type": "folder",
        "items": items(item.get("item"), path, issues),
        "seq": index + 1,
        "root": {
            "docs": description(item.get("description")),
            "meta": { "name": name },
            "request": request,
        },
    })
}

fn request(item: &Value, taken: &mut Vec<String>, index: usize, path: &str, issues: &mut Vec<Issue>) -> Option<Value> {
    let source = &item["request"];
    let Some(raw_method) = source.get("method").and_then(Value::as_str).filter(|m| !m.trim().is_empty()) else {
        issues.push(Issue {
            path: path.to_owned(),
            severity: "error",
            message: "Missing or invalid request method".into(),
        });
        return None;
    };
    let name = unique(non_empty(item.get("name")).unwrap_or("Untitled Request"), taken);

    let mut request = json!({
        "url": construct_url(source.get("url")),
        "method": raw_method.to_uppercase(),
        "auth": empty_auth("inherit"),
        "headers": [],
        "params": [],
        "body": empty_body(),
        "docs": description(source.get("description")),
    });
    let mut kind = "http-request";

    let behavior = item.get("protocolProfileBehavior");
    let mut settings = json!({
        "encodeUrl": behavior.and_then(|b| b.get("disableUrlEncoding")) != Some(&Value::Bool(true)),
        "forwardAuthorizationHeader": behavior.and_then(|b| b.get("followAuthorizationHeader")).filter(|v| !v.is_null()).cloned().unwrap_or(Value::Bool(false)),
    });
    if let Some(follow) = behavior.and_then(|b| b.get("followRedirects")) {
        settings["followRedirects"] = follow.clone();
    }
    match behavior.and_then(|b| b.get("maxRedirects")) {
        Some(Value::Number(n)) if n.as_f64().is_some_and(|f| f.is_finite() && f >= 0.0) => {
            settings["maxRedirects"] = json!(n.as_f64().map_or(0.0, f64::trunc));
        }
        Some(_) => issues.push(Issue {
            path: path.to_owned(),
            severity: "warning",
            message: "Invalid maxRedirects, ignored (must be a number of 0 or more)".into(),
        }),
        None => {}
    }
    if let Some(Value::Object(disabled)) = behavior.and_then(|b| b.get("disabledSystemHeaders")) {
        let omit: Vec<&str> = disabled
            .iter()
            .filter(|(_, on)| **on == Value::Bool(true))
            .map(|(name, _)| name.trim())
            .filter(|n| !n.is_empty())
            .collect();
        if !omit.is_empty() {
            settings["omitHeaders"] = json!(omit);
        }
    }

    if let Some(events) = item.get("event") {
        import_scripts(events, &mut request);
    }

    import_body(source.get("body").unwrap_or(&Value::Null), source.get("header"), &mut request["body"]);
    if source.pointer("/body/mode").and_then(Value::as_str) == Some("graphql") {
        kind = "graphql-request";
        request["body"]["mode"] = json!("graphql");
        request["body"]["graphql"] = graphql_body(source.pointer("/body/graphql"));
    }

    request["headers"] =
        Value::Array(normalize_headers(source.get("header")).iter().filter_map(header_entry).collect());
    process_auth(source.get("auth"), &mut request);

    let mut params: Vec<Value> = source
        .pointer("/url/query")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| param_entry(p, "query"))
        .collect();
    params
        .extend(source.pointer("/url/variable").and_then(Value::as_array).into_iter().flatten().filter_map(path_param));
    request["params"] = Value::Array(params);

    let mut converted =
        json!({ "name": name, "type": kind, "seq": index + 1, "request": request, "settings": settings });
    if let Some(responses) = item.get("response").and_then(Value::as_array) {
        converted["examples"] =
            Value::Array(responses.iter().enumerate().map(|(i, r)| example(r, i, raw_method)).collect());
    }
    Some(converted)
}

/// Une réponse enregistrée de Postman devient un exemple Bruno.
fn example(response: &Value, index: usize, method: &str) -> Value {
    let name = ensure(response.get("name")).replace(['\r', '\n'], " ");
    let name = if name.trim().is_empty() { format!("Example {}", index + 1) } else { name.trim().to_owned() };
    let original = response.get("originalRequest").unwrap_or(&Value::Null);
    let mut request = json!({
        "url": construct_url(original.get("url")),
        "method": original.get("method").and_then(Value::as_str).map_or_else(|| method.to_uppercase(), str::to_uppercase),
        "headers": normalize_headers(original.get("header")).iter().filter_map(header_entry).collect::<Vec<_>>(),
        "params": [],
        "body": empty_body(),
    });
    let mut params: Vec<Value> = original
        .pointer("/url/query")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| param_entry(p, "query"))
        .collect();
    params.extend(
        original.pointer("/url/variable").and_then(Value::as_array).into_iter().flatten().filter_map(path_param),
    );
    request["params"] = Value::Array(params);
    if let Some(body) = original.get("body").filter(|b| !b.is_null()) {
        import_body(body, original.get("header"), &mut request["body"]);
    }
    let headers: Vec<Value> = normalize_headers(response.get("header"))
        .iter()
        .filter_map(|h| {
            if !h.is_object() || is_blank(h) {
                return None;
            }
            Some(json!({ "name": ensure(h.get("key")), "value": ensure(h.get("value")), "description": description(h.get("description")), "enabled": true }))
        })
        .collect();
    json!({
        "name": name,
        "description": "",
        "type": "http-request",
        "request": request,
        "response": {
            "status": response.get("code").cloned().filter(|c| truthy(Some(c))).unwrap_or(Value::Null),
            "statusText": ensure(response.get("status")),
            "headers": headers,
            "body": { "type": body_type(response.get("header")), "content": response.get("body").cloned().filter(|b| truthy(Some(b))).unwrap_or_else(|| json!("")) },
        },
    })
}

fn body_type(headers: Option<&Value>) -> &'static str {
    let found = normalize_headers(headers)
        .into_iter()
        .find(|h| h.get("key").and_then(Value::as_str).is_some_and(|k| k.eq_ignore_ascii_case("content-type")));
    let content_type =
        found.as_ref().and_then(|h| h.get("value")).and_then(Value::as_str).map(str::to_lowercase).unwrap_or_default();
    if content_type.contains("application/json") {
        "json"
    } else if content_type.contains("application/xml") || content_type.contains("text/xml") {
        "xml"
    } else if content_type.contains("text/html") {
        "html"
    } else {
        "text"
    }
}
