//! Requêtes HTTP et GraphQL (`items/stringifyHttpRequest.ts` et `stringifyGraphQLRequest.ts`).

use serde_json::Value as Json;
use xc_core::yaml::{Map, Value};

use super::auth::auth;
use super::common::{
    actions, assertions, body, headers, map, params, put, put_some, response_headers, scripts, variables,
};
use super::js::{
    get, has_length, is_false, is_true, json_pretty, non_empty, non_empty_array, nullish, or, to_number, trim, truthy,
    yaml,
};

pub fn http(item: &Json) -> Map {
    let req = item.get("request");
    let mut root = map([("info", info(item, "http"))]);
    put(&mut root, "http", details(req, "GET", body(get(req, "body"))));
    put_some(&mut root, "runtime", runtime(req));
    put(&mut root, "settings", settings(item, true));
    put_some(
        &mut root,
        "examples",
        non_empty_array(item.get("examples")).map(|l| Value::Seq(l.iter().map(example).collect())),
    );
    put_some(&mut root, "docs", non_empty(get(req, "docs")).map(Value::str));
    put_some(&mut root, "app", app(item.get("app")));
    root
}

pub fn graphql(item: &Json) -> Map {
    let req = item.get("request");
    let mut root = map([("info", info(item, "graphql"))]);
    put(&mut root, "graphql", details(req, "POST", graphql_body(get(req, "body"))));
    put_some(&mut root, "runtime", runtime(req));
    put(&mut root, "settings", settings(item, false));
    put_some(&mut root, "docs", non_empty(get(req, "docs")).map(Value::str));
    root
}

fn info(item: &Json, ty: &str) -> Value {
    let name = non_empty(item.get("name")).unwrap_or("Untitled Request");
    let mut m = map([("name", Value::str(name)), ("type", Value::str(ty))]);
    if truthy(item.get("seq")) {
        put_some(&mut m, "seq", item.get("seq").map(yaml));
    }
    if has_length(item.get("tags")) {
        put_some(&mut m, "tags", item.get("tags").map(yaml));
    }
    put_some(&mut m, "description", non_empty(item.get("description")).map(Value::str));
    Value::Map(m)
}

fn details(req: Option<&Json>, default_method: &str, body: Option<Value>) -> Value {
    let mut m = map([
        ("method", Value::str(non_empty(get(req, "method")).unwrap_or(default_method))),
        ("url", Value::str(non_empty(get(req, "url")).unwrap_or(""))),
    ]);
    put_some(&mut m, "headers", headers(get(req, "headers")));
    put_some(&mut m, "params", params(get(req, "params")));
    put_some(&mut m, "body", body);
    put_some(&mut m, "auth", auth(get(req, "auth")));
    Value::Map(m)
}

fn graphql_body(body: Option<&Json>) -> Option<Value> {
    let gql = get(body, "graphql");
    if get(body, "mode").and_then(Json::as_str) != Some("graphql") || !truthy(gql) {
        return None;
    }
    let mut m = Map::default();
    for key in ["query", "variables"] {
        put_some(&mut m, key, non_empty(get(gql, key)).map(Value::str));
    }
    (!m.is_empty()).then_some(Value::Map(m))
}

fn runtime(req: Option<&Json>) -> Option<Value> {
    let vars = get(req, "vars");
    let mut m = Map::default();
    put_some(&mut m, "variables", variables(vars));
    put_some(&mut m, "scripts", scripts(req));
    put_some(&mut m, "assertions", assertions(get(req, "assertions")));
    put_some(&mut m, "actions", actions(get(vars, "res")));
    (!m.is_empty()).then_some(Value::Map(m))
}

fn settings(item: &Json, with_omit_headers: bool) -> Value {
    let s = item.get("settings");
    let timeout = match get(s, "timeout") {
        Some(t @ Json::String(inherit)) if inherit == "inherit" => yaml(t),
        Some(t @ Json::Number(n)) if n.as_f64().is_some_and(|f| f > 0.0) => yaml(t),
        _ => Value::Int(0),
    };
    let max_redirects = match get(s, "maxRedirects").and_then(Json::as_f64) {
        Some(f) if f >= 0.0 => yaml(&Json::from(f.trunc())),
        _ => Value::Int(5),
    };
    let mut m = map([
        ("encodeUrl", Value::Bool(!is_false(get(s, "encodeUrl")))),
        ("timeout", timeout),
        ("followRedirects", Value::Bool(!is_false(get(s, "followRedirects")))),
        ("maxRedirects", max_redirects),
        ("forwardAuthorizationHeader", nullish(get(s, "forwardAuthorizationHeader"), Value::Bool(true))),
    ]);
    if with_omit_headers {
        put_some(&mut m, "omitHeaders", omit_headers(get(s, "omitHeaders")));
    }
    Value::Map(m)
}

fn omit_headers(list: Option<&Json>) -> Option<Value> {
    let names: Vec<Value> = non_empty_array(list)?
        .iter()
        .map(|n| trim(n.as_str().unwrap_or("")))
        .filter(|n| !n.is_empty())
        .map(Value::str)
        .collect();
    (!names.is_empty()).then_some(Value::Seq(names))
}

fn example(ex: &Json) -> Value {
    let mut m = map([("name", or(ex.get("name"), Value::str("Untitled Example")))]);
    put_some(&mut m, "description", non_empty(ex.get("description")).map(Value::str));
    let request = ex.get("request");
    if truthy(request) {
        let mut r = map([
            ("url", or(get(request, "url"), Value::str(""))),
            ("method", or(get(request, "method"), Value::str("GET"))),
        ]);
        put_some(&mut r, "headers", headers(get(request, "headers")));
        put_some(&mut r, "params", params(get(request, "params")));
        put_some(&mut r, "body", body(get(request, "body")));
        put(&mut m, "request", Value::Map(r));
    }
    let response = ex.get("response");
    if truthy(response) {
        let mut r = Map::default();
        let status = to_number(get(response, "status"));
        if status.fract() == 0.0 && status > 0.0 && status.is_finite() {
            put(&mut r, "status", yaml(&Json::from(status)));
        }
        put_some(&mut r, "statusText", non_empty(get(response, "statusText")).map(Value::str));
        put_some(&mut r, "headers", response_headers(get(response, "headers")));
        let body = get(response, "body");
        if let (true, true, Some(content)) = (truthy(body), truthy(get(body, "type")), get(body, "content")) {
            let data = content.as_str().map_or_else(|| json_pretty(content), str::to_owned);
            put(
                &mut r,
                "body",
                Value::Map(map([("type", or(get(body, "type"), Value::Null)), ("data", Value::Str(data))])),
            );
        }
        put(&mut m, "response", Value::Map(r));
    }
    Value::Map(m)
}

fn app(app: Option<&Json>) -> Option<Value> {
    let enabled = is_true(get(app, "enabled"));
    if !truthy(app) || (!truthy(get(app, "code")) && !enabled) {
        return None;
    }
    let mut m = Map::default();
    if enabled {
        put(&mut m, "enabled", Value::Bool(true));
    }
    if truthy(get(app, "code")) {
        put_some(&mut m, "code", get(app, "code").map(yaml));
    }
    Some(Value::Map(m))
}
