use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use base64::Engine as _;
use xc_engine::HttpRequest;

use crate::collection::resolve_path;
use crate::request::{
    auth_from, key_values, Auth, Body, KeyValue, MultipartField, MultipartValue, ParamKind, RequestDoc,
};
use crate::vars::{Context, Scope};
use crate::CoreError;

const NO_TIMEOUT: Duration = Duration::from_secs(600);

pub struct Prepared {
    pub request: HttpRequest,
    pub unresolved: Vec<String>,
}

/// Construit la requête prête à partir : variables résolues, en-têtes et auth hérités.
pub fn prepare(
    root: &Path,
    request_path: &str,
    doc: &RequestDoc,
    env: Option<&str>,
    runtime: &HashMap<String, String>,
) -> Result<Prepared, CoreError> {
    if doc.request_type != "http" {
        return Err(CoreError::UnsupportedRequestType(doc.request_type.clone()));
    }
    let ctx = Context::load(root, request_path)?;
    let scope = Scope::build(root, &ctx, request_path, doc, env, runtime)?;
    let mut unresolved = Vec::new();
    let mut fill = |s: &str| scope.interpolate(s, &mut unresolved);

    let mut url = fill(&substitute_path_params(&doc.url, doc));
    if !url.contains("://") {
        url = format!("http://{url}");
    }

    let mut headers: Vec<(String, String)> = Vec::new();
    let mut push = |name: String, value: String| {
        headers.retain(|(k, _)| !k.eq_ignore_ascii_case(&name));
        headers.push((name, value));
    };
    let parents = std::iter::once(&ctx.collection).chain(ctx.folders.iter().map(|(_, m)| m));
    for parent in parents {
        if let Some(section) = Context::request_section(parent) {
            for h in key_values(section.seq("headers")).into_iter().filter(|h| h.enabled) {
                push(fill(&h.name), fill(&h.value));
            }
        }
    }
    for h in doc.headers.iter().filter(|h| h.enabled && !h.name.is_empty()) {
        push(fill(&h.name), fill(&h.value));
    }

    match effective_auth(doc, &ctx) {
        Auth::Bearer { token } => push("Authorization".into(), format!("Bearer {}", fill(&token))),
        Auth::Basic { username, password } => {
            let raw = format!("{}:{}", fill(&username), fill(&password));
            push("Authorization".into(), format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(raw)));
        }
        Auth::Apikey { key, value, placement } if placement == "query" => {
            let sep = if url.contains('?') { '&' } else { '?' };
            url = format!("{url}{sep}{}={}", fill(&key), fill(&value));
        }
        Auth::Apikey { key, value, .. } => push(fill(&key), fill(&value)),
        Auth::Inherit | Auth::None | Auth::Other { .. } => {}
    }

    let current_type = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.clone());
    let typed = |default: &str| current_type.clone().unwrap_or_else(|| default.to_owned());
    let body = match &doc.body {
        Body::Json { data } => Some((fill(data).into_bytes(), typed("application/json"))),
        Body::Text { data } => Some((fill(data).into_bytes(), typed("text/plain"))),
        Body::Xml { data } => Some((fill(data).into_bytes(), typed("application/xml"))),
        Body::FormUrlEncoded { fields } => {
            Some((url_encoded(fields, &mut fill), typed("application/x-www-form-urlencoded")))
        }
        Body::MultipartForm { fields } => {
            let (content_type, boundary) = multipart_type(current_type.as_deref());
            Some((multipart(root, fields, &boundary, &mut fill)?, content_type))
        }
        Body::None | Body::Other { .. } => None,
    };
    if let Some((_, content_type)) = &body {
        match headers.iter_mut().find(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            Some(header) => header.1.clone_from(content_type),
            None => headers.push(("Content-Type".into(), content_type.clone())),
        }
    }

    Ok(Prepared {
        request: HttpRequest {
            method: doc.method.to_uppercase(),
            url,
            headers,
            body: body.map(|(bytes, _)| bytes),
            timeout: doc.timeout_ms.map(Duration::from_millis).unwrap_or(NO_TIMEOUT),
        },
        unresolved,
    })
}

fn effective_auth(doc: &RequestDoc, ctx: &Context) -> Auth {
    if doc.auth != Auth::Inherit {
        return doc.auth.clone();
    }
    for (_, folder) in ctx.folders.iter().rev() {
        match Context::request_section(folder).map(|r| auth_from(r.get("auth"))) {
            Some(Auth::Inherit) | None => continue,
            Some(auth) => return auth,
        }
    }
    Context::request_section(&ctx.collection).map(|r| auth_from(r.get("auth"))).unwrap_or(Auth::None)
}

/// Champs activés encodés comme `URLSearchParams`, ce qu'utilise Bruno.
fn url_encoded(fields: &[KeyValue], fill: &mut impl FnMut(&str) -> String) -> Vec<u8> {
    let mut form = form_urlencoded::Serializer::new(String::new());
    for field in fields.iter().filter(|f| f.enabled) {
        form.append_pair(&fill(&field.name), &fill(&field.value));
    }
    form.finish().into_bytes()
}

/// `Content-Type` et frontière : celle que l'utilisateur a fixée, sinon une nouvelle au format du paquet `form-data`.
fn multipart_type(current: Option<&str>) -> (String, String) {
    let current = current.unwrap_or("multipart/form-data");
    if let Some(boundary) = boundary_of(current) {
        return (current.to_owned(), boundary);
    }
    let boundary = format!("--------------------------{}", &uuid::Uuid::new_v4().simple().to_string()[..24]);
    if current.starts_with("multipart/") {
        (format!("{current}; boundary={boundary}"), boundary)
    } else {
        (current.to_owned(), boundary)
    }
}

fn boundary_of(content_type: &str) -> Option<String> {
    let start = content_type.to_ascii_lowercase().find("boundary=")? + "boundary=".len();
    let rest = &content_type[start..];
    let value = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split('"').next(),
        None => rest.split(|c: char| c == ';' || c.is_whitespace()).next(),
    };
    value.filter(|v| !v.is_empty()).map(str::to_owned)
}

/// Corps multipart octet pour octet comme le paquet `form-data` de Bruno ; fichiers lus depuis la collection.
fn multipart(
    root: &Path,
    fields: &[MultipartField],
    boundary: &str,
    fill: &mut impl FnMut(&str) -> String,
) -> Result<Vec<u8>, CoreError> {
    let mut body = Vec::new();
    for field in fields.iter().filter(|f| f.enabled) {
        let disposition = format!("form-data; name=\"{}\"", fill(&field.name));
        let content_type = field.content_type.as_deref().filter(|c| !c.is_empty());
        match &field.value {
            MultipartValue::Text(value) => {
                write_part(&mut body, boundary, &disposition, content_type, fill(value).as_bytes());
            }
            MultipartValue::File(paths) => {
                for path in paths {
                    let path = resolve_path(root, fill(path).trim())?;
                    let bytes = fs::read(&path).map_err(|e| CoreError::io(&path, e))?;
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                    let guessed = guess_mime(&path);
                    let disposition = format!("{disposition}; filename=\"{file_name}\"");
                    write_part(&mut body, boundary, &disposition, Some(content_type.unwrap_or(guessed)), &bytes);
                }
            }
        }
    }
    if !body.is_empty() {
        body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    }
    Ok(body)
}

/// Type MIME d'un fichier comme `form-data` (via `mime-types` 2.1) : quelques extensions où `mime_guess` diffère.
fn guess_mime(path: &Path) -> &'static str {
    let extension = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("xml") => "application/xml",
        Some("yml" | "yaml") => "text/yaml",
        Some("js") => "application/javascript",
        _ => mime_guess::from_path(path).first_raw().unwrap_or("application/octet-stream"),
    }
}

fn write_part(body: &mut Vec<u8>, boundary: &str, disposition: &str, content_type: Option<&str>, value: &[u8]) {
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: {disposition}\r\n").as_bytes());
    if let Some(content_type) = content_type {
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
    }
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(value);
    body.extend_from_slice(b"\r\n");
}

/// Remplace les segments `:nom` de l'URL par la valeur du paramètre de chemin correspondant.
fn substitute_path_params(url: &str, doc: &RequestDoc) -> String {
    let (base, query) = match url.split_once('?') {
        Some((b, q)) => (b, Some(q)),
        None => (url, None),
    };
    let replaced: Vec<String> = base
        .split('/')
        .map(|segment| {
            segment
                .strip_prefix(':')
                .and_then(|name| doc.params.iter().find(|p| p.kind == ParamKind::Path && p.enabled && p.name == name))
                .map(|p| p.value.clone())
                .unwrap_or_else(|| segment.to_owned())
        })
        .collect();
    let path = replaced.join("/");
    match query {
        Some(q) => format!("{path}?{q}"),
        None => path,
    }
}
