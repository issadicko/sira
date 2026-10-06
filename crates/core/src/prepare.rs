use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use base64::Engine as _;
use xc_codegen::{Auth as SnippetAuth, Body as SnippetBody, Part, PartValue, Snippet};
use xc_engine::{HttpRequest, Network, Redirects};

use crate::collection::resolve_visible_path;
use crate::graphql;
use crate::oauth2::OAuth2;
use crate::request::{
    auth_from, key_values, Auth, Body, KeyValue, MultipartField, MultipartValue, ParamKind, RequestDoc,
};
use crate::vars::{Context, Scope, ScopeOverrides};
use crate::CoreError;

const NO_TIMEOUT: Duration = Duration::from_secs(600);
const MAX_MULTIPART_BODY: u64 = 512 << 20;

pub struct Prepared {
    pub request: HttpRequest,
    pub unresolved: Vec<String>,
    /// Ce que l'auth fait à l'envoi même, une fois les variables résolues : elle dépend de la requête finale ou de la
    /// réponse du serveur (signature, défi Digest).
    pub auth: SendAuth,
}

/// L'auth qui s'applique au moment de l'envoi.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SendAuth {
    #[default]
    None,
    Digest {
        username: String,
        password: String,
    },
    Aws(AwsSettings),
    /// OAuth 2.0, champs résolus : le jeton s'obtient (ou se rafraîchit) avant l'envoi.
    Oauth2(Box<OAuth2>),
}

/// La configuration AWS Signature V4 d'une requête, variables résolues ; un champ vide est absent.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AwsSettings {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    pub service: String,
    pub region: String,
    pub profile_name: String,
}

/// Ce qu'un script pré-requête a changé et que `doc` ne dit pas : les en-têtes de la collection, des dossiers et de la
/// requête, déjà fusionnés (sans l'auth ni le `Content-Type` automatique), et les variables écrites.
#[derive(Debug, Clone, Default)]
pub struct Overrides {
    pub headers: Option<Vec<(String, String)>>,
    pub vars: ScopeOverrides,
}

/// En-têtes activés de la collection, des dossiers puis de la requête, fusionnés comme le fait Bruno : une clé garde sa
/// première position et la dernière valeur, `content-type` est unifié en minuscules, les noms ne différant que par la
/// casse restent deux en-têtes. Ni interpolés, ni complétés par l'auth.
pub fn merged_headers(ctx: &Context, doc: &RequestDoc) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    let parents = std::iter::once(&ctx.collection).chain(ctx.folders.iter().map(|(_, m)| m));
    let inherited = parents
        .filter_map(Context::request_section)
        .flat_map(|section| key_values(section.seq("headers")))
        .chain(doc.headers.iter().cloned());
    for h in inherited.filter(|h| h.enabled && !h.name.is_empty()) {
        let name = if h.name.eq_ignore_ascii_case("content-type") { "content-type".to_owned() } else { h.name };
        match headers.iter_mut().find(|(k, _)| *k == name) {
            Some(slot) => slot.1 = h.value,
            None => headers.push((name, h.value)),
        }
    }
    headers
}

/// Les redirections que la requête demande dans son bloc `settings`, avec les défauts d'exécution de Bruno.
pub fn redirects_of(doc: &RequestDoc) -> Redirects {
    Redirects {
        follow: doc.follow_redirects.unwrap_or(true),
        max: doc.max_redirects.map_or(Redirects::DEFAULT_MAX, |max| u32::try_from(max).unwrap_or(u32::MAX)),
        forward_authorization: doc.forward_authorization_header.unwrap_or(true),
    }
}

/// Construit la requête prête à partir : variables résolues, en-têtes et auth hérités.
pub fn prepare(
    root: &Path,
    request_path: &str,
    doc: &RequestDoc,
    env: Option<&str>,
    runtime: &HashMap<String, String>,
) -> Result<Prepared, CoreError> {
    prepare_with(root, request_path, doc, env, runtime, Overrides::default())
}

/// [`prepare`] avec ce que les scripts ont changé.
pub fn prepare_with(
    root: &Path,
    request_path: &str,
    doc: &RequestDoc,
    env: Option<&str>,
    runtime: &HashMap<String, String>,
    overrides: Overrides,
) -> Result<Prepared, CoreError> {
    if doc.request_type != "http" && doc.request_type != "graphql" {
        return Err(CoreError::UnsupportedRequestType(doc.request_type.clone()));
    }
    let ctx = Context::load(root, request_path)?;
    let scope = Scope::build(root, &ctx, request_path, doc, env, runtime)?.with_overrides(overrides.vars);
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
    let own;
    let merged = match &overrides.headers {
        Some(headers) => headers,
        None => {
            own = merged_headers(&ctx, doc);
            &own
        }
    };
    for (name, value) in merged {
        push(fill(name), fill(value));
    }

    let mut send_auth = SendAuth::None;
    match effective_auth(doc, &ctx) {
        Auth::Digest { username, password } => {
            send_auth = SendAuth::Digest { username: fill(&username), password: fill(&password) };
        }
        Auth::Oauth2(config) => send_auth = SendAuth::Oauth2(Box::new(config.resolved(&mut fill))),
        Auth::Awsv4 { access_key_id, secret_access_key, session_token, service, region, profile_name } => {
            send_auth = SendAuth::Aws(AwsSettings {
                access_key_id: fill(&access_key_id),
                secret_access_key: fill(&secret_access_key),
                session_token: fill(&session_token),
                service: fill(&service),
                region: fill(&region),
                profile_name: fill(&profile_name),
            });
        }
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
        Body::Graphql { query, variables } => {
            Some((graphql::payload(&fill(query), &fill(variables))?.into_bytes(), typed("application/json")))
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
            max_response_body: None,
            network: Network { redirects: redirects_of(doc), ..Network::default() },
        },
        unresolved,
        auth: send_auth,
    })
}

/// La requête telle qu'elle partirait, variables résolues, prête à être transcrite en code (`xc-codegen`). Les fichiers
/// d'un formulaire multipart ne sont pas lus : l'extrait garde leurs chemins. Renvoie aussi les variables sans valeur.
pub fn snippet(
    root: &Path,
    request_path: &str,
    doc: &RequestDoc,
    env: Option<&str>,
    runtime: &HashMap<String, String>,
    overrides: Overrides,
) -> Result<(Snippet, Vec<String>), CoreError> {
    let multipart = match &doc.body {
        Body::MultipartForm { fields } => Some(fields.clone()),
        _ => None,
    };
    let mut plain = doc.clone();
    if multipart.is_some() {
        plain.body = Body::None;
    }
    let Prepared { request, mut unresolved, auth } =
        prepare_with(root, request_path, &plain, env, runtime, overrides.clone())?;

    let mut headers = request.headers;
    let body = match multipart {
        Some(fields) => {
            headers.retain(|(name, _)| !name.eq_ignore_ascii_case("content-type"));
            let ctx = Context::load(root, request_path)?;
            let scope = Scope::build(root, &ctx, request_path, doc, env, runtime)?.with_overrides(overrides.vars);
            let mut fill = |s: &str| scope.interpolate(s, &mut unresolved);
            let mut parts = Vec::new();
            for field in fields.iter().filter(|f| f.enabled) {
                let name = fill(&field.name);
                let content_type = field.content_type.clone().filter(|c| !c.is_empty());
                match &field.value {
                    MultipartValue::Text(value) => {
                        parts.push(Part { name, value: PartValue::Text(fill(value)), content_type });
                    }
                    MultipartValue::File(paths) => {
                        for path in paths {
                            let path = fill(path).trim().to_owned();
                            parts.push(Part {
                                name: name.clone(),
                                value: PartValue::File(path),
                                content_type: content_type.clone(),
                            });
                        }
                    }
                }
            }
            SnippetBody::Multipart(parts)
        }
        None => match request.body {
            Some(bytes) => SnippetBody::Raw(String::from_utf8_lossy(&bytes).into_owned()),
            None => SnippetBody::None,
        },
    };

    let mut notes = Vec::new();
    let auth = match auth {
        SendAuth::None => SnippetAuth::None,
        SendAuth::Digest { username, password } => SnippetAuth::Digest { username, password },
        SendAuth::Aws(aws) => SnippetAuth::Aws {
            access_key_id: aws.access_key_id,
            secret_access_key: aws.secret_access_key,
            session_token: aws.session_token,
            region: aws.region,
            service: aws.service,
        },
        SendAuth::Oauth2(_) => {
            notes.push(
                "OAuth 2.0 : ajoute l'en-tête Authorization avec le jeton que Sira obtient à l'envoi.".to_owned(),
            );
            SnippetAuth::None
        }
    };
    unresolved.dedup();
    Ok((Snippet { method: request.method, url: request.url, headers, body, auth, notes }, unresolved))
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

/// Corps multipart octet pour octet comme le paquet `form-data` de Bruno ; fichiers lus depuis la collection,
/// sans éléments cachés (`.env`) et dans la limite de 512 Mo au total.
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
                    let path = resolve_visible_path(root, fill(path).trim())?;
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                    let disposition = format!("{disposition}; filename=\"{file_name}\"");
                    write_head(&mut body, boundary, &disposition, Some(content_type.unwrap_or(guess_mime(&path))));
                    append_file(&path, &mut body)?;
                    body.extend_from_slice(b"\r\n");
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

fn write_head(body: &mut Vec<u8>, boundary: &str, disposition: &str, content_type: Option<&str>) {
    body.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: {disposition}\r\n").as_bytes());
    if let Some(content_type) = content_type {
        body.extend_from_slice(format!("Content-Type: {content_type}\r\n").as_bytes());
    }
    body.extend_from_slice(b"\r\n");
}

fn write_part(body: &mut Vec<u8>, boundary: &str, disposition: &str, content_type: Option<&str>, value: &[u8]) {
    write_head(body, boundary, disposition, content_type);
    body.extend_from_slice(value);
    body.extend_from_slice(b"\r\n");
}

/// Lit le fichier à la suite de `body`, sans que celui-ci dépasse [`MAX_MULTIPART_BODY`] octets.
fn append_file(path: &Path, body: &mut Vec<u8>) -> Result<(), CoreError> {
    let io = |e| CoreError::io(path, e);
    let too_large = || CoreError::BodyTooLarge { path: path.display().to_string(), max_mb: MAX_MULTIPART_BODY >> 20 };
    let file = fs::File::open(path).map_err(io)?;
    let room = MAX_MULTIPART_BODY.saturating_sub(body.len() as u64);
    if file.metadata().map_err(io)?.len() > room {
        return Err(too_large());
    }
    file.take(room + 1).read_to_end(body).map_err(io)?;
    if body.len() as u64 > MAX_MULTIPART_BODY {
        return Err(too_large());
    }
    Ok(())
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
