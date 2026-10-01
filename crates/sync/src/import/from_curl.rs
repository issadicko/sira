//! cURL vers requête : collage dans la barre d'URL (`handleHttpPaste`, `QueryUrl`) et « New Request → From cURL »
//! (`NewRequest`, `newHttpRequest`) de Bruno. L'item JSON est celui que construit Bruno ; il passe par
//! `stringify::item`, comme le fichier écrit par Bruno.

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};
use url::Url;
use xc_core::collection::{resolve_path, write_atomic};
use xc_core::yaml::{self, is_js_space};
use xc_core::{open_collection, RequestDoc, TreeItem};

use super::naming::{fit, sanitize_name, validate_name};
use super::{join, text, ImportError};
use crate::curl::{request_from_curl, request_from_curl_typed};
use crate::stringify;

const REQUEST_EXT: &str = ".yml";
const AUTH_KEYS: [&str; 6] = ["basic", "bearer", "digest", "ntlm", "awsv4", "apikey"];

static ODATA_SEGMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[A-Za-z0-9_.-]+\([^)]*\)$").expect("expression régulière valide"));
static ODATA_PARAM: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r":([a-zA-Z_][A-Za-z0-9_]*)").expect("expression régulière valide"));

/// Début de `{{variable}}` en tête de `s` (`/\{\{.*?\}\}/`), avec sa longueur.
fn template_len(s: &str) -> Option<usize> {
    let inner = s.strip_prefix("{{")?;
    let end = inner.find("}}")?;
    (!inner[..end].contains(['\n', '\r', '\u{2028}', '\u{2029}'])).then_some(end + 4)
}

/// `splitOnFirst(url, '?')` : le `?` des `{{variables}}` ne compte pas.
fn split_on_first(url: &str) -> (&str, Option<&str>) {
    let bytes = url.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'?' => return (&url[..i], Some(&url[i + 1..])),
            b'{' => i += template_len(&url[i..]).unwrap_or(1),
            _ => i += 1,
        }
    }
    (url, None)
}

/// `parseQueryParams` : un paramètre sans `=` n'a pas de valeur.
fn query_params(query: Option<&str>) -> Vec<Value> {
    let Some(query) = query.filter(|q| !q.is_empty()) else { return Vec::new() };
    let without_fragment = query.split('#').next().unwrap_or_default();
    let pairs = without_fragment.split('&').map(|pair| match pair.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (pair, None),
    });
    pairs
        .filter(|(name, _)| !name.is_empty())
        .map(|(name, value)| {
            let mut param = Map::new();
            param.insert("name".into(), json!(name));
            if let Some(value) = value {
                param.insert("value".into(), json!(value));
            }
            param.insert("enabled".into(), json!(true));
            param.insert("type".into(), json!("query"));
            Value::Object(param)
        })
        .collect()
}

/// `parsePathParams` : segments `:nom` du chemin, et paramètres `:nom` des segments de style OData.
fn path_params(url: &str) -> Vec<Value> {
    if url.is_empty() {
        return Vec::new();
    }
    let with_scheme = if url.starts_with("http://") || url.starts_with("https://") {
        url.to_owned()
    } else {
        format!("http://{url}")
    };
    let path = Url::parse(&with_scheme).map_or_else(|_| with_scheme.clone(), |parsed| parsed.path().to_owned());
    let mut names: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        if let Some(name) = segment.strip_prefix(':') {
            if !name.is_empty() && !names.contains(&name) {
                names.push(name);
            }
        } else if ODATA_SEGMENT.is_match(segment) {
            for name in ODATA_PARAM.captures_iter(segment).filter_map(|c| c.get(1)).map(|m| m.as_str()) {
                if !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    }
    names.into_iter().map(|name| json!({ "name": name, "value": "", "enabled": true, "type": "path" })).collect()
}

/// `/^\s*curl\s/i` : ce que Bruno exige pour traiter un texte collé comme une commande cURL.
fn looks_like_curl(text: &str) -> bool {
    let rest = text.trim_start_matches(is_js_space);
    rest.get(..4).is_some_and(|word| word.eq_ignore_ascii_case("curl")) && rest[4..].starts_with(is_js_space)
}

fn pasted_headers(request: &Value) -> Vec<Value> {
    let headers = request.get("headers").and_then(Value::as_array).into_iter().flatten();
    headers
        .map(|header| {
            json!({
                "name": header.get("name").cloned().unwrap_or(json!("")),
                "value": header.get("value").cloned().unwrap_or(json!("")),
                "description": "",
                "enabled": header.get("enabled").cloned().unwrap_or(json!(true))
            })
        })
        .collect()
}

/// Bruno ne renseigne, au collage, que le contenu des corps json, text et xml ; les autres modes n'ont que leur mode.
fn pasted_body(request: &Value) -> Value {
    let body = request.get("body");
    let mode = body.map_or("none", |b| text(b, "mode"));
    let mut pasted = json!({
        "mode": mode, "json": null, "text": null, "xml": null, "sparql": null,
        "multipartForm": [], "formUrlEncoded": [], "file": []
    });
    let content = body.and_then(|b| b.get(mode)).filter(|c| !c.is_null() && c.as_str() != Some(""));
    if let (true, Some(content)) = (["json", "text", "xml"].contains(&mode), content) {
        pasted[mode] = content.clone();
    }
    pasted
}

fn pasted_auth(request: &Value) -> Value {
    let auth = request.get("auth").filter(|a| !text(a, "mode").is_empty());
    let Some(auth) = auth else { return json!({ "mode": "inherit" }) };
    let mut pasted = Map::new();
    match AUTH_KEYS.iter().find(|key| auth.get(**key).is_some_and(|content| !content.is_null())) {
        Some(key) => {
            pasted.insert("mode".into(), json!(key));
            pasted.insert((*key).into(), auth[*key].clone());
        }
        None => {
            pasted.insert("mode".into(), auth["mode"].clone());
        }
    }
    Value::Object(pasted)
}

fn settings() -> Value {
    json!({ "encodeUrl": false, "forwardAuthorizationHeader": false })
}

fn pasted_item(request: &Value) -> Value {
    let url = text(request, "url");
    let (base, query) = split_on_first(url);
    let params = [query_params(query), path_params(base)].concat();
    let method = match text(request, "method") {
        "" => "GET".to_owned(),
        method => method.to_uppercase(),
    };
    json!({
        "type": "http-request",
        "name": "",
        "request": {
            "method": method,
            "url": url,
            "headers": pasted_headers(request),
            "params": params,
            "body": pasted_body(request),
            "vars": { "req": [], "res": [] },
            "assertions": [],
            "auth": pasted_auth(request)
        },
        "settings": settings()
    })
}

/// La requête HTTP que Bruno construit quand on colle `command` dans la barre d'URL d'une requête vierge
/// (méthode en majuscules, paramètres tirés de l'URL, en-têtes, corps et authentification), ou `None` si le texte
/// n'est pas une commande cURL reconnue. Comme Bruno, seuls les corps json, text et xml reçoivent leur contenu.
/// `name` et `seq` ne sont pas significatifs : l'appelant garde ceux de la requête ouverte.
pub fn request_doc_from_curl(command: &str) -> Option<RequestDoc> {
    if !looks_like_curl(command) {
        return None;
    }
    let request = request_from_curl(command)?;
    match yaml::parse(&stringify::item(&pasted_item(&request))).ok()? {
        yaml::Value::Map(tree) => Some(RequestDoc::from_tree(&tree)),
        _ => None,
    }
}

/// `identifyCurlRequestType` : une URL en `/graphql` ou un `Content-Type` `application/graphql` donne une requête GraphQL.
fn is_graphql(request: &Value) -> bool {
    let content_type = request
        .get("headers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|header| text(header, "name").to_lowercase() == "content-type")
        .map_or("", |header| text(header, "value"));
    text(request, "url").ends_with("/graphql") || content_type.contains("application/graphql")
}

/// Validation du formulaire de Bruno : nom obligatoire de 255 caractères au plus, nom de fichier `sanitizeName(nom)`
/// valide et hors des noms réservés `collection` et `folder`. Renvoie le nom du fichier avec son extension.
fn file_name(name: &str) -> Result<String, ImportError> {
    let invalid = ImportError::InvalidName;
    let trimmed = name.trim_matches(is_js_space);
    if trimmed.is_empty() {
        return Err(invalid("le nom est obligatoire".into()));
    }
    if trimmed.encode_utf16().count() > 255 {
        return Err(invalid("le nom ne peut pas dépasser 255 caractères".into()));
    }
    let sanitized = sanitize_name(name);
    let filename = sanitized.trim_matches(is_js_space);
    if matches!(filename, "collection" | "folder") {
        return Err(invalid(format!("les noms de fichier « collection » et « folder » sont réservés : {filename}")));
    }
    validate_name(filename).map_err(invalid)?;
    let base = filename.replacen(REQUEST_EXT, "", 1);
    validate_name(&base).map_err(invalid)?;
    Ok(fit(&base, "", REQUEST_EXT))
}

fn find_folder<'a>(items: &'a [TreeItem], path: &str) -> Option<&'a [TreeItem]> {
    items.iter().find_map(|item| match item {
        TreeItem::Folder { path: p, children, .. } if p == path => Some(children.as_slice()),
        TreeItem::Folder { children, .. } => find_folder(children, path),
        TreeItem::Request { .. } => None,
    })
}

/// `seq` d'une nouvelle requête : un de plus que le nombre de dossiers et de requêtes du dossier visé.
fn next_seq(root: &Path, folder: &str) -> Result<usize, ImportError> {
    let collection = open_collection(root)?;
    let siblings =
        if folder.is_empty() { Some(collection.items.as_slice()) } else { find_folder(&collection.items, folder) };
    siblings.map(|items| items.len() + 1).ok_or_else(|| ImportError::FolderNotFound(folder.to_owned()))
}

fn new_item(request: &Value, kind: &str, name: &str, seq: usize) -> Value {
    let url = text(request, "url");
    let params = [query_params(split_on_first(url).1), path_params(url)].concat();
    json!({
        "type": kind,
        "name": name,
        "request": {
            "method": request["method"],
            "url": request["url"],
            "headers": request.get("headers").cloned().unwrap_or_else(|| json!([])),
            "params": params,
            "body": request["body"],
            "vars": { "req": [], "res": [] },
            "assertions": [],
            "auth": request.get("auth").cloned().unwrap_or_else(|| json!({ "mode": "inherit" }))
        },
        "settings": settings(),
        "seq": seq
    })
}

/// « New Request → From cURL » de Bruno : crée dans `folder` (relatif à la racine, `""` pour la racine) le fichier
/// de la requête `name` décrite par `command`, avec le `seq` suivant du dossier, et renvoie son chemin relatif
/// avec des `/`. Contrairement à Bruno, qui numérote le fichier en silence, un fichier existant est refusé.
pub fn create_request_from_curl(root: &Path, folder: &str, name: &str, command: &str) -> Result<String, ImportError> {
    let mut request = request_from_curl(command).ok_or(ImportError::InvalidCurl)?;
    let kind = if is_graphql(&request) { "graphql-request" } else { "http-request" };
    if kind == "graphql-request" {
        request = request_from_curl_typed(command, kind).ok_or(ImportError::InvalidCurl)?;
    }
    let file = file_name(name)?;
    let folder = folder.trim_matches('/');
    let dir = resolve_path(root, folder)?;
    if !dir.is_dir() {
        return Err(ImportError::FolderNotFound(folder.to_owned()));
    }
    let relative = join(folder, &file);
    let path = dir.join(&file);
    if path.exists() {
        return Err(ImportError::AlreadyExists(relative));
    }
    let item = new_item(&request, kind, name, next_seq(root, folder)?);
    write_atomic(&path, &stringify::item(&item))?;
    Ok(relative)
}
