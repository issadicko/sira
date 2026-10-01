//! `folder.yml` et `opencollection.yml` (`stringifyFolder.ts` et `stringifyCollection.ts`).

use serde_json::Value as Json;
use xc_core::yaml::{Map, Value};

use super::common::{map, markdown_docs, put, put_some, request_defaults};
use super::js::{get, has_length, is_false, is_true, or, string, truthy, yaml};

pub fn folder(root: &Json) -> Map {
    let meta = root.get("meta");
    let mut info =
        map([("name", or(get(meta, "name"), Value::str("Untitled Folder"))), ("type", Value::str("folder"))]);
    put_some(&mut info, "seq", get(meta, "seq").filter(|s| s.is_number()).map(yaml));
    let tags = normalize_tags(get(meta, "tags"));
    if !tags.is_empty() {
        put(&mut info, "tags", Value::Seq(tags.into_iter().map(Value::Str).collect()));
    }
    let request = root.get("request");
    let with_auth = get(get(request, "auth"), "mode").and_then(Json::as_str) != Some("none");
    let mut m = map([("info", Value::Map(info))]);
    put_some(&mut m, "request", request_defaults(request, with_auth));
    put_some(&mut m, "docs", markdown_docs(root.get("docs")));
    m
}

fn normalize_tags(tags: Option<&Json>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tag in tags.and_then(Json::as_array).into_iter().flatten().filter_map(Json::as_str) {
        let tag = super::js::trim(tag);
        if !tag.is_empty() && !out.iter().any(|t| t == tag) {
            out.push(tag.to_owned());
        }
    }
    out
}

pub fn collection(root: &Json, config: &Json) -> Map {
    let mut info = map([("name", or(config.get("name"), Value::str("Untitled Collection")))]);
    if let Some(v) = config.get("version").filter(|v| !v.is_null() && v.as_str() != Some("")) {
        put(&mut info, "version", Value::Str(string(v)));
    }
    let mut m = map([("opencollection", Value::str("1.0.0")), ("info", Value::Map(info))]);
    put_some(&mut m, "config", collection_config(config));
    let request = root.get("request");
    let with_auth = get(get(request, "auth"), "mode").is_some_and(|mode| truthy(Some(mode)) && mode != "none");
    put_some(&mut m, "request", request_defaults(request, with_auth));
    put_some(&mut m, "docs", markdown_docs(root.get("docs")));
    put(&mut m, "bundled", Value::Bool(false));
    put(&mut m, "extensions", Value::Map(extensions(config)));
    m
}

fn valid_proxy(config: &Json) -> Option<&Json> {
    let proxy = config.get("proxy").filter(|p| p.is_object() && p.get("inherit").is_some())?;
    proxy.get("config").filter(|c| c.is_object()).map(|_| proxy)
}

fn collection_config(config: &Json) -> Option<Value> {
    let protobuf = config.get("protobuf");
    let proto_files = get(protobuf, "protoFiles").and_then(Json::as_array).filter(|l| !l.is_empty());
    let import_paths = get(protobuf, "importPaths").and_then(Json::as_array).filter(|l| !l.is_empty());
    let proxy = valid_proxy(config);
    let certs = get(config.get("clientCertificates"), "certs").and_then(Json::as_array).filter(|l| !l.is_empty());
    if proto_files.is_none() && import_paths.is_none() && proxy.is_none() && certs.is_none() {
        return None;
    }
    let mut m = Map::default();
    if proto_files.is_some() || import_paths.is_some() {
        let mut p = Map::default();
        if let Some(files) = proto_files {
            let files = files.iter().map(|f| {
                let mut file = map([("type", Value::str("file"))]);
                put_some(&mut file, "path", f.get("path").map(yaml));
                Value::Map(file)
            });
            put(&mut p, "protoFiles", Value::Seq(files.collect()));
        }
        if let Some(paths) = import_paths {
            let paths = paths.iter().map(|i| {
                let mut path = Map::default();
                put_some(&mut path, "path", i.get("path").map(yaml));
                if is_false(i.get("enabled")) {
                    put(&mut path, "disabled", Value::Bool(true));
                }
                Value::Map(path)
            });
            put(&mut p, "importPaths", Value::Seq(paths.collect()));
        }
        put(&mut m, "protobuf", Value::Map(p));
    }
    if let Some(proxy) = proxy {
        put(&mut m, "proxy", proxy_config(proxy));
    }
    if let Some(certs) = certs {
        put(&mut m, "clientCertificates", Value::Seq(certs.iter().filter_map(certificate).collect()));
    }
    Some(Value::Map(m))
}

fn proxy_config(proxy: &Json) -> Value {
    let c = proxy.get("config");
    let auth = get(c, "auth");
    let text = |v: Option<&Json>| or(v, Value::str(""));
    let mut credentials = map([("username", text(get(auth, "username"))), ("password", text(get(auth, "password")))]);
    if is_true(get(auth, "disabled")) {
        put(&mut credentials, "disabled", Value::Bool(true));
    }
    let mut m = Map::default();
    put_some(&mut m, "inherit", proxy.get("inherit").map(yaml));
    put(
        &mut m,
        "config",
        Value::Map(map([
            ("protocol", or(get(c, "protocol"), Value::str("http"))),
            ("hostname", text(get(c, "hostname"))),
            ("port", text(get(c, "port"))),
            ("auth", Value::Map(credentials)),
            ("bypassProxy", text(get(c, "bypassProxy"))),
        ])),
    );
    if is_true(proxy.get("disabled")) {
        put(&mut m, "disabled", Value::Bool(true));
    }
    Value::Map(m)
}

fn certificate(cert: &Json) -> Option<Value> {
    let (ty, paths): (&str, &[(&str, &str)]) = match cert.get("type").and_then(Json::as_str)? {
        "cert" => ("pem", &[("certificateFilePath", "certFilePath"), ("privateKeyFilePath", "keyFilePath")]),
        "pfx" => ("pkcs12", &[("pkcs12FilePath", "pfxFilePath")]),
        _ => return None,
    };
    let mut m = Map::default();
    put_some(&mut m, "domain", cert.get("domain").map(yaml));
    put(&mut m, "type", Value::str(ty));
    for (key, field) in paths {
        put_some(&mut m, key, cert.get(field).map(yaml));
    }
    if truthy(cert.get("passphrase")) {
        put_some(&mut m, "passphrase", cert.get("passphrase").map(yaml));
    }
    if is_true(cert.get("disabled")) {
        put(&mut m, "disabled", Value::Bool(true));
    }
    Some(Value::Map(m))
}

fn extensions(config: &Json) -> Map {
    let mut bruno = Map::default();
    let ignore = config.get("ignore");
    if has_length(ignore) {
        put_some(&mut bruno, "ignore", ignore.filter(|i| i.is_array()).map(yaml));
    }
    let presets = config.get("presets");
    let preset = |key: &str| get(presets, key).filter(|v| has_length(Some(v))).map(yaml);
    let (request_type, request_url, environment) =
        (preset("requestType"), preset("requestUrl"), preset("defaultEnvironment"));
    if request_type.is_some() || request_url.is_some() || environment.is_some() {
        let mut request = Map::default();
        put_some(&mut request, "type", request_type);
        put_some(&mut request, "url", request_url);
        let mut p = Map::default();
        if !request.is_empty() {
            put(&mut p, "request", Value::Map(request));
        }
        put_some(&mut p, "defaultEnvironment", environment);
        put(&mut bruno, "presets", Value::Map(p));
    }
    let scripts = config.get("scripts");
    let mut s = Map::default();
    let roots = get(scripts, "additionalContextRoots");
    if has_length(roots) {
        put_some(&mut s, "additionalContextRoots", roots.map(yaml));
    }
    if let Some(flow) = get(scripts, "flow").filter(|f| *f == "sandwich" || *f == "sequential") {
        put(&mut s, "flow", yaml(flow));
    }
    if !s.is_empty() {
        put(&mut bruno, "scripts", Value::Map(s));
    }
    let openapi = openapi_sync(config.get("openapi"));
    if !openapi.is_empty() {
        put(&mut bruno, "openapi", Value::Seq(openapi));
    }
    if bruno.is_empty() {
        return Map::default();
    }
    map([("bruno", Value::Map(bruno))])
}

fn openapi_sync(entries: Option<&Json>) -> Vec<Value> {
    let usable = |e: &&Json| e.get("sourceUrl").and_then(Json::as_str).is_some_and(|u| !u.is_empty());
    let list = entries.and_then(Json::as_array).into_iter().flatten().filter(usable);
    list.map(|e| {
        let mut m = map([("sourceUrl", e.get("sourceUrl").map(yaml).unwrap_or(Value::Null))]);
        if let Some(g) = e.get("groupBy").filter(|g| *g == "tags" || *g == "path") {
            put(&mut m, "groupBy", yaml(g));
        }
        for key in ["lastSyncDate", "specHash"] {
            if truthy(e.get(key)) {
                put_some(&mut m, key, e.get(key).map(yaml));
            }
        }
        put(&mut m, "autoCheck", Value::Bool(!is_false(e.get("autoCheck"))));
        put(&mut m, "autoCheckInterval", or(e.get("autoCheckInterval"), Value::Int(5)));
        Value::Map(m)
    })
    .collect()
}
