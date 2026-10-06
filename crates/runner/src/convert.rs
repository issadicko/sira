use serde_json::{json, Map, Value};
use xc_core::vars::Context;
use xc_core::{merged_headers, Auth, Body, KeyValue, ParamKind, RequestDoc};
use xc_engine::HttpResponse;
use xc_script::{PathParam, ResponseSize, ScriptRequest, ScriptResponse};

fn auth_mode(auth: &Auth) -> &'static str {
    match auth {
        Auth::Bearer { .. } => "bearer",
        Auth::Basic { .. } => "basic",
        Auth::Apikey { .. } => "apikey",
        Auth::Digest { .. } => "digest",
        Auth::Awsv4 { .. } => "awsv4",
        Auth::Oauth2(_) => "oauth2",
        Auth::Inherit | Auth::None | Auth::Other { .. } => "none",
    }
}

fn pairs(fields: &[KeyValue]) -> Value {
    Value::Array(fields.iter().map(|f| json!({ "name": f.name, "value": f.value, "enabled": f.enabled })).collect())
}

/// Le corps comme `req.getBody({ raw: true })` le donne : le texte tel que saisi (variables non résolues), ou les
/// champs d'un formulaire.
fn raw_body(body: &Body) -> Option<Value> {
    Some(match body {
        Body::Json { data } | Body::Text { data } | Body::Xml { data } => Value::String(data.clone()),
        Body::FormUrlEncoded { fields } => pairs(fields),
        Body::None | Body::MultipartForm { .. } | Body::Other { .. } => return None,
    })
}

fn content_type_of(body: &Body) -> Option<&'static str> {
    match body {
        Body::Json { .. } => Some("application/json"),
        Body::Text { .. } => Some("text/plain"),
        Body::Xml { .. } => Some("application/xml"),
        _ => None,
    }
}

/// La requête avant interpolation, telle que Bruno la montre à un script pré-requête.
pub fn script_request(ctx: &Context, doc: &RequestDoc) -> ScriptRequest {
    let mut headers: Map<String, Value> =
        merged_headers(ctx, doc).into_iter().map(|(k, v)| (k, Value::String(v))).collect();
    if let Some(kind) = content_type_of(&doc.body) {
        if !headers.keys().any(|k| k.eq_ignore_ascii_case("content-type")) {
            headers.insert("content-type".into(), Value::String(kind.into()));
        }
    }
    ScriptRequest {
        name: doc.name.clone(),
        method: doc.method.clone(),
        url: doc.url.clone(),
        headers,
        data: raw_body(&doc.body),
        tags: Vec::new(),
        path_params: doc
            .params
            .iter()
            .filter(|p| p.kind == ParamKind::Path)
            .map(|p| PathParam { name: p.name.clone(), value: p.value.clone(), kind: "path".into() })
            .collect(),
        timeout: doc.timeout_ms,
        auth_mode: Some(auth_mode(&doc.auth).into()),
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Ce que le script a changé de la requête : le document modifié et, si les en-têtes ont changé, leur nouvelle liste.
pub fn apply_request(
    doc: &RequestDoc,
    before: &ScriptRequest,
    after: &ScriptRequest,
) -> (RequestDoc, Option<Vec<(String, String)>>) {
    let mut next = doc.clone();
    next.method.clone_from(&after.method);
    next.url.clone_from(&after.url);
    if after.data != before.data {
        next.body = match (&after.data, &doc.body) {
            (None, _) => Body::None,
            (Some(Value::String(data)), Body::Json { .. }) => Body::Json { data: data.clone() },
            (Some(Value::String(data)), Body::Xml { .. }) => Body::Xml { data: data.clone() },
            (Some(Value::String(data)), Body::Text { .. }) => Body::Text { data: data.clone() },
            (Some(Value::String(data)), _) => {
                let json = after
                    .headers
                    .iter()
                    .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && text(v).contains("json"));
                if json {
                    Body::Json { data: data.clone() }
                } else {
                    Body::Text { data: data.clone() }
                }
            }
            (Some(Value::Array(items)), Body::FormUrlEncoded { .. }) => Body::FormUrlEncoded {
                fields: items
                    .iter()
                    .filter_map(Value::as_object)
                    .map(|m| KeyValue {
                        name: m.get("name").map(text).unwrap_or_default(),
                        value: m.get("value").map(text).unwrap_or_default(),
                        enabled: m.get("enabled").is_none_or(|e| e != &Value::Bool(false)),
                        description: None,
                    })
                    .collect(),
            },
            (Some(other), _) => Body::Json { data: other.to_string() },
        };
    }
    let headers =
        (after.headers != before.headers).then(|| after.headers.iter().map(|(k, v)| (k.clone(), text(v))).collect());
    (next, headers)
}

fn header_block(res: &HttpResponse) -> u64 {
    let status = format!("HTTP/1.1 {} {}\r\n", res.status, res.reason);
    let headers: usize = res.headers.iter().map(|(k, v)| k.len() + v.len() + 4).sum();
    (status.len() + headers + 2) as u64
}

/// La réponse pour les scripts : en-têtes en minuscules (`set-cookie` en liste, les autres répétés joints par une
/// virgule), corps en JSON quand il l'est, texte sinon (BOM retiré).
pub fn script_response(res: &HttpResponse, url: &str, parse_json: bool) -> ScriptResponse {
    let mut headers: Map<String, Value> = Map::new();
    for (name, value) in &res.headers {
        let key = name.to_ascii_lowercase();
        match (headers.get_mut(&key), key.as_str()) {
            (None, "set-cookie") => {
                headers.insert(key, json!([value]));
            }
            (None, _) => {
                headers.insert(key, Value::String(value.clone()));
            }
            (Some(Value::Array(list)), _) => list.push(Value::String(value.clone())),
            (Some(existing), _) => *existing = Value::String(format!("{}, {value}", text(existing))),
        }
    }
    let body = xc_engine::lossy_text(res.body.clone());
    let body = body.strip_prefix('\u{feff}').unwrap_or(&body);
    let data = if parse_json {
        serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.to_owned()))
    } else {
        Value::String(body.to_owned())
    };
    let header = header_block(res);
    ScriptResponse {
        status: res.status,
        status_text: res.reason.clone(),
        headers,
        data,
        response_time: res.timings.total_ms.round() as u64,
        url: url.to_owned(),
        size: ResponseSize { header, body: res.body.len() as u64, total: header + res.body.len() as u64 },
    }
}

/// Octets d'un corps posé par `res.setBody()` : le texte tel quel, le reste en JSON.
pub fn body_bytes(data: &Value) -> Vec<u8> {
    match data {
        Value::String(s) => s.clone().into_bytes(),
        Value::Null => Vec::new(),
        other => other.to_string().into_bytes(),
    }
}
