//! Port de `openapi-common.js` et des aides de `common/index.js` partagées par les
//! convertisseurs OpenAPI 3 et Swagger 2.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use super::heap::{type_error, Heap, Js};
use super::R;
use crate::js::{collapse_spaces, is_js_space, js_order, number_value, string_to_number, trim};

const MAX_DEPTH: usize = 400;

/// Opération extraite de la spec, avant transformation en requête Bruno.
pub struct Request {
    pub method: Rc<str>,
    pub path: String,
    pub original_path: Rc<str>,
    pub op: Js,
    pub servers: Js,
    pub key: String,
}

/// Requête Bruno sans `seq` ni clé d'opération.
pub struct Item {
    pub fields: Map<String, Value>,
    pub key: String,
}

pub enum Entry {
    Request(Item),
    Folder(Value),
}

pub type Transform<'a> = dyn FnMut(&Request, &mut HashSet<String>) -> R<Item> + 'a;

pub fn str_of(v: &Js, what: &str) -> R<Rc<str>> {
    match v {
        Js::Str(s) => Ok(s.clone()),
        _ => Err(type_error(format!("{what} n'est pas une chaîne"))),
    }
}

/// Objet JSON en omettant les valeurs `undefined`, comme `JSON.stringify`.
pub fn object(h: &Heap, fields: Vec<(&str, Js)>) -> R<Value> {
    let mut map = Map::new();
    for (k, v) in fields {
        if let Some(v) = h.to_value(&v)? {
            map.insert(k.to_owned(), v);
        }
    }
    Ok(Value::Object(map))
}

pub fn to_spec_string(v: &Js) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

pub fn normalize_item_name(name: &str) -> String {
    let collapsed = collapse_spaces(name, " ");
    trim(&collapsed).trim_end_matches(|c: char| c == '.' || is_js_space(c)).to_owned()
}

/// `name`, sinon `name (MÉTHODE)`, sinon `name (n)`.
pub fn unique_name(mut name: String, method: &str, used: &mut HashSet<String>) -> String {
    if used.contains(&name) {
        let mut unique = format!("{name} ({})", method.to_uppercase());
        let mut counter = 1;
        while used.contains(&unique) {
            unique = format!("{name} ({counter})");
            counter += 1;
        }
        name = unique;
    }
    used.insert(name.clone());
    name
}

/// `summary || operationId || description`, normalisé ; vide si aucun.
pub fn operation_name(h: &Heap, op: &Js) -> R<String> {
    let source =
        h.get(op, "summary").or(h.get(op, "operationId")).or(Js::Str(to_spec_string(&h.get(op, "description")).into()));
    if !source.truthy() {
        return Ok(String::new());
    }
    Ok(normalize_item_name(&str_of(&source, "le nom de l'opération")?))
}

pub fn ensure_url(url: &str) -> String {
    let chars: Vec<char> = url.chars().collect();
    let mut out = String::with_capacity(url.len());
    let mut i = 0;
    while i < chars.len() {
        let slashes = chars[i + 1..].iter().take_while(|c| **c == '/').count();
        if chars[i] != ':' && slashes >= 2 {
            out.push(chars[i]);
            out.push('/');
            i += 1 + slashes;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// `path.replace(/{([^}]+)}/g, ':$1')`
pub fn colon_path(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        match after.find('}') {
            Some(end) if end > 0 => {
                out.push_str(&rest[..start]);
                out.push(':');
                out.push_str(&after[..end]);
                rest = &after[end + 1..];
            }
            _ => {
                out.push_str(&rest[..=start]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Chemin OpenAPI dont chaque segment de gabarit `{...}` devient `{}`.
pub fn template_path(path: &str) -> String {
    let mut out = String::new();
    let mut rest = path;
    while let Some(start) = rest.find('{') {
        let Some(end) = rest[start..].find('}') else { break };
        out.push_str(&rest[..start]);
        out.push_str("{}");
        rest = &rest[start + end + 1..];
    }
    out.push_str(rest);
    out
}

/// Clé d'opération : `operationId` s'il est non vide, sinon `MÉTHODE chemin`, rendue unique par ` #n`.
pub fn assign_keys(h: &Heap, requests: &mut [Request]) {
    let mut used = HashSet::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for request in requests {
        let base = match h.get(&request.op, "operationId").as_str().map(trim) {
            Some(id) if !id.is_empty() => id.to_owned(),
            _ => format!("{} {}", request.method.to_uppercase(), template_path(&request.original_path)),
        };
        let n = seen.entry(base.clone()).or_insert(0);
        *n += 1;
        let mut key = if *n == 1 { base.clone() } else { format!("{base} #{n}") };
        while used.contains(&key) {
            *n += 1;
            key = format!("{base} #{n}");
        }
        used.insert(key.clone());
        request.key = key;
    }
}

pub fn merge_params(h: &Heap, path_params: &Js, op_params: &Js) -> R<Js> {
    let id =
        |p: &Js| -> R<String> { Ok(format!("{}:{}", h.to_string(&h.prop(p, "name")?), h.to_string(&h.get(p, "in")))) };
    let own = h.array_strict(op_params, "parameters")?;
    let overrides = own.iter().map(id).collect::<R<HashSet<_>>>()?;
    let mut merged = Vec::new();
    for p in h.array_strict(path_params, "parameters")? {
        if !overrides.contains(&id(&p)?) {
            merged.push(p);
        }
    }
    merged.extend(own);
    Ok(h.arr(merged))
}

pub fn auth_template(mode: &str) -> Map<String, Value> {
    let Value::Object(map) =
        json!({"mode": mode, "basic": null, "bearer": null, "digest": null, "apikey": null, "oauth2": null})
    else {
        unreachable!()
    };
    map
}

pub fn status_text(status: f64) -> &'static str {
    let code = if status.fract() == 0.0 && (100.0..=511.0).contains(&status) { status as u16 } else { 0 };
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choice",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        306 => "unused",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "Unknown",
    }
}

fn structured_type(mime: &str, suffix: &str) -> bool {
    let token =
        |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'));
    let Some((kind, sub)) = mime.split_once('/') else { return false };
    token(kind) && (sub == suffix || sub.strip_suffix(suffix).and_then(|p| p.strip_suffix('+')).is_some_and(token))
}

pub fn body_type_from_content_type(content_type: &Js) -> &'static str {
    let Some(ct) = content_type.as_str().filter(|s| !s.is_empty()) else { return "text" };
    let ct = ct.to_lowercase();
    if structured_type(&ct, "json") {
        "json"
    } else if structured_type(&ct, "xml") {
        "xml"
    } else if structured_type(&ct, "html") {
        "html"
    } else {
        "text"
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Handler {
    Json,
    Form,
    Multipart,
    Xml,
    Sparql,
    Text,
}

impl Handler {
    pub fn find(mime: &str) -> Option<Self> {
        if structured_type(mime, "json") {
            Some(Self::Json)
        } else if mime == "application/x-www-form-urlencoded" {
            Some(Self::Form)
        } else if mime == "multipart/form-data" {
            Some(Self::Multipart)
        } else if structured_type(mime, "xml") || mime == "application/xml" {
            Some(Self::Xml)
        } else if mime == "application/sparql-query" {
            Some(Self::Sparql)
        } else if mime.starts_with("text/") || mime == "application/octet-stream" || mime == "*/*" {
            Some(Self::Text)
        } else {
            None
        }
    }

    pub fn mode(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Form => "formUrlEncoded",
            Self::Multipart => "multipartForm",
            Self::Xml => "xml",
            Self::Sparql => "sparql",
            Self::Text => "text",
        }
    }
}

#[derive(Clone)]
pub enum Sparql {
    Absent,
    Null,
    Str(String),
}

/// Corps de requête Bruno (`request.body`).
#[derive(Clone)]
pub struct Body {
    pub mode: &'static str,
    pub json: Option<String>,
    pub text: Option<String>,
    pub xml: Option<String>,
    pub sparql: Sparql,
    pub form_url_encoded: Vec<Value>,
    pub multipart_form: Vec<Value>,
}

impl Body {
    pub fn none() -> Self {
        Self {
            mode: "none",
            json: None,
            text: None,
            xml: None,
            sparql: Sparql::Absent,
            form_url_encoded: Vec::new(),
            multipart_form: Vec::new(),
        }
    }

    fn example_copy(&self) -> Self {
        let sparql = match &self.sparql {
            Sparql::Str(s) if !s.is_empty() => Sparql::Str(s.clone()),
            _ => Sparql::Null,
        };
        Self { sparql, ..self.clone() }
    }

    /// Corps d'une requête : `sparql` n'existe que s'il a été renseigné, en dernier.
    pub fn to_value(&self) -> Value {
        let mut map = self.head();
        self.forms(&mut map);
        if let Sparql::Str(s) = &self.sparql {
            map.insert("sparql".into(), json!(s));
        }
        Value::Object(map)
    }

    /// Corps d'un exemple : `sparql` toujours présent, avant les formulaires.
    fn example_value(&self) -> Value {
        let mut map = self.head();
        let sparql = match &self.sparql {
            Sparql::Str(s) => json!(s),
            _ => Value::Null,
        };
        map.insert("sparql".into(), sparql);
        self.forms(&mut map);
        Value::Object(map)
    }

    fn head(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert("mode".into(), json!(self.mode));
        map.insert("json".into(), json!(self.json));
        map.insert("text".into(), json!(self.text));
        map.insert("xml".into(), json!(self.xml));
        map
    }

    fn forms(&self, map: &mut Map<String, Value>) {
        map.insert("formUrlEncoded".into(), Value::Array(self.form_url_encoded.clone()));
        map.insert("multipartForm".into(), Value::Array(self.multipart_form.clone()));
    }
}

/// Port de `BODY_TYPE_HANDLERS[i].handle(body, bodySchema)`.
pub fn handle_body(h: &Heap, handler: Handler, body: &mut Body, schema: &Js) -> R<()> {
    match handler {
        Handler::Json => {
            if !schema.truthy() {
                return Ok(());
            }
            let example = h.get(schema, "example");
            let value = if !example.is_undef() {
                example
            } else if h.get(schema, "type").is_str("array") {
                let items = h.get(schema, "items");
                if items.truthy() {
                    let item = default_value(h, &items, &mut Vec::new(), 0)?;
                    h.arr(vec![item])
                } else {
                    h.arr(Vec::new())
                }
            } else {
                empty_json_body(h, schema, &mut Vec::new(), 0)?
            };
            body.json = h.stringify(&value, true)?;
        }
        Handler::Form | Handler::Multipart => {
            if !schema.truthy() {
                return Ok(());
            }
            let example = h.get(schema, "example");
            let is_example = example.truthy();
            let fields = example.or(h.get(schema, "properties")).or(h.obj(Vec::new()));
            for (name, prop) in h.each(&fields) {
                let is_file = handler == Handler::Multipart
                    && !is_example
                    && h.prop(&prop, "type")?.is_str("string")
                    && h.get(&prop, "format").is_str("binary");
                let value = if is_file || is_example {
                    prop.clone()
                } else {
                    h.prop(&prop, "example")?.coalesce(h.get(&prop, "default")).coalesce(Js::str(""))
                };
                let text = if value.is_undef() { Js::str("") } else { Js::Str(h.to_string(&value).into()) };
                let description = h.prop(&prop, "description")?.or(Js::str(""));
                if handler == Handler::Form {
                    body.form_url_encoded.push(object(
                        h,
                        vec![
                            ("name", name),
                            ("value", text),
                            ("description", description),
                            ("enabled", Js::Bool(true)),
                        ],
                    )?);
                } else {
                    let (kind, value) = if is_file { ("file", h.arr(Vec::new())) } else { ("text", text) };
                    body.multipart_form.push(object(
                        h,
                        vec![
                            ("type", Js::str(kind)),
                            ("name", name),
                            ("value", value),
                            ("description", description),
                            ("enabled", Js::Bool(true)),
                        ],
                    )?);
                }
            }
        }
        Handler::Xml => body.xml = Some(xml_body(h, schema)?),
        Handler::Sparql | Handler::Text => {
            let example = h.get(schema, "example");
            let text = if example.is_undef() { String::new() } else { h.to_string(&example) };
            if handler == Handler::Sparql {
                body.sparql = Sparql::Str(text);
            } else {
                body.text = Some(text);
            }
        }
    }
    Ok(())
}

fn guard(depth: usize) -> R<()> {
    if depth > MAX_DEPTH {
        return Err(type_error("récursion trop profonde dans un schéma"));
    }
    Ok(())
}

/// Port de `getDefaultValueForSchema`.
pub fn default_value(h: &Heap, schema: &Js, visited: &mut Vec<Js>, depth: usize) -> R<Js> {
    guard(depth)?;
    let example = h.prop(schema, "example")?;
    if !example.is_undef() {
        return Ok(example);
    }
    let enumeration = h.get(schema, "enum");
    if enumeration.truthy() && h.gt_zero(&h.get(&enumeration, "length")) {
        return Ok(h.index(&enumeration, 0));
    }
    let kind = h.get(schema, "type");
    if kind.is_str("object") || h.get(schema, "properties").truthy() {
        return empty_json_body(h, schema, visited, depth + 1);
    }
    if kind.is_str("array") {
        let items = h.get(schema, "items");
        if items.truthy() {
            if h.get(&items, "type").is_str("object") || h.get(&items, "properties").truthy() {
                let item = empty_json_body(h, &items, visited, depth + 1)?;
                return Ok(h.arr(vec![item]));
            }
            let example = h.get(&items, "example");
            if !example.is_undef() {
                return Ok(if example.is_array() { example } else { h.arr(vec![example]) });
            }
            let item = default_value(h, &items, visited, depth + 1)?;
            let empty =
                item.strict_eq(&Js::str("")) || item.strict_eq(&Js::Num(0.0)) || item.strict_eq(&Js::Bool(false));
            if !empty {
                return Ok(h.arr(vec![item]));
            }
        }
        return Ok(h.arr(Vec::new()));
    }
    Ok(primitive_default(&kind))
}

fn primitive_default(kind: &Js) -> Js {
    if kind.is_str("integer") || kind.is_str("number") {
        Js::Num(0.0)
    } else if kind.is_str("boolean") {
        Js::Bool(false)
    } else {
        Js::str("")
    }
}

/// Port de `buildEmptyJsonBody`, `visited` suivant les ancêtres par identité.
pub fn empty_json_body(h: &Heap, schema: &Js, visited: &mut Vec<Js>, depth: usize) -> R<Js> {
    guard(depth)?;
    if visited.iter().any(|v| v.strict_eq(schema)) {
        return Ok(h.obj(Vec::new()));
    }
    visited.push(schema.clone());
    let properties = h.prop(schema, "properties")?.or(h.obj(Vec::new()));
    let mut entries = Vec::new();
    for (name, prop) in h.each(&properties) {
        let value = default_value(h, &prop, visited, depth + 1)?;
        entries.push((h.to_string(&name).into(), value));
    }
    visited.pop();
    Ok(h.obj(entries))
}

/// Port de `getExampleFromSchema`.
pub fn example_from_schema(h: &Heap, schema: &Js) -> R<Js> {
    let example = h.prop(schema, "example")?;
    if !example.is_undef() {
        return Ok(example);
    }
    let kind = h.get(schema, "type");
    if kind.is_str("object") || (h.get(schema, "properties").truthy() && !kind.truthy()) {
        return empty_json_body(h, schema, &mut Vec::new(), 0);
    }
    if kind.is_str("array") {
        let items = h.get(schema, "items");
        if items.truthy() {
            let item_kind = h.get(&items, "type");
            if item_kind.is_str("object") || h.get(&items, "properties").truthy() {
                let item = empty_json_body(h, &items, &mut Vec::new(), 0)?;
                return Ok(h.arr(vec![item]));
            }
            if item_kind.is_str("integer")
                || item_kind.is_str("number")
                || item_kind.is_str("boolean")
                || item_kind.is_str("string")
            {
                return Ok(h.arr(vec![primitive_default(&item_kind)]));
            }
        }
        return Ok(h.arr(Vec::new()));
    }
    Ok(primitive_default(&kind))
}

/// Port de `buildXmlBody`.
pub fn xml_body(h: &Heap, schema: &Js) -> R<String> {
    if !schema.truthy() {
        return Ok(String::new());
    }
    let example = h.get(schema, "example");
    if let Js::Str(s) = &example {
        return Ok(s.to_string());
    }
    let values = matches!(example, Js::Arr(_) | Js::Obj(_)).then_some(example);
    let properties = h.get(schema, "properties");
    if !properties.truthy() && values.is_none() {
        return Ok(String::new());
    }
    let root = h.to_string(&h.get(&h.get(schema, "xml"), "name").or(Js::str("root")));
    let value_of = |k: &str| values.as_ref().map_or(Js::Undef, |v| h.get(v, k));
    let mut attributes = Vec::new();
    for (name, p) in h.entries(&properties.clone().or(h.obj(Vec::new())))? {
        let xml = h.prop(&p, "xml")?;
        if h.get(&xml, "attribute").truthy() {
            let attr_name = h.to_string(&h.get(&xml, "name").or(Js::Str(name.clone())));
            attributes.push(format!("{attr_name}=\"{}\"", h.to_string(&value_of(&name).coalesce(Js::str("")))));
        }
    }
    let entries: Vec<(Rc<str>, Js, Js)> = if properties.truthy() {
        h.entries(&properties)?.into_iter().map(|(k, p)| (k.clone(), p, value_of(&k))).collect()
    } else {
        h.entries(&values.clone().unwrap_or(Js::Undef).or(h.obj(Vec::new())))?
            .into_iter()
            .map(|(k, v)| (k, h.obj(Vec::new()), v))
            .collect()
    };
    let mut children = Vec::new();
    for (name, prop, value) in entries {
        children.extend(xml_element(h, &name, prop, &value, "  ", 0)?);
    }
    let attrs = if attributes.is_empty() { String::new() } else { format!(" {}", attributes.join(" ")) };
    let inner = if children.is_empty() { String::new() } else { format!("\n{}\n", children.join("\n")) };
    Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<{root}{attrs}>{inner}</{root}>"))
}

fn xml_element(h: &Heap, name: &str, prop: Js, value: &Js, indent: &str, depth: usize) -> R<Option<String>> {
    guard(depth)?;
    let prop = if prop.is_undef() { h.obj(Vec::new()) } else { prop };
    let xml = h.prop(&prop, "xml")?;
    let xml_name = h.to_string(&h.get(&xml, "name").or(Js::str(name)));
    if h.get(&xml, "attribute").truthy() {
        return Ok(None);
    }
    let wrap = |children: Vec<String>| {
        let inner = if children.is_empty() { String::new() } else { format!("\n{}\n{indent}", children.join("\n")) };
        format!("{indent}<{xml_name}>{inner}</{xml_name}>")
    };
    let deeper = format!("{indent}  ");
    if let Js::Obj(_) = value {
        let mut children = Vec::new();
        for (k, v) in h.entries(value)? {
            let child = h.get(&h.get(&prop, "properties"), &k).or(h.obj(Vec::new()));
            children.extend(xml_element(h, &k, child, &v, &deeper, depth + 1)?);
        }
        return Ok(Some(wrap(children)));
    }
    let properties = h.get(&prop, "properties");
    if h.get(&prop, "type").is_str("object") || properties.truthy() {
        let mut children = Vec::new();
        for (k, p) in h.entries(&properties.or(h.obj(Vec::new())))? {
            children.extend(xml_element(h, &k, p, &Js::Undef, &deeper, depth + 1)?);
        }
        return Ok(Some(wrap(children)));
    }
    let content = if value.is_nullish() { String::new() } else { h.to_string(value) };
    Ok(Some(format!("{indent}<{xml_name}>{content}</{xml_name}>")))
}

/// Port de `populateRequestBody`.
fn populate_body(h: &Heap, body: &mut Body, schema: &Js, content_type: &Js) -> R<()> {
    let Some(ct) = content_type.as_str().filter(|s| !s.is_empty()) else { return Ok(()) };
    let normalized = ct.to_lowercase();
    if let Some(handler) = Handler::find(&normalized) {
        body.mode = handler.mode();
        if normalized == "application/x-www-form-urlencoded" {
            body.form_url_encoded.clear();
        } else if normalized == "multipart/form-data" {
            body.multipart_form.clear();
        }
        handle_body(h, handler, body, schema)?;
    }
    Ok(())
}

/// État d'une requête en cours de construction, recopié dans ses exemples.
pub struct Draft {
    pub url: String,
    pub method: String,
    pub headers: Vec<Value>,
    pub params: Vec<Value>,
    pub body: Body,
}

pub struct Example {
    pub value: Js,
    pub name: Js,
    pub description: Js,
    pub status: Rc<str>,
    pub content_type: Js,
    pub request_body: Option<(Js, Js)>,
}

/// Port de `createBrunoExample`.
pub fn create_example(h: &Heap, draft: &Draft, ex: Example) -> R<Value> {
    let name = h.to_string(&ex.name.clone().coalesce(Js::str(""))).replace("\r\n", " ").replace('\n', " ");
    let name = match trim(&name) {
        "" => format!("{} Response", ex.status),
        n => n.to_owned(),
    };
    let numeric = string_to_number(&ex.status);
    let status = numeric.is_finite().then_some(numeric);
    let mut body = draft.body.example_copy();
    if let Some((schema, content_type)) = &ex.request_body {
        populate_body(h, &mut body, schema, content_type)?;
    }
    let response_type = body_type_from_content_type(&ex.content_type);
    let content = match &ex.value {
        Js::Obj(_) | Js::Arr(_) if response_type == "xml" => {
            let wrapper = h.obj(vec![("example".into(), ex.value.clone())]);
            Some(Value::String(xml_body(h, &wrapper)?))
        }
        Js::Obj(_) | Js::Arr(_) | Js::Null => h.stringify(&ex.value, true)?.map(Value::String),
        other => h.to_value(other)?,
    };
    let headers = if ex.content_type.truthy() {
        vec![object(
            h,
            vec![
                ("name", Js::str("Content-Type")),
                ("value", ex.content_type.clone()),
                ("description", Js::str("")),
                ("enabled", Js::Bool(true)),
            ],
        )?]
    } else {
        Vec::new()
    };
    let mut response_body = Map::new();
    response_body.insert("type".into(), json!(response_type));
    if let Some(content) = content {
        response_body.insert("content".into(), content);
    }
    let mut example = Map::new();
    example.insert("name".into(), json!(name));
    if let Some(description) = h.to_value(&ex.description)? {
        example.insert("description".into(), description);
    }
    example.insert("type".into(), json!("http-request"));
    example.insert(
        "request".into(),
        json!({
            "url": draft.url,
            "method": draft.method,
            "headers": draft.headers,
            "params": draft.params,
            "body": body.example_value(),
        }),
    );
    example.insert(
        "response".into(),
        json!({
            "status": status.map_or(Value::Null, number_value),
            "statusText": status.filter(|s| *s != 0.0).map(status_text),
            "headers": headers,
            "body": response_body,
        }),
    );
    Ok(Value::Object(example))
}

static NOT_TAG_CHAR: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}\-_]").expect("regex"));
static UNDERSCORES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_+").expect("regex"));
static LEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^\p{L}\p{N}]+").expect("regex"));
static TRAILING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\p{L}\p{N}]+$").expect("regex"));

/// Port de `sanitizeTag` pour le format BRU (Bruno n'indique pas `collectionFormat` à l'import).
pub fn sanitize_tag(h: &Heap, tag: &Js) -> R<Option<String>> {
    if !tag.truthy() || !matches!(tag, Js::Str(_) | Js::Obj(_) | Js::Arr(_)) {
        return Ok(None);
    }
    let usable = match tag {
        Js::Str(s) => s.clone(),
        Js::Obj(_) if h.has_own(tag, "name")? => str_of(&h.get(tag, "name"), "le nom de tag")?,
        _ => "".into(),
    };
    let underscored = collapse_spaces(trim(&usable), "_");
    let replaced = NOT_TAG_CHAR.replace_all(&underscored, "_");
    let collapsed = UNDERSCORES.replace_all(&replaced, "_");
    let leading = LEADING.replace(&collapsed, "");
    let sanitized = TRAILING.replace(&leading, "");
    Ok((!sanitized.is_empty()).then(|| sanitized.into_owned()))
}

pub fn sanitize_tags(h: &Heap, tags: &Js) -> R<Vec<String>> {
    let Some(items) = h.items(tags) else { return Ok(Vec::new()) };
    let mut out: Vec<String> = Vec::new();
    for tag in items {
        if let Some(t) = sanitize_tag(h, &tag)? {
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    Ok(out)
}

const PROTOTYPE_KEYS: [&str; 12] = [
    "constructor",
    "__defineGetter__",
    "__defineSetter__",
    "hasOwnProperty",
    "__lookupGetter__",
    "__lookupSetter__",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "toString",
    "valueOf",
    "__proto__",
    "toLocaleString",
];

/// Table indexée par une clé de la spec dans un objet JavaScript ordinaire (`{}`).
struct Groups<T>(Vec<(Rc<str>, T)>);

impl<T> Groups<T> {
    fn entry(&mut self, key: &str, make: impl FnOnce() -> T) -> R<&mut T> {
        if PROTOTYPE_KEYS.contains(&key) {
            return Err(type_error(format!("clé « {key} » réservée par JavaScript")));
        }
        let at = match self.0.iter().position(|(k, _)| &**k == key) {
            Some(i) => i,
            None => {
                self.0.push((key.into(), make()));
                self.0.len() - 1
            }
        };
        Ok(&mut self.0[at].1)
    }

    fn values(self) -> Vec<T> {
        js_order(self.0).into_iter().map(|(_, v)| v).collect()
    }
}

fn folder(name: &str, docs: Option<&String>, items: Vec<Value>) -> Value {
    let mut root = Map::new();
    root.insert("request".into(), json!({ "auth": auth_template("inherit") }));
    root.insert("meta".into(), json!({ "name": name }));
    if let Some(docs) = docs {
        root.insert("docs".into(), json!(docs));
    }
    json!({ "name": name, "type": "folder", "root": root, "items": items })
}

/// Numérote les requêtes (`seq`) comme `hydrateSeqInCollection` puis ajoute la clé d'opération.
pub fn finish(entries: Vec<Entry>) -> Vec<Value> {
    let mut seq = 0;
    entries
        .into_iter()
        .map(|entry| match entry {
            Entry::Folder(f) => f,
            Entry::Request(item) => {
                seq += 1;
                let mut fields = item.fields;
                fields.insert("seq".into(), json!(seq));
                fields.insert("operationKey".into(), json!(item.key));
                Value::Object(fields)
            }
        })
        .collect()
}

/// Port de `groupRequestsByTags` et de l'assemblage des dossiers par tag.
pub fn group_by_tags(h: &Heap, requests: &[Request], spec_tags: &Js, transform: &mut Transform) -> R<Vec<Value>> {
    let mut groups: Groups<Vec<&Request>> = Groups(Vec::new());
    let mut ungrouped = Vec::new();
    for request in requests {
        let tags = h.get(&request.op, "tags").or(h.arr(Vec::new()));
        if !h.gt_zero(&h.get(&tags, "length")) {
            ungrouped.push(request);
            continue;
        }
        let first = str_of(&h.index(&tags, 0), "le premier tag")?;
        match sanitize_tag(h, &Js::str(trim(&first)))? {
            Some(tag) => groups.entry(&tag, Vec::new)?.push(request),
            None => ungrouped.push(request),
        }
    }
    let descriptions = tag_descriptions(h, spec_tags)?;
    let mut used = HashSet::new();
    let mut entries = Vec::new();
    for (name, members) in js_order(groups.0) {
        let items = members.into_iter().map(|r| transform(r, &mut used).map(Entry::Request)).collect::<R<Vec<_>>>()?;
        entries.push(Entry::Folder(folder(&name, descriptions.get(&*name), finish(items))));
    }
    for request in ungrouped {
        entries.push(Entry::Request(transform(request, &mut used)?));
    }
    Ok(finish(entries))
}

fn tag_descriptions(h: &Heap, tags: &Js) -> R<HashMap<String, String>> {
    let mut out = HashMap::new();
    for (_, tag) in h.each(&tags.clone().or(h.arr(Vec::new()))) {
        let name = h.get(&tag, "name");
        if matches!(tag, Js::Obj(_) | Js::Arr(_)) && name.as_str().is_some() {
            let docs = to_spec_string(&h.get(&tag, "description"));
            if let (Some(key), false) = (sanitize_tag(h, &name)?, docs.is_empty()) {
                out.insert(key, docs);
            }
        }
    }
    Ok(out)
}

struct PathGroup<'a> {
    name: Rc<str>,
    requests: Vec<&'a Request>,
    sub: Groups<PathGroup<'a>>,
}

impl<'a> PathGroup<'a> {
    fn new(name: &str) -> Self {
        Self { name: name.into(), requests: Vec::new(), sub: Groups(Vec::new()) }
    }
}

/// Port de `groupRequestsByPath`.
pub fn group_by_path(requests: &[Request], transform: &mut Transform) -> R<Vec<Value>> {
    let mut groups: Groups<PathGroup> = Groups(Vec::new());
    for request in requests {
        let path = if request.original_path.is_empty() { request.path.as_str() } else { &request.original_path };
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let Some((first, rest)) = segments.split_first() else {
            groups.entry("Root", || PathGroup::new("Root"))?.requests.push(request);
            continue;
        };
        let mut group = groups.entry(first, || PathGroup::new(first))?;
        for segment in rest {
            group = group.sub.entry(segment, || PathGroup::new(segment))?;
        }
        group.requests.push(request);
    }
    groups
        .values()
        .into_iter()
        .map(|g| {
            let name = g.name.clone();
            Ok(folder(&name, None, build_path_folder(g, transform)?))
        })
        .collect()
}

fn build_path_folder(group: PathGroup, transform: &mut Transform) -> R<Vec<Value>> {
    let mut used = HashSet::new();
    let mut entries =
        group.requests.into_iter().map(|r| transform(r, &mut used).map(Entry::Request)).collect::<R<Vec<_>>>()?;
    for sub in group.sub.values() {
        let name = sub.name.clone();
        let items = build_path_folder(sub, transform)?;
        if !items.is_empty() {
            entries.push(Entry::Folder(folder(&name, None, items)));
        }
    }
    Ok(finish(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ef_imp_02_string_helpers_follow_bruno() {
        assert_eq!(normalize_item_name("  Get\n  the\tpets . . "), "Get the pets");
        assert_eq!(ensure_url("{{baseUrl}}//a///b"), "{{baseUrl}}/a/b");
        assert_eq!(ensure_url("https://x//y"), "https://x/y");
        assert_eq!(colon_path("/a/{id}/{x{y}/{}/{z"), "/a/:id/:x{y/{}/{z");
        assert_eq!(template_path("/users/{id}/posts/{post_id}.{fmt}"), "/users/{}/posts/{}.{}");
    }

    #[test]
    fn ef_imp_02_sanitize_tag_matches_bru_rules() {
        let h = Heap::default();
        let tag = |s: &str| sanitize_tag(&h, &Js::str(s)).unwrap();
        assert_eq!(tag("  Pet  Store!! "), Some("Pet_Store".into()));
        assert_eq!(tag("__été-2024__"), Some("été-2024".into()));
        assert_eq!(tag("¡!"), None);
    }
}
