use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use base64::Engine as _;
use xc_engine::HttpRequest;

use crate::request::{auth_from, key_values, Auth, Body, ParamKind, RequestDoc};
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

    let (body, content_type) = match &doc.body {
        Body::Json { data } => (Some(fill(data)), Some("application/json")),
        Body::Text { data } => (Some(fill(data)), Some("text/plain")),
        Body::Xml { data } => (Some(fill(data)), Some("application/xml")),
        Body::None | Body::Other { .. } => (None, None),
    };
    if let Some(ct) = content_type {
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
            headers.push(("Content-Type".into(), ct.into()));
        }
    }

    Ok(Prepared {
        request: HttpRequest {
            method: doc.method.to_uppercase(),
            url,
            headers,
            body: body.map(String::into_bytes),
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
