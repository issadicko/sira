//! Sens des blocs `.bru` : port des actions sémantiques de `bruToJson.js` et `collectionBruToJson.js` (bruno-lang v2),
//! qui donnent un JSON intermédiaire, puis de `parseBruRequest` et `parseBruCollection` (bruno-filestore), qui en font
//! l'élément de collection que le sérialiseur YAML sait écrire.

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Map, Value as Json};

use super::example;
use super::scanner::{self, Annotation, Block, Content, Kind, Pair, ParseError, Value};
use crate::js::{number_value, string_to_number, truthy};

const OAUTH2_EXTRAS: [&str; 8] = [
    "auth_req:headers",
    "auth_req:queryparams",
    "access_token_req:headers",
    "access_token_req:queryparams",
    "access_token_req:body",
    "refresh_token_req:headers",
    "refresh_token_req:queryparams",
    "refresh_token_req:body",
];

/// Le contenu de chaque nom de bloc connu d'un fichier de requête, de dossier ou de collection.
pub fn kind(name: &str) -> Option<Kind> {
    Some(match name {
        "meta"
        | "app"
        | "settings"
        | "grpc"
        | "ws"
        | "get"
        | "post"
        | "put"
        | "delete"
        | "patch"
        | "options"
        | "head"
        | "connect"
        | "trace"
        | "http"
        | "headers"
        | "metadata"
        | "query"
        | "params:path"
        | "params:query"
        | "vars:pre-request"
        | "vars:post-response"
        | "auth"
        | "auth:awsv4"
        | "auth:basic"
        | "auth:bearer"
        | "auth:digest"
        | "auth:ntlm"
        | "auth:oauth1"
        | "auth:oauth2"
        | "auth:wsse"
        | "auth:apikey"
        | "auth:akamai-edgegrid"
        | "body:form-urlencoded"
        | "body:multipart-form"
        | "body:file"
        | "body:grpc"
        | "body:ws" => Kind::Pairs,
        "assert" => Kind::Assert,
        "body"
        | "body:json"
        | "body:text"
        | "body:xml"
        | "body:sparql"
        | "body:graphql"
        | "body:graphql:vars"
        | "script:pre-request"
        | "script:post-response"
        | "script:grpc:before-call-start"
        | "script:grpc:before-message-send"
        | "script:grpc:after-message-receive"
        | "script:grpc:after-call-end"
        | "tests"
        | "docs" => Kind::Text,
        "example" => Kind::Raw,
        _ if name.strip_prefix("auth:oauth2:additional_params:").is_some_and(|rest| OAUTH2_EXTRAS.contains(&rest)) => {
            Kind::Pairs
        }
        _ => return None,
    })
}

/// Lit un fichier et fusionne ses blocs comme `_.mergeWith(…, concatArrays)` : objets fusionnés, tableaux concaténés.
pub fn read(text: &str) -> Result<(Json, Vec<(usize, String)>), ParseError> {
    let parsed = scanner::parse(text, kind)?;
    let mut merged = Json::Object(Map::new());
    for block in &parsed.blocks {
        if let Some(part) = partial(block) {
            merge(&mut merged, part);
        }
    }
    Ok((merged, parsed.skipped))
}

fn merge(into: &mut Json, from: Json) {
    match (into, from) {
        (Json::Object(a), Json::Object(b)) => {
            for (key, value) in b {
                match a.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        a.insert(key, value);
                    }
                }
            }
        }
        (Json::Array(a), Json::Array(b)) => a.extend(b),
        (slot, value) => *slot = value,
    }
}

fn partial(block: &Block) -> Option<Json> {
    let name = block.name.as_str();
    match &block.content {
        Content::Pairs(pairs) => pairs_block(name, pairs),
        Content::Text(text) => text_block(name, text),
        Content::Raw(raw) => Some(json!({ "examples": [raw] })),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Paires

fn value_json(v: &Value) -> Json {
    match v {
        Value::Text(s) => json!(s),
        Value::List(items) => json!(items),
    }
}

/// `mapPairListToKeyValPair` : les paires fusionnées en un objet, la dernière clé répétée l'emporte.
fn dict(pairs: &[Pair]) -> Map<String, Json> {
    let mut out = Map::new();
    for pair in pairs {
        out.insert(pair.key.clone(), value_json(&pair.value));
    }
    out
}

fn text_field<'a>(pairs: &'a [Pair], name: &str) -> Option<&'a str> {
    pairs.iter().find(|p| p.key == name).and_then(|p| p.value.text())
}

/// `_.find(auth, { name }).value`, ou `''`.
fn field(pairs: &[Pair], name: &str) -> String {
    text_field(pairs, name).unwrap_or_default().to_owned()
}

fn field_or(pairs: &[Pair], name: &str, default: &str) -> String {
    text_field(pairs, name).filter(|v| !v.is_empty()).unwrap_or(default).to_owned()
}

/// `safeParseJson(value) ?? default`.
fn json_field(pairs: &[Pair], name: &str, default: Json) -> Json {
    match pairs.iter().find(|p| p.key == name) {
        Some(pair) => pair
            .value
            .text()
            .and_then(|t| serde_json::from_str::<Json>(t).ok())
            .filter(|v| !v.is_null())
            .unwrap_or(default),
        None => default,
    }
}

const DATA_TYPES: [&str; 4] = ["string", "number", "boolean", "object"];

fn description(annotations: &[Annotation], item: &mut Map<String, Json>) {
    let Some(annotation) = annotations.iter().find(|a| a.name == "description") else { return };
    let value = annotation.value.clone().unwrap_or_default();
    let value = if value.contains('\n') { value.replace(r"\'\'\'", "'''").replace(r"\\'", r"\'") } else { value };
    item.insert("description".into(), json!(value));
}

/// `extractTypedAnnotations` : `@number`, `@boolean` ou `@object` donnent un `dataType` et une valeur typée.
fn data_type(annotations: &[Annotation], item: &mut Map<String, Json>) {
    let Some(annotation) = annotations.iter().rev().find(|a| DATA_TYPES.contains(&a.name.as_str())) else { return };
    if annotation.name == "string" {
        return;
    }
    item.insert("dataType".into(), json!(annotation.name));
    let Some(Json::String(raw)) = item.get("value").cloned() else { return };
    let trimmed = crate::js::trim(&raw);
    let typed = match annotation.name.as_str() {
        "number" if !trimmed.is_empty() => {
            let n = string_to_number(trimmed);
            (!n.is_nan()).then(|| number_value(n))
        }
        "boolean" => match trimmed {
            _ if raw == "true" => Some(json!(true)),
            _ if raw == "false" => Some(json!(false)),
            _ => None,
        },
        "object" if !trimmed.is_empty() => {
            serde_json::from_str::<Json>(trimmed).ok().filter(|v| v.is_object() || v.is_array())
        }
        _ => None,
    };
    if let Some(typed) = typed {
        item.insert("value".into(), typed);
    }
}

/// `mapPairListToKeyValPairs`.
pub(super) fn kv(pairs: &[Pair], parse_enabled: bool, extract_types: bool) -> Vec<Json> {
    pairs
        .iter()
        .map(|pair| {
            let mut name = pair.key.clone();
            let mut item = Map::new();
            let mut enabled = true;
            if parse_enabled && name.starts_with('~') {
                name.remove(0);
                enabled = false;
            }
            item.insert("name".into(), json!(name));
            item.insert("value".into(), value_json(&pair.value));
            if parse_enabled {
                item.insert("enabled".into(), json!(enabled));
                description(&pair.annotations, &mut item);
            }
            if extract_types {
                data_type(&pair.annotations, &mut item);
            }
            Json::Object(item)
        })
        .collect()
}

/// `mapRequestParams`.
pub(super) fn params(pairs: &[Pair], kind: &str) -> Json {
    let list = kv(pairs, true, false).into_iter().map(|mut p| {
        p["type"] = json!(kind);
        p
    });
    json!({ "params": list.collect::<Vec<_>>() })
}

fn content_type_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?s)^(.*?)\s*@contentType\((.*?)\)\s*$").expect("motif contentType"))
}

pub(super) fn multipart(pairs: &[Pair]) -> Vec<Json> {
    kv(pairs, true, false)
        .into_iter()
        .map(|mut pair| {
            pair["type"] = json!("text");
            if let Some(value) = pair["value"].as_str().map(str::to_owned) {
                match content_type_pattern().captures(&value) {
                    Some(m) => {
                        pair["value"] = json!(&m[1]);
                        pair["contentType"] = json!(&m[2]);
                    }
                    None => pair["contentType"] = json!(""),
                }
            }
            if let Some(value) = pair["value"].as_str().map(str::to_owned) {
                if let Some(files) = value.strip_prefix("@file(").and_then(|v| v.strip_suffix(')')) {
                    pair["type"] = json!("file");
                    pair["value"] = json!(files.split('|').filter(|f| !f.is_empty()).collect::<Vec<_>>());
                }
            }
            pair
        })
        .collect()
}

pub(super) fn file_body(pairs: &[Pair]) -> Vec<Json> {
    kv(pairs, true, false)
        .into_iter()
        .map(|mut pair| {
            if let Some(value) = pair["value"].as_str().map(str::to_owned) {
                match content_type_pattern().captures(&value) {
                    Some(m) => {
                        pair["value"] = json!(crate::js::trim(&m[1]));
                        pair["contentType"] = json!(crate::js::trim(&m[2]));
                    }
                    None => pair["contentType"] = json!(""),
                }
            }
            let path =
                pair["value"].as_str().and_then(|v| v.strip_prefix("@file(")?.strip_suffix(')')).map(str::to_owned);
            if let Some(path) = path {
                let enabled = pair["enabled"].clone();
                pair["filePath"] = json!(path);
                pair["selected"] = enabled;
                let map = pair.as_object_mut().expect("objet");
                map.remove("value");
                map.remove("name");
                map.remove("enabled");
            }
            pair
        })
        .collect()
}

fn vars(pairs: &[Pair], extract_types: bool) -> Vec<Json> {
    let mut list = kv(pairs, true, extract_types);
    for var in &mut list {
        let name = var["name"].as_str().unwrap_or_default().to_owned();
        match name.strip_prefix('@') {
            Some(stripped) => {
                var["name"] = json!(stripped);
                var["local"] = json!(true);
            }
            None => var["local"] = json!(false),
        }
    }
    list
}

fn to_bool(v: Option<&Json>) -> bool {
    match v {
        Some(Json::Bool(b)) => *b,
        Some(Json::String(s)) => s == "true",
        _ => false,
    }
}

/// `parseInt(text, 10)`.
fn parse_int(text: &str) -> Option<i64> {
    let t = crate::js::trim(text);
    let (sign, digits) = match t.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, t.strip_prefix('+').unwrap_or(t)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    digits[..end].parse::<i64>().ok().map(|n| sign * n)
}

fn settings(pairs: &[Pair]) -> Json {
    let s = dict(pairs);
    let mut out = Map::new();
    let timeout = match s.get("timeout") {
        Some(Json::String(t)) if t == "inherit" => Some(json!("inherit")),
        Some(Json::String(t)) => parse_int(t).map(|n| json!(n)),
        _ => None,
    };
    out.insert("encodeUrl".into(), json!(to_bool(s.get("encodeUrl"))));
    out.insert("timeout".into(), timeout.unwrap_or(json!(0)));
    if s.contains_key("followRedirects") {
        out.insert("followRedirects".into(), json!(to_bool(s.get("followRedirects"))));
    }
    if truthy(s.get("maxRedirects")) {
        let n = string_to_number(s["maxRedirects"].as_str().unwrap_or_default());
        if n.is_finite() && n >= 0.0 {
            out.insert("maxRedirects".into(), number_value(n.trunc()));
        }
    }
    if s.contains_key("forwardAuthorizationHeader") {
        out.insert("forwardAuthorizationHeader".into(), json!(to_bool(s.get("forwardAuthorizationHeader"))));
    }
    if let Some(keep_alive) = s.get("keepAliveInterval").and_then(Json::as_str).map(string_to_number) {
        if keep_alive.is_finite() && keep_alive != 0.0 {
            out.insert("keepAliveInterval".into(), number_value(keep_alive));
        }
    }
    if let Some(Json::Array(names)) = s.get("omitHeaders") {
        let names: Vec<_> = names
            .iter()
            .map(|n| crate::js::trim(n.as_str().unwrap_or_default()).to_owned())
            .filter(|n| !n.is_empty())
            .collect();
        if !names.is_empty() {
            out.insert("omitHeaders".into(), json!(names));
        }
    }
    json!({ "settings": out })
}

fn pairs_block(name: &str, pairs: &[Pair]) -> Option<Json> {
    Some(match name {
        "meta" => {
            let mut meta = dict(pairs);
            if !truthy(meta.get("seq")) {
                meta.insert("seq".into(), json!(1));
            }
            if !truthy(meta.get("type")) {
                meta.insert("type".into(), json!("http"));
            }
            json!({ "meta": meta })
        }
        "app" => {
            let app = dict(pairs);
            json!({ "app": { "code": app.get("code").filter(|c| truthy(*c)).cloned().unwrap_or(Json::Null), "enabled": to_bool(app.get("enabled")) } })
        }
        "settings" => settings(pairs),
        "grpc" => json!({ "grpc": dict(pairs) }),
        "ws" => json!({ "ws": dict(pairs) }),
        "get" | "post" | "put" | "delete" | "patch" | "options" | "head" | "connect" | "trace" => {
            let mut http = Map::new();
            http.insert("method".into(), json!(name));
            http.extend(dict(pairs));
            json!({ "http": http })
        }
        "http" => json!({ "http": dict(pairs) }),
        "headers" => json!({ "headers": kv(pairs, true, false) }),
        "metadata" => json!({ "metadata": kv(pairs, true, false) }),
        "query" | "params:query" => params(pairs, "query"),
        "params:path" => params(pairs, "path"),
        "auth" => json!({ "auth": { "mode": pairs_mode(pairs) } }),
        "auth:awsv4" => json!({ "auth": { "awsv4": strings(pairs, &[
            ("accessKeyId", "accessKeyId"), ("secretAccessKey", "secretAccessKey"), ("sessionToken", "sessionToken"),
            ("service", "service"), ("region", "region"), ("profileName", "profileName"),
        ]) } }),
        "auth:basic" => {
            json!({ "auth": { "basic": strings(pairs, &[("username", "username"), ("password", "password")]) } })
        }
        "auth:bearer" => json!({ "auth": { "bearer": strings(pairs, &[("token", "token")]) } }),
        "auth:digest" => {
            json!({ "auth": { "digest": strings(pairs, &[("username", "username"), ("password", "password")]) } })
        }
        "auth:ntlm" => {
            json!({ "auth": { "ntlm": strings(pairs, &[("username", "username"), ("password", "password"), ("domain", "domain")]) } })
        }
        "auth:wsse" => {
            json!({ "auth": { "wsse": strings(pairs, &[("username", "username"), ("password", "password")]) } })
        }
        "auth:apikey" => {
            json!({ "auth": { "apikey": strings(pairs, &[("key", "key"), ("value", "value"), ("placement", "placement")]) } })
        }
        "auth:oauth1" => json!({ "auth": { "oauth1": oauth1(pairs) } }),
        "auth:oauth2" => json!({ "auth": { "oauth2": oauth2(pairs) } }),
        "auth:akamai-edgegrid" => {
            let size = field(pairs, "maxBodySize");
            let max = if size.is_empty() || string_to_number(&size).is_nan() {
                Json::Null
            } else {
                number_value(string_to_number(&size))
            };
            let mut edgegrid = strings(
                pairs,
                &[
                    ("accessToken", "accessToken"),
                    ("clientToken", "clientToken"),
                    ("clientSecret", "clientSecret"),
                    ("nonce", "nonce"),
                    ("timestamp", "timestamp"),
                    ("baseURL", "baseURL"),
                    ("headersToSign", "headersToSign"),
                ],
            );
            edgegrid["maxBodySize"] = max;
            json!({ "auth": { "akamaiEdgegrid": edgegrid } })
        }
        "body:form-urlencoded" => json!({ "body": { "formUrlEncoded": kv(pairs, true, false) } }),
        "body:multipart-form" => json!({ "body": { "multipartForm": multipart(pairs) } }),
        "body:file" => json!({ "body": { "file": file_body(pairs) } }),
        "vars:pre-request" => json!({ "vars": { "req": vars(pairs, true) } }),
        "vars:post-response" => json!({ "vars": { "res": vars(pairs, false) } }),
        "assert" => json!({ "assertions": kv(pairs, true, false) }),
        "body:grpc" | "body:ws" => return None,
        other => {
            let key = other.strip_prefix("auth:oauth2:additional_params:")?;
            let (stage, place) = key.split_once(':')?;
            let stage = if stage == "auth_req" {
                "auth_req"
            } else if stage == "access_token_req" {
                "access_token_req"
            } else {
                "refresh_token_req"
            };
            let place = if place == "body" { "bodyvalues" } else { place };
            json!({ format!("oauth2_additional_parameters_{stage}_{place}"): kv(pairs, true, false) })
        }
    })
}

fn pairs_mode(pairs: &[Pair]) -> String {
    field_or(pairs, "mode", "none")
}

/// Un objet de chaînes dont chaque clé est cherchée par son nom dans le bloc (vide si elle manque).
fn strings(pairs: &[Pair], names: &[(&str, &str)]) -> Json {
    let mut out = Map::new();
    for (key, source) in names {
        out.insert((*key).to_owned(), json!(field(pairs, source)));
    }
    Json::Object(out)
}

fn oauth1(pairs: &[Pair]) -> Json {
    let private_key = field(pairs, "private_key");
    let from_file = private_key.starts_with("@file(") && private_key.ends_with(')');
    json!({
        "consumerKey": field(pairs, "consumer_key"),
        "consumerSecret": field(pairs, "consumer_secret"),
        "accessToken": field(pairs, "access_token"),
        "accessTokenSecret": field(pairs, "token_secret"),
        "callbackUrl": field(pairs, "callback_url"),
        "verifier": field(pairs, "verifier"),
        "signatureMethod": field(pairs, "signature_method"),
        "privateKey": if from_file { private_key[6..private_key.len() - 1].to_owned() } else { private_key.clone() },
        "privateKeyType": if !private_key.is_empty() && from_file { "file" } else { "text" },
        "timestamp": field(pairs, "timestamp"),
        "nonce": field(pairs, "nonce"),
        "version": field(pairs, "version"),
        "realm": field(pairs, "realm"),
        "placement": field(pairs, "placement"),
        "includeBodyHash": field(pairs, "include_body_hash") == "true",
    })
}

fn oauth2(pairs: &[Pair]) -> Json {
    let grant = field(pairs, "grant_type");
    let mut config = Map::new();
    let mut put = |key: &str, value: Json| {
        config.insert(key.to_owned(), value);
    };
    let common = |put: &mut dyn FnMut(&str, Json)| {
        put("credentialsPlacement", json!(field_or(pairs, "credentials_placement", "body")));
        put("credentialsId", json!(field_or(pairs, "credentials_id", "credentials")));
        put("tokenSource", json!(field_or(pairs, "token_source", "access_token")));
        put("tokenPlacement", json!(field_or(pairs, "token_placement", "header")));
        put("tokenHeaderPrefix", json!(field_or(pairs, "token_header_prefix", "")));
        put("tokenQueryKey", json!(field_or(pairs, "token_query_key", "access_token")));
        put("autoFetchToken", json_field(pairs, "auto_fetch_token", json!(true)));
        put("autoRefreshToken", json_field(pairs, "auto_refresh_token", json!(false)));
    };
    match grant.as_str() {
        "password" => {
            put("grantType", json!(grant));
            put("accessTokenUrl", json!(field(pairs, "access_token_url")));
            put("refreshTokenUrl", json!(field(pairs, "refresh_token_url")));
            put("username", json!(field(pairs, "username")));
            put("password", json!(field(pairs, "password")));
            put("clientId", json!(field(pairs, "client_id")));
            put("clientSecret", json!(field(pairs, "client_secret")));
            put("scope", json!(field(pairs, "scope")));
            common(&mut put);
        }
        "authorization_code" => {
            put("grantType", json!(grant));
            put("callbackUrl", json!(field(pairs, "callback_url")));
            put("authorizationUrl", json!(field(pairs, "authorization_url")));
            put("accessTokenUrl", json!(field(pairs, "access_token_url")));
            put("refreshTokenUrl", json!(field(pairs, "refresh_token_url")));
            put("clientId", json!(field(pairs, "client_id")));
            put("clientSecret", json!(field(pairs, "client_secret")));
            put("scope", json!(field(pairs, "scope")));
            put("state", json!(field(pairs, "state")));
            put("pkce", json_field(pairs, "pkce", json!(false)));
            common(&mut put);
        }
        "client_credentials" => {
            put("grantType", json!(grant));
            put("accessTokenUrl", json!(field(pairs, "access_token_url")));
            put("refreshTokenUrl", json!(field(pairs, "refresh_token_url")));
            put("clientId", json!(field(pairs, "client_id")));
            put("clientSecret", json!(field(pairs, "client_secret")));
            put("scope", json!(field(pairs, "scope")));
            common(&mut put);
        }
        "implicit" => {
            put("grantType", json!(grant));
            put("callbackUrl", json!(field(pairs, "callback_url")));
            put("authorizationUrl", json!(field(pairs, "authorization_url")));
            put("clientId", json!(field(pairs, "client_id")));
            put("scope", json!(field(pairs, "scope")));
            put("state", json!(field(pairs, "state")));
            put("credentialsId", json!(field_or(pairs, "credentials_id", "credentials")));
            put("tokenSource", json!(field_or(pairs, "token_source", "access_token")));
            put("tokenPlacement", json!(field_or(pairs, "token_placement", "header")));
            put("tokenHeaderPrefix", json!(field_or(pairs, "token_header_prefix", "")));
            put("tokenQueryKey", json!(field_or(pairs, "token_query_key", "access_token")));
            put("autoFetchToken", json_field(pairs, "auto_fetch_token", json!(true)));
        }
        _ => {}
    }
    Json::Object(config)
}

// ---------------------------------------------------------------------------------------------------------------
// Blocs de texte

fn text_block(name: &str, text: &str) -> Option<Json> {
    let text = json!(text);
    Some(match name {
        "body" => json!({ "http": { "body": "json" }, "body": { "json": text } }),
        "body:json" => json!({ "body": { "json": text } }),
        "body:text" => json!({ "body": { "text": text } }),
        "body:xml" => json!({ "body": { "xml": text } }),
        "body:sparql" => json!({ "body": { "sparql": text } }),
        "body:graphql" => json!({ "body": { "graphql": { "query": text } } }),
        "body:graphql:vars" => json!({ "body": { "graphql": { "variables": text } } }),
        "script:pre-request" => json!({ "script": { "req": text } }),
        "script:post-response" => json!({ "script": { "res": text } }),
        "tests" => json!({ "tests": text }),
        "docs" => json!({ "docs": text }),
        _ => return None,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Éléments de collection

/// Pourquoi une requête n'a pas été convertie.
#[derive(Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// `grpc`, `ws` ou `app` : Sira n'ouvre que les requêtes HTTP et GraphQL.
    Type(String),
}

/// `parseBruRequest` : l'élément de collection (`http-request` ou `graphql-request`) d'un fichier lu, et ce qui a été
/// écarté en route (un exemple illisible).
pub fn request(json: &Json) -> Result<(Json, Vec<String>), Unsupported> {
    let meta_type = json.pointer("/meta/type").and_then(Json::as_str).unwrap_or("http");
    let kind = match meta_type {
        "http" => "http-request",
        "graphql" => "graphql-request",
        "grpc" | "ws" | "app" => return Err(Unsupported::Type(meta_type.to_owned())),
        _ => "http-request",
    };
    let seq = match json.pointer("/meta/seq") {
        Some(Json::Number(n)) => number_value(n.as_f64().unwrap_or(1.0)),
        Some(Json::String(s)) => number_value(string_to_number(s)),
        _ => json!(1),
    };
    let tags = json.pointer("/meta/tags").filter(|t| t.is_array()).cloned().unwrap_or_else(|| json!([]));

    let mut auth = json.get("auth").cloned().filter(Json::is_object).unwrap_or_else(|| json!({}));
    auth["mode"] = json!(json.pointer("/http/auth").and_then(Json::as_str).unwrap_or("none"));
    let mut body = json.get("body").cloned().filter(Json::is_object).unwrap_or_else(|| json!({}));
    body["mode"] = json!(json.pointer("/http/body").and_then(Json::as_str).unwrap_or("none"));

    if let Some(grant) = json.pointer("/auth/oauth2/grantType").filter(|g| truthy(*g)).and_then(Json::as_str) {
        let extra = oauth2_additional_parameters(json, grant);
        if extra.as_object().is_some_and(|o| !o.is_empty()) {
            auth["oauth2"]["additionalParameters"] = extra;
        }
    }

    let app = json.get("app").map(|a| json!({ "code": a.get("code").cloned().unwrap_or(Json::Null), "enabled": a.get("enabled") == Some(&json!(true)) }));
    let method = json.pointer("/http/method").and_then(Json::as_str).unwrap_or_default().to_uppercase();
    let parent_method = json.pointer("/http/method").and_then(Json::as_str).unwrap_or("GET");
    let mut warnings = Vec::new();
    let mut examples = Vec::new();
    for (index, raw) in
        json.get("examples").and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str).enumerate()
    {
        match example::parse(raw, parent_method) {
            Ok(example) => examples.push(example),
            Err(message) => warnings.push(format!("Exemple n°{} ignoré : {message}", index + 1)),
        }
    }
    let item = json!({
        "type": kind,
        "name": json.pointer("/meta/name").cloned().unwrap_or(Json::Null),
        "seq": seq,
        "settings": json.get("settings").cloned().unwrap_or_else(|| json!({})),
        "app": app.unwrap_or(Json::Null),
        "tags": tags,
        "request": {
            "method": method,
            "url": json.pointer("/http/url").cloned().unwrap_or(Json::Null),
            "headers": json.get("headers").cloned().unwrap_or_else(|| json!([])),
            "auth": auth,
            "body": body,
            "script": json.get("script").cloned().unwrap_or_else(|| json!({})),
            "vars": json.get("vars").cloned().unwrap_or_else(|| json!({})),
            "assertions": json.get("assertions").cloned().unwrap_or_else(|| json!([])),
            "tests": json.get("tests").cloned().unwrap_or_else(|| json!("")),
            "docs": json.get("docs").cloned().unwrap_or_else(|| json!("")),
            "params": json.get("params").cloned().unwrap_or_else(|| json!([])),
        },
        "examples": examples,
    });
    Ok((item, warnings))
}

/// `getOauth2AdditionalParameters` : paramètres d'ajout aux trois requêtes d'un flux OAuth 2, selon le type de flux.
fn oauth2_additional_parameters(json: &Json, grant: &str) -> Json {
    let sources: [(&str, &[(&str, &str)]); 3] = [
        ("authorization", &[("headers", "auth_req_headers"), ("queryparams", "auth_req_queryparams")]),
        (
            "token",
            &[
                ("headers", "access_token_req_headers"),
                ("queryparams", "access_token_req_queryparams"),
                ("body", "access_token_req_bodyvalues"),
            ],
        ),
        (
            "refresh",
            &[
                ("headers", "refresh_token_req_headers"),
                ("queryparams", "refresh_token_req_queryparams"),
                ("body", "refresh_token_req_bodyvalues"),
            ],
        ),
    ];
    let mut out = Map::new();
    for (stage, places) in sources {
        let included = match stage {
            "authorization" => matches!(grant, "authorization_code" | "implicit"),
            _ => grant != "implicit",
        };
        if !included {
            continue;
        }
        let mut list = Vec::new();
        for (send_in, key) in places {
            for param in
                json.get(format!("oauth2_additional_parameters_{key}")).and_then(Json::as_array).into_iter().flatten()
            {
                let mut param = param.clone();
                param["sendIn"] = json!(send_in);
                list.push(param);
            }
        }
        if !list.is_empty() {
            out.insert(stage.to_owned(), Json::Array(list));
        }
    }
    Json::Object(out)
}

/// `parseBruCollection` : la racine (collection ou dossier) d'un `collection.bru` ou d'un `folder.bru`.
pub fn collection_root(json: &Json) -> Json {
    let mut auth = json.get("auth").cloned().filter(Json::is_object).unwrap_or_else(|| json!({}));
    if let Some(grant) = json.pointer("/auth/oauth2/grantType").filter(|g| truthy(*g)).and_then(Json::as_str) {
        let extra = oauth2_additional_parameters(json, grant);
        if extra.as_object().is_some_and(|o| !o.is_empty()) {
            auth["oauth2"]["additionalParameters"] = extra;
        }
    }
    let mut root = json!({
        "request": {
            "headers": json.get("headers").cloned().unwrap_or_else(|| json!([])),
            "auth": auth,
            "script": json.get("script").cloned().unwrap_or_else(|| json!({})),
            "vars": json.get("vars").cloned().unwrap_or_else(|| json!({})),
            "tests": json.get("tests").cloned().unwrap_or_else(|| json!("")),
        },
        "settings": json.get("settings").cloned().unwrap_or_else(|| json!({})),
        "docs": json.get("docs").cloned().unwrap_or_else(|| json!("")),
    });
    if let Some(meta) = json.get("meta").filter(|m| m.is_object()) {
        let mut out = json!({ "name": meta.get("name").cloned().unwrap_or(Json::Null) });
        if let Some(seq) = meta.get("seq") {
            out["seq"] = match seq {
                Json::String(s) if !string_to_number(s).is_nan() => number_value(string_to_number(s)),
                Json::Number(_) => seq.clone(),
                _ => json!(1),
            };
        }
        if let Some(tags) = meta.get("tags").filter(|t| t.as_array().is_some_and(|l| !l.is_empty())) {
            out["tags"] = tags.clone();
        }
        root["meta"] = out;
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(text: &str) -> Json {
        let (json, _) = read(text).unwrap();
        request(&json).unwrap().0
    }

    #[test]
    fn a_request_file_becomes_a_bruno_collection_item() {
        let item = item(
            "meta {\n  name: List users\n  type: http\n  seq: 3\n  tags: [\n    smoke\n  ]\n}\n\nget {\n  url: {{base}}/users\n  body: none\n  auth: bearer\n}\n\nparams:query {\n  page: 1\n  ~debug: true\n}\n\nheaders {\n  Accept: application/json\n}\n\nauth:bearer {\n  token: {{token}}\n}\n\nassert {\n  res.status: eq 200\n}\n\ntests {\n  test(\"ok\", function () {});\n}\n",
        );
        assert_eq!(item["type"], "http-request");
        assert_eq!((item["name"].as_str(), item["seq"].as_i64()), (Some("List users"), Some(3)));
        assert_eq!(item["tags"], json!(["smoke"]));
        assert_eq!(item["request"]["method"], "GET");
        assert_eq!(item["request"]["url"], "{{base}}/users");
        assert_eq!(item["request"]["auth"], json!({ "mode": "bearer", "bearer": { "token": "{{token}}" } }));
        assert_eq!(
            item["request"]["params"][1],
            json!({ "name": "debug", "value": "true", "enabled": false, "type": "query" })
        );
        assert_eq!(item["request"]["assertions"][0]["name"], "res.status");
        assert_eq!(item["request"]["tests"], "test(\"ok\", function () {});");
    }

    #[test]
    fn graphql_requests_carry_their_query_and_variables() {
        let item = item("meta {\n  name: g\n  type: graphql\n}\n\npost {\n  url: x\n  body: graphql\n  auth: none\n}\n\nbody:graphql {\n  { products { id } }\n}\n\nbody:graphql:vars {\n  {\"first\": 3}\n}\n");
        assert_eq!(item["type"], "graphql-request");
        assert_eq!(item["request"]["body"]["mode"], "graphql");
        assert_eq!(
            item["request"]["body"]["graphql"],
            json!({ "query": "{ products { id } }", "variables": "{\"first\": 3}" })
        );
    }

    #[test]
    fn multipart_files_and_content_types_are_split_out() {
        let item = item("meta {\n  name: m\n}\n\npost {\n  url: x\n  body: multipartForm\n}\n\nbody:multipart-form {\n  photo: @file(a.png|b.png) @contentType(image/png)\n  note: hello\n}\n");
        let fields = &item["request"]["body"]["multipartForm"];
        assert_eq!(fields[0]["type"], "file");
        assert_eq!(fields[0]["value"], json!(["a.png", "b.png"]));
        assert_eq!(fields[0]["contentType"], "image/png");
        assert_eq!((fields[1]["type"].as_str(), fields[1]["contentType"].as_str()), (Some("text"), Some("")));
    }

    #[test]
    fn variables_flag_local_ones_and_apply_typed_annotations() {
        let item = item("meta {\n  name: v\n}\n\nget {\n  url: x\n}\n\nvars:pre-request {\n  @count: 3\n  @number\n  retries: 4\n  name: Ada\n}\n\nvars:post-response {\n  token: res.body.token\n}\n");
        let req = &item["request"]["vars"]["req"];
        assert_eq!((req[0]["name"].as_str(), req[0]["local"].as_bool()), (Some("count"), Some(true)));
        assert_eq!(req[1]["value"], json!(4));
        assert_eq!(req[1]["dataType"], "number");
        assert_eq!(req[2]["local"], json!(false));
        assert_eq!(item["request"]["vars"]["res"][0]["name"], "token");
    }

    #[test]
    fn oauth2_blocks_fill_the_grant_and_its_additional_parameters() {
        let item = item("meta {\n  name: o\n}\n\nget {\n  url: x\n  auth: oauth2\n}\n\nauth:oauth2 {\n  grant_type: client_credentials\n  access_token_url: https://t/token\n  client_id: id\n  client_secret: s\n  scope: read\n  credentials_placement: basic_auth_header\n  auto_refresh_token: true\n}\n\nauth:oauth2:additional_params:access_token_req:body {\n  audience: api\n}\n");
        let oauth = &item["request"]["auth"]["oauth2"];
        assert_eq!(oauth["grantType"], "client_credentials");
        assert_eq!(oauth["credentialsPlacement"], "basic_auth_header");
        assert_eq!(oauth["autoRefreshToken"], json!(true));
        assert_eq!(oauth["autoFetchToken"], json!(true));
        assert_eq!(oauth["additionalParameters"]["token"][0]["sendIn"], "body");
    }

    #[test]
    fn settings_default_encode_url_to_false_when_absent_like_bruno() {
        let item = item("meta {\n  name: s\n}\n\nget {\n  url: x\n}\n\nsettings {\n  timeout: 2000\n  followRedirects: false\n  maxRedirects: 3\n}\n");
        assert_eq!(
            item["settings"],
            json!({ "encodeUrl": false, "timeout": 2000, "followRedirects": false, "maxRedirects": 3 })
        );
    }

    #[test]
    fn saved_examples_are_attached_and_an_unreadable_one_becomes_a_warning() {
        let (json, _) = read("meta {\n  name: e\n}\n\nget {\n  url: x\n}\n\nexample {\n  name: Ok\n  response: {\n    status: {\n      code: 201\n      text: Created\n    }\n  }\n}\n\nexample {\n  nonsense\n}\n").unwrap();
        let (item, warnings) = request(&json).unwrap();
        assert_eq!(item["examples"].as_array().unwrap().len(), 1);
        assert_eq!(item["examples"][0]["response"]["status"], json!(201));
        assert_eq!(item["examples"][0]["request"]["method"], "get");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("Exemple n°2 ignoré"), "{warnings:?}");
    }

    #[test]
    fn grpc_websocket_and_app_items_are_unsupported() {
        let (json, _) = read("meta {\n  name: g\n  type: grpc\n}\n\ngrpc {\n  url: x\n}\n").unwrap();
        assert_eq!(request(&json), Err(Unsupported::Type("grpc".into())));
    }

    #[test]
    fn a_folder_file_gives_its_root() {
        let (json, _) = read("meta {\n  name: Users\n  seq: 2\n}\n\nauth {\n  mode: inherit\n}\n\nheaders {\n  X-A: 1\n}\n\ndocs {\n  # Users\n}\n").unwrap();
        let root = collection_root(&json);
        assert_eq!(root["meta"], json!({ "name": "Users", "seq": 2 }));
        assert_eq!(root["request"]["auth"]["mode"], "inherit");
        assert_eq!(root["request"]["headers"][0]["name"], "X-A");
        assert_eq!(root["docs"], "# Users");
    }
}
