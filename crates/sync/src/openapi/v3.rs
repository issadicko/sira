//! Port de `openapi-to-bruno.js` (OpenAPI 3.x).

use std::collections::HashSet;
use std::rc::Rc;

use serde_json::{json, Map, Value};

use super::common::{
    assign_keys, auth_template, colon_path, create_example, ensure_url, example_from_schema, group_by_path,
    group_by_tags, handle_body, merge_params, normalize_item_name, object, operation_name, sanitize_tags, str_of,
    to_spec_string, unique_name, Body, Draft, Example, Handler, Item, Request,
};
use super::heap::{type_error, Heap, Js};
use super::resolve::{resolve, Flavor};
use super::{GroupBy, R};
use crate::js::{expand_replacement, replace_all, trim, utf16};

pub const METHODS: [&str; 8] = ["get", "put", "post", "delete", "options", "head", "patch", "trace"];

pub fn convert(h: &Heap, data: &Value, group_by: GroupBy) -> R<Value> {
    let spec = resolve(h, data, Flavor::OpenApi)?;
    if !spec.truthy() {
        return Err(type_error("Invalid OpenAPI collection. Failed to resolve refs."));
    }
    let name = collection_name(h, &spec)?;
    let environments = environments(h, &spec)?;
    let security = Security::new(h, &spec)?;
    let mut requests = requests(h, &spec)?;
    assign_keys(h, &mut requests);
    let converter = Converter { h, security: &security };
    let mut transform = |r: &Request, used: &mut HashSet<String>| converter.transform(r, used);
    let items = match group_by {
        GroupBy::Path => group_by_path(&requests, &mut transform)?,
        GroupBy::Tags => group_by_tags(h, &requests, &h.get(&spec, "tags"), &mut transform)?,
    };
    let auth = match security.supported.first() {
        Some(scheme) if scheme.truthy() => collection_auth(h, scheme)?,
        _ => auth_template("none"),
    };
    Ok(json!({
        "name": name,
        "version": "1",
        "items": items,
        "environments": environments,
        "root": {
            "request": { "auth": auth },
            "meta": { "name": name },
            "docs": to_spec_string(&h.get(&h.get(&spec, "info"), "description")),
        },
    }))
}

pub fn collection_name(h: &Heap, spec: &Js) -> R<String> {
    match h.get(&h.get(spec, "info"), "title") {
        Js::Undef | Js::Null => Ok("Untitled Collection".into()),
        Js::Str(s) if !trim(&s).is_empty() => Ok(trim(&s).to_owned()),
        Js::Str(_) => Ok("Untitled Collection".into()),
        _ => Err(type_error("info.title n'est pas une chaîne")),
    }
}

fn environments(h: &Heap, spec: &Js) -> R<Vec<Value>> {
    let servers = h.get(spec, "servers").or(h.arr(Vec::new()));
    let mut used = HashSet::new();
    let mut out = Vec::new();
    for (index, server) in h.array_strict(&servers, "servers")?.into_iter().enumerate() {
        let label = h.prop(&server, "name")?.or(h.get(&server, "description"));
        let mut name =
            if label.truthy() { normalize_item_name(&str_of(&label, "le nom du serveur")?) } else { String::new() };
        if name.is_empty() {
            name = format!("Environment {}", index + 1);
        }
        if used.contains(&name) {
            let mut counter = 2;
            while used.contains(&format!("{name} ({counter})")) {
                counter += 1;
            }
            name = format!("{name} ({counter})");
        }
        used.insert(name.clone());
        let mut variables = Vec::new();
        for (var, value) in server_vars(h, &server)? {
            variables.push(object(
                h,
                vec![
                    ("name", var),
                    ("value", value),
                    ("type", Js::str("text")),
                    ("enabled", Js::Bool(true)),
                    ("secret", Js::Bool(false)),
                ],
            )?);
        }
        out.push(json!({ "name": name, "variables": variables }));
    }
    Ok(out)
}

/// Port de `extractServerVars`.
fn server_vars(h: &Heap, server: &Js) -> R<Vec<(Js, Js)>> {
    let variables = h.prop(server, "variables")?;
    let url = h.get(server, "url");
    if variables.truthy() && !h.keys(&variables)?.is_empty() {
        let mut template = str_of(&url, "server.url")?.to_string();
        for (name, _) in h.each(&variables) {
            let name = h.to_string(&name);
            template = replace_all(&template, &format!("{{{name}}}"), &format!("{{{{{name}}}}}"));
        }
        let mut vars = vec![(Js::str("baseUrl"), Js::Str(without_trailing_slash(&template).into()))];
        for (name, variable) in h.each(&variables) {
            let default = h.prop(&variable, "default")?;
            let value = if !default.is_undef() {
                default
            } else {
                let enumeration = h.get(&variable, "enum");
                if enumeration.truthy() {
                    h.index(&enumeration, 0)
                } else {
                    Js::str("")
                }
            };
            vars.push((name, Js::Str(h.to_string(&value).into())));
        }
        return Ok(vars);
    }
    let url = str_of(&url, "server.url")?;
    Ok(vec![(Js::str("baseUrl"), Js::Str(without_trailing_slash(&url).into()))])
}

fn without_trailing_slash(url: &str) -> &str {
    url.strip_suffix('/').unwrap_or(url)
}

pub struct Security {
    pub schemes: Js,
    pub supported: Vec<Js>,
}

impl Security {
    fn new(h: &Heap, spec: &Js) -> R<Self> {
        let schemes = match h.path(spec, &["components", "securitySchemes"]) {
            Js::Undef => h.obj(Vec::new()),
            other => other,
        };
        let has_schemes = !h.keys(&schemes)?.is_empty();
        let defaults = h.get(spec, "security").or(h.arr(Vec::new()));
        let mut supported = Vec::new();
        if has_schemes {
            for requirement in h.array_strict(&defaults, "security")? {
                let scheme = h.get(&schemes, &first_key(h, &requirement)?);
                if scheme.truthy() {
                    supported.push(scheme);
                }
            }
        }
        Ok(Self { schemes, supported })
    }
}

/// `Object.keys(x)[0]` utilisé comme clé : `"undefined"` s'il n'y en a pas.
pub fn first_key(h: &Heap, v: &Js) -> R<Rc<str>> {
    Ok(h.keys(v)?.into_iter().next().unwrap_or_else(|| "undefined".into()))
}

fn requests(h: &Heap, spec: &Js) -> R<Vec<Request>> {
    let mut out = Vec::new();
    for (path, item) in h.entries(&h.get(spec, "paths"))? {
        let path_params = h.prop(&item, "parameters")?.or(h.arr(Vec::new()));
        for (method, op) in h.entries(&item)? {
            if !METHODS.contains(&method.to_lowercase().as_str()) {
                continue;
            }
            let variants = h.items(&h.prop(&op, "x-bruno-variants")?).unwrap_or_default();
            let operations = std::iter::once(op.clone())
                .chain(variants.into_iter().filter(|v| matches!(v, Js::Obj(_) | Js::Arr(_))));
            for operation in operations {
                let cleaned = h.spread(&operation, &[("x-bruno-variants", Js::Undef)]);
                let own = h.get(&cleaned, "parameters").or(h.arr(Vec::new()));
                let merged = merge_params(h, &path_params, &own)?;
                let servers = h.get(&cleaned, "servers").or(h.get(&item, "servers")).or(Js::Null);
                out.push(Request {
                    method: method.clone(),
                    path: colon_path(&path),
                    original_path: path.clone(),
                    op: h.spread(&cleaned, &[("parameters", merged)]),
                    servers,
                    key: String::new(),
                });
            }
        }
    }
    Ok(out)
}

/// `path.replace(/{([a-zA-Z]+)}/g, '{{<operationId>_$1}}')`
fn link_variables(path: &str, operation_id: &str) -> String {
    let input = utf16(path);
    let replacement = utf16(&format!("{{{{{operation_id}_$1}}}}"));
    let is_letter = |unit: &u16| u8::try_from(*unit).is_ok_and(|b| b.is_ascii_alphabetic());
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let letters =
            if input[i] == u16::from(b'{') { input[i + 1..].iter().take_while(|u| is_letter(u)).count() } else { 0 };
        if letters > 0 && input.get(i + letters + 1) == Some(&u16::from(b'}')) {
            expand_replacement(&mut out, &replacement, &input, i..i + letters + 2, &[&input[i + 1..=i + letters]]);
            i += letters + 2;
        } else {
            out.push(input[i]);
            i += 1;
        }
    }
    String::from_utf16_lossy(&out)
}

struct Converter<'a> {
    h: &'a Heap,
    security: &'a Security,
}

struct RequestBodyExample {
    key: Option<Rc<str>>,
    schema: Js,
    summary: Js,
    description: Js,
    content_type: Rc<str>,
}

impl Converter<'_> {
    fn transform(&self, request: &Request, used: &mut HashSet<String>) -> R<Item> {
        let h = self.h;
        let op = &request.op;
        let mut name = operation_name(h, op)?;
        if name.is_empty() {
            name = format!("{} {}", request.method, request.path);
        }
        let name = unique_name(name, &request.method, used);
        let path = link_variables(&request.path, &h.to_string(&h.get(op, "operationId")));
        let tags = sanitize_tags(h, &h.get(op, "tags").or(h.arr(Vec::new())))?;
        let docs = to_spec_string(&h.get(op, "description"));
        let mut draft = Draft {
            url: ensure_url(&format!("{{{{baseUrl}}}}{path}")),
            method: request.method.to_uppercase(),
            headers: Vec::new(),
            params: Vec::new(),
            body: Body::none(),
        };
        let mut auth = auth_template("inherit");

        let mut vars = None;
        if request.servers.truthy() && h.gt_zero(&h.get(&request.servers, "length")) {
            let mut list = Vec::new();
            for (var, value) in server_vars(h, &h.index(&request.servers, 0))? {
                list.push(object(
                    h,
                    vec![("name", var), ("value", value), ("enabled", Js::Bool(true)), ("local", Js::Bool(false))],
                )?);
            }
            vars = Some(list);
        }

        self.parameters(op, &mut draft)?;
        self.security(op, &mut auth, &mut draft)?;
        self.request_body(op, &mut draft.body)?;
        let script = self.link_script(op)?;
        let examples = self.examples(op, &draft)?;

        let mut req = Map::new();
        req.insert("docs".into(), json!(docs));
        req.insert("url".into(), json!(draft.url));
        req.insert("method".into(), json!(draft.method));
        req.insert("auth".into(), Value::Object(auth));
        req.insert("headers".into(), Value::Array(draft.headers));
        req.insert("params".into(), Value::Array(draft.params));
        req.insert("body".into(), draft.body.to_value());
        req.insert("script".into(), json!({ "res": script }));
        if let Some(list) = vars {
            req.insert("vars".into(), json!({ "req": list, "res": [] }));
        }
        let mut fields = Map::new();
        fields.insert("name".into(), json!(name));
        fields.insert("type".into(), json!("http-request"));
        fields.insert("settings".into(), json!({ "forwardAuthorizationHeader": false }));
        fields.insert("tags".into(), json!(tags));
        fields.insert("request".into(), Value::Object(req));
        if !examples.is_empty() {
            fields.insert("examples".into(), Value::Array(examples));
        }
        Ok(Item { fields, key: request.key.clone() })
    }

    fn parameters(&self, op: &Js, draft: &mut Draft) -> R<()> {
        let h = self.h;
        for (_, param) in h.each(&h.get(op, "parameters").or(h.arr(Vec::new()))) {
            let schema = h.prop(&param, "schema")?;
            let location = h.get(&param, "in");
            if schema.truthy() && h.get(&schema, "properties").truthy() {
                let schema_example = h.get(&schema, "example").or(h.obj(Vec::new()));
                let required = h.get(&schema, "required");
                for (prop_name, prop) in h.each(&h.get(&schema, "properties")) {
                    let is_required = h.items(&required).is_some_and(|r| r.iter().any(|x| x.strict_eq(&prop_name)));
                    let parent_example = h.get(&schema_example, &h.to_string(&prop_name));
                    let prop_schema = if h.prop(&prop, "example")?.is_undef() && !parent_example.is_undef() {
                        h.spread(&prop, &[("example", parent_example)])
                    } else {
                        prop.clone()
                    };
                    let temp = h.spread(
                        &param,
                        &[
                            ("example", Js::Undef),
                            ("examples", Js::Undef),
                            ("name", prop_name.clone()),
                            ("schema", prop_schema),
                            ("required", Js::Bool(is_required)),
                        ],
                    );
                    let description = h.get(&prop, "description").or(Js::str(""));
                    for (value, enabled) in parameter_entries(h, &temp)? {
                        push_param(h, draft, &location, prop_name.clone(), value, description.clone(), enabled)?;
                    }
                }
            } else {
                let description = h.get(&param, "description").or(Js::str(""));
                for (value, enabled) in parameter_entries(h, &param)? {
                    push_param(h, draft, &location, h.get(&param, "name"), value, description.clone(), enabled)?;
                }
            }
        }
        Ok(())
    }

    fn security(&self, op: &Js, auth: &mut Map<String, Value>, draft: &mut Draft) -> R<()> {
        let h = self.h;
        let requirements = h.get(op, "security");
        if !(requirements.truthy() && h.gt_zero(&h.get(&requirements, "length"))) {
            return Ok(());
        }
        let scheme = h.get(&self.security.schemes, &first_key(h, &h.index(&requirements, 0))?);
        if !scheme.truthy() {
            return Ok(());
        }
        if let Some((mode, value)) = scheme_auth(h, &scheme)? {
            auth.insert("mode".into(), json!(mode));
            auth.insert(mode.into(), value);
        }
        if h.get(&scheme, "type").is_str("apiKey") {
            let location = h.get(&scheme, "in");
            let entry = vec![
                ("name", h.get(&scheme, "name")),
                ("value", Js::str("{{apiKey}}")),
                ("description", h.get(&scheme, "description").or(Js::str(""))),
                ("enabled", Js::Bool(true)),
            ];
            if location.is_str("header") || location.is_str("cookie") {
                draft.headers.push(object(h, entry)?);
            } else if location.is_str("query") {
                let mut entry = entry;
                entry.push(("type", Js::str("query")));
                draft.params.push(object(h, entry)?);
            }
        }
        Ok(())
    }

    fn request_body(&self, op: &Js, body: &mut Body) -> R<()> {
        let h = self.h;
        let request_body = h.get(op, "requestBody");
        if !request_body.truthy() {
            return Ok(());
        }
        let content = match h.get(&request_body, "content") {
            Js::Undef => h.obj(Vec::new()),
            other => other,
        };
        let mime = h.keys(&content)?.into_iter().next();
        let body_content = h.get(&content, mime.as_deref().unwrap_or("undefined")).or(h.obj(Vec::new()));
        let mut schema = h.get(&body_content, "schema");
        if h.get(&schema, "example").is_undef() {
            let example = content_level_example(h, &body_content)?;
            if !example.is_undef() {
                schema = h.spread(&schema, &[("example", example)]);
            }
        }
        let normalized = mime.map(|m| m.to_lowercase()).unwrap_or_default();
        if let Some(handler) = Handler::find(&normalized) {
            body.mode = handler.mode();
            handle_body(h, handler, body, &schema)?;
        }
        Ok(())
    }

    fn link_script(&self, op: &Js) -> R<Option<String>> {
        let h = self.h;
        let mut script = Vec::new();
        for (status, response) in h.each(&h.get(op, "responses").or(h.arr(Vec::new()))) {
            if !h.has_own(&response, "links")? {
                continue;
            }
            script.push(format!("if (res.status === {}) {{", h.to_string(&status)));
            for (_, link) in h.each(&h.get(&response, "links")) {
                let operation_id = h.to_string(&h.prop(&link, "operationId")?);
                for (parameter, expression) in h.each(&h.get(&link, "parameters").or(h.arr(Vec::new()))) {
                    let value = runtime_expression(&expression)?;
                    script.push(format!("  bru.setVar('{operation_id}_{}', {value});", h.to_string(&parameter)));
                }
            }
            script.push("}".into());
        }
        Ok((!script.is_empty()).then(|| script.join("\n")))
    }

    fn examples(&self, op: &Js, draft: &Draft) -> R<Vec<Value>> {
        let h = self.h;
        let responses = h.get(op, "responses");
        if !responses.truthy() {
            return Ok(Vec::new());
        }
        let request_examples = self.request_body_examples(op)?;
        let mut out = Vec::new();
        for (status, response) in h.entries(&responses)? {
            let content = h.prop(&response, "content")?;
            let description = h.get(&response, "description").or(Js::str(""));
            let default_name = Js::Str(format!("{status} Response").into());
            if !content.truthy() {
                let ex = Example {
                    value: Js::str(""),
                    name: default_name,
                    description,
                    status,
                    content_type: Js::Null,
                    request_body: None,
                };
                self.with_request_bodies(draft, ex, None, &request_examples, &mut out)?;
                continue;
            }
            for (content_type, media) in h.entries(&content)? {
                let named = h.prop(&media, "examples")?;
                let base = |value, name, description| Example {
                    value,
                    name,
                    description,
                    status: status.clone(),
                    content_type: Js::Str(content_type.clone()),
                    request_body: None,
                };
                if named.truthy() {
                    for (key, example) in h.entries(&named)? {
                        let name = h.prop(&example, "summary")?.or(Js::Str(key.clone())).or(default_name.clone());
                        let description = h.get(&example, "description").or(Js::str(""));
                        let value = h.get(&example, "value");
                        let value = if value.is_undef() { example.clone() } else { value };
                        self.with_request_bodies(
                            draft,
                            base(value, name, description),
                            Some(&key),
                            &request_examples,
                            &mut out,
                        )?;
                    }
                } else if !h.get(&media, "example").is_undef() {
                    let ex = base(h.get(&media, "example"), default_name.clone(), description.clone());
                    self.with_request_bodies(draft, ex, None, &request_examples, &mut out)?;
                } else if h.get(&media, "schema").truthy() {
                    let value = example_from_schema(h, &h.get(&media, "schema"))?;
                    let ex = base(value, default_name.clone(), description.clone());
                    self.with_request_bodies(draft, ex, None, &request_examples, &mut out)?;
                }
            }
        }
        Ok(out)
    }

    fn request_body_examples(&self, op: &Js) -> R<Vec<RequestBodyExample>> {
        let h = self.h;
        let request_body = h.get(op, "requestBody");
        let content = h.get(&request_body, "content");
        let mut out = Vec::new();
        if !(request_body.truthy() && content.truthy()) {
            return Ok(out);
        }
        for (content_type, media) in h.entries(&content)? {
            let named = h.prop(&media, "examples")?;
            if named.truthy() {
                for (key, example) in h.entries(&named)? {
                    let value = h.prop(&example, "value")?;
                    let value = if value.is_undef() { example.clone() } else { value };
                    out.push(RequestBodyExample {
                        key: Some(key),
                        schema: h.obj(vec![("example".into(), value)]),
                        summary: h.get(&example, "summary"),
                        description: h.get(&example, "description"),
                        content_type: content_type.clone(),
                    });
                }
            } else if !h.get(&media, "example").is_undef() {
                out.push(RequestBodyExample {
                    key: None,
                    schema: h.obj(vec![("example".into(), h.get(&media, "example"))]),
                    summary: Js::Null,
                    description: Js::Null,
                    content_type,
                });
            } else if h.get(&media, "schema").truthy() {
                out.push(RequestBodyExample {
                    key: None,
                    schema: h.get(&media, "schema"),
                    summary: Js::Null,
                    description: Js::Null,
                    content_type,
                });
            }
        }
        Ok(out)
    }

    /// Port de `createExamplesWithRequestBody`.
    fn with_request_bodies(
        &self,
        draft: &Draft,
        ex: Example,
        response_key: Option<&str>,
        request_examples: &[RequestBodyExample],
        out: &mut Vec<Value>,
    ) -> R<()> {
        let h = self.h;
        let keyed: Vec<_> = request_examples.iter().filter(|rb| rb.key.is_some()).collect();
        let request_body = |rb: &RequestBodyExample| Some((rb.schema.clone(), Js::Str(rb.content_type.clone())));
        let matching =
            response_key.filter(|k| !k.is_empty()).and_then(|k| keyed.iter().find(|rb| rb.key.as_deref() == Some(k)));
        if let Some(rb) = matching {
            out.push(create_example(h, draft, Example { request_body: request_body(rb), ..ex })?);
        } else if !keyed.is_empty() {
            for rb in keyed {
                let label = rb.summary.clone().or(rb.key.clone().map_or(Js::Undef, Js::Str));
                let name = Js::Str(format!("{} ({})", h.to_string(&ex.name), h.to_string(&label)).into());
                let description = ex.description.clone().or(rb.description.clone()).or(Js::str(""));
                let combined = Example {
                    value: ex.value.clone(),
                    name,
                    description,
                    status: ex.status.clone(),
                    content_type: ex.content_type.clone(),
                    request_body: request_body(rb),
                };
                out.push(create_example(h, draft, combined)?);
            }
        } else if let Some(rb) = request_examples.iter().find(|rb| rb.key.is_none()) {
            out.push(create_example(h, draft, Example { request_body: request_body(rb), ..ex })?);
        } else {
            out.push(create_example(h, draft, ex)?);
        }
        Ok(())
    }
}

fn push_param(
    h: &Heap,
    draft: &mut Draft,
    location: &Js,
    name: Js,
    value: String,
    description: Js,
    enabled: Js,
) -> R<()> {
    let entry =
        vec![("name", name), ("value", Js::Str(value.into())), ("description", description), ("enabled", enabled)];
    let kind = if location.is_str("query") || location.is_str("querystring") {
        "query"
    } else if location.is_str("path") {
        "path"
    } else {
        if location.is_str("header") {
            draft.headers.push(object(h, entry)?);
        }
        return Ok(());
    };
    let mut entry = entry;
    entry.push(("type", Js::str(kind)));
    draft.params.push(object(h, entry)?);
    Ok(())
}

fn content_level_example(h: &Heap, content: &Js) -> R<Js> {
    let example = h.get(content, "example");
    if !example.is_undef() {
        return Ok(example);
    }
    let examples = h.get(content, "examples").coalesce(h.obj(Vec::new()));
    let first = h.values(&examples)?.into_iter().next().unwrap_or(Js::Undef);
    Ok(h.get(&first, "value"))
}

/// Port de `getParameterEntries` (OpenAPI 3) : couples (valeur, activé).
fn parameter_entries(h: &Heap, param: &Js) -> R<Vec<(String, Js)>> {
    let schema = h.get(param, "schema").or(h.obj(Vec::new()));
    let required = h.get(param, "required");
    let enumeration = h.get(&schema, "enum");
    if enumeration.is_array() && h.gt_zero(&h.get(&enumeration, "length")) {
        return Ok(enum_entries(h, &enumeration, &h.get(&schema, "default"), &required));
    }
    let kind = h.get(&schema, "type");
    let items = h.get(&schema, "items");
    let item_enum = h.get(&items, "enum");
    if kind.is_str("array") && items.truthy() && item_enum.is_array() && h.gt_zero(&h.get(&item_enum, "length")) {
        let array_default = h.get(&schema, "default");
        if array_default.is_array() {
            return Ok(vec![(h.stringify(&array_default, false)?.unwrap_or_default(), Js::Bool(true))]);
        }
        return Ok(enum_entries(h, &item_enum, &h.get(&items, "default"), &required));
    }

    let mut value = String::new();
    let mut enabled = required.clone().or(Js::Bool(false));
    let example = h.get(param, "example");
    if !example.is_undef() {
        value = h.to_string(&example);
        enabled = Js::Bool(true);
    } else {
        let examples = h.get(param, "examples");
        if examples.truthy() {
            let first = h.values(&examples)?.into_iter().next().unwrap_or(Js::Undef);
            let first_value = h.get(&first, "value");
            if !first_value.is_undef() {
                value = h.to_string(&first_value);
                enabled = Js::Bool(true);
            }
        }
    }
    let default = h.get(&schema, "default");
    if value.is_empty() && !default.is_undef() {
        value = if kind.is_str("array") && default.is_array() {
            h.stringify(&default, false)?.unwrap_or_default()
        } else {
            h.to_string(&default)
        };
        enabled = Js::Bool(true);
    }
    let schema_example = h.get(&schema, "example");
    if value.is_empty() && !schema_example.is_undef() {
        value = h.to_string(&schema_example);
        enabled = Js::Bool(true);
    }
    if value.is_empty() && kind.is_str("array") && items.truthy() {
        let item_example = h.get(&items, "example");
        let item_default = h.get(&items, "default");
        value = if !item_example.is_undef() {
            h.to_string(&item_example)
        } else if item_enum.truthy() && h.gt_zero(&h.get(&item_enum, "length")) {
            h.to_string(&h.index(&item_enum, 0))
        } else if !item_default.is_undef() {
            h.to_string(&item_default)
        } else {
            "[]".into()
        };
        enabled = required.clone().or(Js::Bool(false));
    }
    let schema_examples = h.get(&schema, "examples");
    if value.is_empty() && schema_examples.is_array() && h.gt_zero(&h.get(&schema_examples, "length")) {
        value = h.to_string(&h.index(&schema_examples, 0));
        enabled = Js::Bool(true);
    }
    let minimum = h.get(&schema, "minimum");
    if value.is_empty() && !minimum.is_undef() {
        value = h.to_string(&minimum);
        enabled = required.clone().or(Js::Bool(false));
    }
    if value.is_empty() {
        let nullable = h.get(&schema, "nullable").strict_eq(&Js::Bool(true));
        let allow_empty = h.get(param, "allowEmptyValue").strict_eq(&Js::Bool(true));
        if (nullable || allow_empty) && !required.truthy() {
            enabled = Js::Bool(false);
        }
    }
    Ok(vec![(value, enabled)])
}

fn enum_entries(h: &Heap, enumeration: &Js, default: &Js, required: &Js) -> Vec<(String, Js)> {
    let default = (!default.is_undef()).then(|| h.to_string(default));
    let values = h.items(enumeration).unwrap_or_default();
    values
        .iter()
        .map(|v| {
            let text = h.to_string(v);
            let is_default = default.as_ref().is_some_and(|d| *d == text);
            let first = values.iter().position(|x| x.strict_eq(v)) == Some(0);
            (text, Js::Bool(is_default || (default.is_none() && first && required.truthy())))
        })
        .collect()
}

/// Authentification d'un schéma de sécurité : `(mode, configuration)`.
fn scheme_auth(h: &Heap, scheme: &Js) -> R<Option<(&'static str, Value)>> {
    let kind = h.get(scheme, "type");
    let http = h.get(scheme, "scheme");
    let credentials = json!({ "username": "{{username}}", "password": "{{password}}" });
    Ok(Some(if kind.is_str("http") && http.is_str("basic") {
        ("basic", credentials)
    } else if kind.is_str("http") && http.is_str("bearer") {
        ("bearer", json!({ "token": "{{token}}" }))
    } else if kind.is_str("http") && http.is_str("digest") {
        ("digest", credentials)
    } else if kind.is_str("apiKey") {
        let placement = if h.get(scheme, "in").is_str("query") { "queryparams" } else { "header" };
        (
            "apikey",
            object(
                h,
                vec![
                    ("key", h.get(scheme, "name")),
                    ("value", Js::str("{{apiKey}}")),
                    ("placement", Js::str(placement)),
                ],
            )?,
        )
    } else if kind.is_str("oauth2") {
        ("oauth2", oauth2(h, scheme)?)
    } else {
        return Ok(None);
    }))
}

fn oauth2(h: &Heap, scheme: &Js) -> R<Value> {
    let flows = h.get(scheme, "flows").or(h.obj(Vec::new()));
    let (grant, flow) = [
        ("authorization_code", "authorizationCode"),
        ("implicit", "implicit"),
        ("password", "password"),
        ("client_credentials", "clientCredentials"),
    ]
    .into_iter()
    .find(|(_, flow)| h.get(&flows, flow).truthy())
    .unwrap_or(("client_credentials", "clientCredentials"));
    let config = h.get(&flows, flow).or(h.obj(Vec::new()));
    let scopes = h.get(&config, "scopes");
    let scope = match h.items(&scopes) {
        Some(list) => h.join(&list, " "),
        None => h.keys(&scopes.or(h.obj(Vec::new())))?.join(" "),
    };
    object(
        h,
        vec![
            ("grantType", Js::str(grant)),
            ("authorizationUrl", h.get(&config, "authorizationUrl").or(Js::str("{{oauth_authorize_url}}"))),
            ("accessTokenUrl", h.get(&config, "tokenUrl").or(Js::str("{{oauth_token_url}}"))),
            ("refreshTokenUrl", h.get(&config, "refreshUrl").or(Js::str("{{oauth_refresh_url}}"))),
            ("callbackUrl", Js::str("{{oauth_callback_url}}")),
            ("clientId", Js::str("{{oauth_client_id}}")),
            ("clientSecret", Js::str("{{oauth_client_secret}}")),
            ("scope", Js::Str(scope.into())),
            ("state", Js::str("{{oauth_state}}")),
            ("credentialsPlacement", Js::str("header")),
            ("tokenPlacement", Js::str("header")),
            ("tokenHeaderPrefix", Js::str("Bearer")),
            ("autoFetchToken", Js::Bool(false)),
            ("autoRefreshToken", Js::Bool(true)),
        ],
    )
}

fn collection_auth(h: &Heap, scheme: &Js) -> R<Map<String, Value>> {
    let mut auth = auth_template("none");
    if let Some((mode, value)) = scheme_auth(h, scheme)? {
        auth.insert("mode".into(), json!(mode));
        auth.insert(mode.into(), value);
    }
    Ok(auth)
}

/// Port de `openAPIRuntimeExpressionToScript`.
fn runtime_expression(expression: &Js) -> R<String> {
    let expression = str_of(expression, "l'expression de lien")?;
    if &*expression == "$response.body" {
        return Ok("res.body".into());
    }
    if let Some(pointer) = expression.strip_prefix("$response.body#") {
        return Ok(format!("res.body{}", pointer.replacen('/', ".", 1)));
    }
    Ok(expression.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ef_imp_02_link_variables_match_the_second_replace_of_bruno() {
        assert_eq!(link_variables("/a/:id", "op"), "/a/:id");
        assert_eq!(link_variables("/a/:{x}/{1}", "get$&"), "/a/:{{get{x}_x}}/{1}");
    }
}
