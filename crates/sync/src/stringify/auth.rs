//! Authentification (`formats/yml/common/auth.ts` et `auth-oauth2.ts` de Bruno).

use serde_json::Value as Json;
use xc_core::yaml::{Map, Value};

use super::access::{get, non_empty, non_empty_array, or, yaml};
use super::common::{map, put, put_some};
use crate::js::{trim, truthy};

pub fn auth(auth: Option<&Json>) -> Option<Value> {
    if !truthy(auth) {
        return None;
    }
    let config = |key: &str| get(auth, key);
    let built = match get(auth, "mode").and_then(Json::as_str)? {
        "inherit" => return Some(Value::str("inherit")),
        "awsv4" => strings(
            "awsv4",
            config("awsv4"),
            &["accessKeyId", "secretAccessKey", "sessionToken", "service", "region", "profileName"],
        ),
        "basic" => strings("basic", config("basic"), &["username", "password"]),
        "bearer" => strings("bearer", config("bearer"), &["token"]),
        "digest" => strings("digest", config("digest"), &["username", "password"]),
        "ntlm" => strings("ntlm", config("ntlm"), &["username", "password", "domain"]),
        "wsse" => strings("wsse", config("wsse"), &["username", "password"]),
        "apikey" => apikey(config("apikey")),
        "oauth1" => oauth1(config("oauth1")),
        "oauth2" => return oauth2(config("oauth2")).map(Value::Map),
        "akamai-edgegrid" => edgegrid(config("akamaiEdgegrid")),
        _ => return None,
    };
    Some(Value::Map(built))
}

fn put_strings(m: &mut Map, config: Option<&Json>, keys: &[&str]) {
    for key in keys {
        if let Some(s) = get(config, key).and_then(Json::as_str) {
            put(m, key, Value::str(s));
        }
    }
}

fn strings(ty: &str, config: Option<&Json>, keys: &[&str]) -> Map {
    let mut m = map([("type", Value::str(ty))]);
    put_strings(&mut m, config, keys);
    m
}

fn apikey(config: Option<&Json>) -> Map {
    let mut m = strings("apikey", config, &["key", "value"]);
    match get(config, "placement").and_then(Json::as_str) {
        Some("header") => put(&mut m, "placement", Value::str("header")),
        Some("queryparams") => put(&mut m, "placement", Value::str("query")),
        _ => {}
    }
    m
}

fn oauth1(config: Option<&Json>) -> Map {
    let mut m = strings(
        "oauth1",
        config,
        &[
            "consumerKey",
            "consumerSecret",
            "accessToken",
            "accessTokenSecret",
            "callbackUrl",
            "verifier",
            "signatureMethod",
        ],
    );
    if let Some(key) = get(config, "privateKey").and_then(Json::as_str) {
        let ty = if get(config, "privateKeyType").and_then(Json::as_str) == Some("file") { "file" } else { "text" };
        put(&mut m, "privateKey", Value::Map(map([("type", Value::str(ty)), ("value", Value::str(key))])));
    }
    put_strings(&mut m, config, &["timestamp", "nonce", "version", "realm", "placement"]);
    if let Some(b) = get(config, "includeBodyHash").filter(|v| v.is_boolean()) {
        put(&mut m, "includeBodyHash", yaml(b));
    }
    m
}

fn edgegrid(config: Option<&Json>) -> Map {
    let mut m = strings(
        "akamai-edgegrid",
        config,
        &["accessToken", "clientToken", "clientSecret", "nonce", "timestamp", "baseURL", "headersToSign"],
    );
    if let Some(n) = get(config, "maxBodySize").filter(|v| v.is_number()) {
        put(&mut m, "maxBodySize", yaml(n));
    }
    m
}

fn oauth2(o: Option<&Json>) -> Option<Map> {
    if !truthy(o) {
        return None;
    }
    let grant = get(o, "grantType").and_then(Json::as_str)?;
    let flow = match grant {
        "client_credentials" | "authorization_code" | "implicit" => grant,
        "password" => "resource_owner_password_credentials",
        _ => return None,
    };
    let mut m = map([("type", Value::str("oauth2")), ("flow", Value::str(flow))]);
    let text = |m: &mut Map, key: &str, field: &str| {
        if let Some(s) = non_empty(get(o, field)) {
            put(m, key, Value::str(s));
        }
    };
    let token_requests = [("token", "accessTokenRequest"), ("refresh", "refreshTokenRequest")];
    match grant {
        "client_credentials" | "password" => {
            text(&mut m, "accessTokenUrl", "accessTokenUrl");
            text(&mut m, "refreshTokenUrl", "refreshTokenUrl");
            put_some(&mut m, "credentials", credentials(o));
            if grant == "password" {
                let mut owner = Map::default();
                text(&mut owner, "username", "username");
                text(&mut owner, "password", "password");
                put_some(&mut m, "resourceOwner", (!owner.is_empty()).then_some(Value::Map(owner)));
            }
            text(&mut m, "scope", "scope");
            put_some(&mut m, "additionalParameters", additional_parameters(o, &token_requests));
        }
        "authorization_code" => {
            text(&mut m, "authorizationUrl", "authorizationUrl");
            text(&mut m, "accessTokenUrl", "accessTokenUrl");
            text(&mut m, "refreshTokenUrl", "refreshTokenUrl");
            text(&mut m, "callbackUrl", "callbackUrl");
            put_some(&mut m, "credentials", credentials(o));
            let requests = [("authorization", "authorizationRequest"), token_requests[0], token_requests[1]];
            put_some(&mut m, "additionalParameters", additional_parameters(o, &requests));
            text(&mut m, "scope", "scope");
            text(&mut m, "state", "state");
            put_some(&mut m, "pkce", pkce(get(o, "pkce")));
        }
        _ => {
            text(&mut m, "authorizationUrl", "authorizationUrl");
            text(&mut m, "callbackUrl", "callbackUrl");
            if let Some(id) = non_empty(get(o, "clientId")) {
                put(&mut m, "credentials", Value::Map(map([("clientId", Value::str(id))])));
            }
            text(&mut m, "scope", "scope");
            text(&mut m, "state", "state");
            put_some(
                &mut m,
                "additionalParameters",
                additional_parameters(o, &[("authorization", "authorizationRequest")]),
            );
        }
    }
    put(&mut m, "tokenConfig", token_config(o));
    put_some(&mut m, "settings", settings(o));
    Some(m)
}

fn credentials(o: Option<&Json>) -> Option<Value> {
    let mut m = Map::default();
    for (key, field) in
        [("clientId", "clientId"), ("clientSecret", "clientSecret"), ("placement", "credentialsPlacement")]
    {
        if let Some(s) = non_empty(get(o, field)) {
            put(&mut m, key, Value::str(s));
        }
    }
    (!m.is_empty()).then_some(Value::Map(m))
}

fn additional_parameters(o: Option<&Json>, requests: &[(&str, &str)]) -> Option<Value> {
    let lists = get(o, "additionalParameters");
    let mut m = Map::default();
    for (source, key) in requests {
        put_some(&mut m, key, parameters(get(lists, source)));
    }
    (!m.is_empty()).then_some(Value::Map(m))
}

fn parameters(list: Option<&Json>) -> Option<Value> {
    let mapped: Vec<Value> = non_empty_array(list)?
        .iter()
        .filter_map(|p| {
            let name = non_empty(p.get("name"))?;
            let placement = match trim(p.get("sendIn")?.as_str()?).to_lowercase().as_str() {
                "headers" => "header",
                "queryparams" => "query",
                "body" => "body",
                _ => return None,
            };
            let mut m = map([("name", Value::str(trim(name))), ("placement", Value::str(placement))]);
            if let Some(v) = non_empty(p.get("value")) {
                put(&mut m, "value", Value::str(v));
            }
            Some(Value::Map(m))
        })
        .collect();
    (!mapped.is_empty()).then_some(Value::Seq(mapped))
}

fn pkce(v: Option<&Json>) -> Option<Value> {
    match v {
        None | Some(Json::Null) => None,
        v if truthy(v) => Some(Value::Map(Map::default())),
        _ => Some(Value::Map(map([("disabled", Value::Bool(true))]))),
    }
}

fn token_config(o: Option<&Json>) -> Value {
    let mut m = Map::default();
    if let Some(id) = non_empty(get(o, "credentialsId")) {
        put(&mut m, "id", Value::str(id));
    }
    let placement = get(o, "tokenPlacement");
    if non_empty(placement).is_none() {
        put(&mut m, "placement", Value::Map(map([("header", Value::str(""))])));
    }
    let target = match placement.and_then(Json::as_str) {
        Some("header") => Some(("header", "tokenHeaderPrefix")),
        Some("url") => Some(("query", "tokenQueryKey")),
        _ => None,
    };
    if let Some((key, field)) = target {
        let mut p = Map::default();
        put_some(&mut p, key, get(o, field).map(yaml));
        put(&mut m, "placement", Value::Map(p));
    }
    put(&mut m, "source", or(get(o, "tokenSource"), Value::str("access_token")));
    Value::Map(m)
}

fn settings(o: Option<&Json>) -> Option<Value> {
    let mut m = Map::default();
    for key in ["autoFetchToken", "autoRefreshToken"] {
        put_some(&mut m, key, get(o, key).filter(|v| v.is_boolean()).map(yaml));
    }
    (!m.is_empty()).then_some(Value::Map(m))
}
