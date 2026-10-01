//! Port de `swagger2-to-bruno.js` (Swagger 2.0).

use std::collections::HashSet;

use serde_json::{json, Map, Value};

use super::common::{
    assign_keys, auth_template, colon_path, create_example, ensure_url, example_from_schema, group_by_path,
    group_by_tags, handle_body, merge_params, object, operation_name, sanitize_tags, str_of, to_spec_string,
    unique_name, Body, Draft, Example, Handler, Item, Request,
};
use super::heap::{type_error, Heap, Js};
use super::resolve::{resolve, Flavor};
use super::v3::{collection_name, first_key};
use super::{GroupBy, R};

pub const METHODS: [&str; 7] = ["get", "put", "post", "delete", "options", "head", "patch"];

pub fn convert(h: &Heap, data: &Value, group_by: GroupBy) -> R<Value> {
    let spec = resolve(h, data, Flavor::Swagger)?;
    if !spec.truthy() {
        return Err(type_error("Invalid Swagger 2.0 specification. Failed to resolve refs."));
    }
    let name = collection_name(h, &spec)?;
    let urls = server_urls(h, &spec)?;
    let environments: Vec<Value> = urls
        .iter()
        .enumerate()
        .map(|(i, url)| {
            let name = if urls.len() > 1 { format!("Environment {}", i + 1) } else { "Environment".into() };
            json!({
                "name": name,
                "variables": [{ "name": "baseUrl", "value": url, "type": "text", "enabled": true, "secret": false }],
            })
        })
        .collect();
    let definitions = h.get(&spec, "securityDefinitions").or(h.obj(Vec::new()));
    let supported = supported_definitions(h, &spec, &definitions)?;
    let mut requests = requests(h, &spec)?;
    assign_keys(h, &mut requests);
    let converter = Converter {
        h,
        definitions: &definitions,
        consumes: h.get(&spec, "consumes"),
        produces: h.get(&spec, "produces"),
    };
    let mut transform = |r: &Request, used: &mut HashSet<String>| converter.transform(r, used);
    let items = match group_by {
        GroupBy::Path => group_by_path(&requests, &mut transform)?,
        GroupBy::Tags => group_by_tags(h, &requests, &h.get(&spec, "tags"), &mut transform)?,
    };
    let mut auth = auth_template("none");
    if let Some(def) = supported.first() {
        if let Some((mode, value)) = definition_auth(h, def, &Js::Undef)? {
            auth.insert("mode".into(), json!(mode));
            auth.insert(mode.into(), value);
        }
    }
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

/// Port de `buildServerUrls`.
pub fn server_urls(h: &Heap, spec: &Js) -> R<Vec<String>> {
    let host = h.get(spec, "host").or(Js::str(""));
    let base_path = h.get(spec, "basePath").or(Js::str(""));
    if !host.truthy() && !base_path.truthy() {
        return Ok(Vec::new());
    }
    if !host.truthy() {
        return Ok(vec![str_of(&base_path, "basePath")?.trim_end_matches('/').to_owned()]);
    }
    let schemes = h.get(spec, "schemes");
    let schemes = if schemes.truthy() && h.get(&schemes, "length").truthy() {
        h.array_strict(&schemes, "schemes")?
    } else {
        vec![Js::str("https")]
    };
    let (host, base_path) = (h.to_string(&host), h.to_string(&base_path));
    Ok(schemes
        .iter()
        .map(|s| format!("{}://{host}{base_path}", h.to_string(s)).trim_end_matches('/').to_owned())
        .collect())
}

fn supported_definitions(h: &Heap, spec: &Js, definitions: &Js) -> R<Vec<Js>> {
    if h.keys(definitions)?.is_empty() {
        return Ok(Vec::new());
    }
    let defaults = h.get(spec, "security").or(h.arr(Vec::new()));
    let mut out = Vec::new();
    for requirement in h.array_strict(&defaults, "security")? {
        let def = h.get(definitions, &first_key(h, &requirement)?);
        if def.truthy() {
            out.push(def);
        }
    }
    Ok(out)
}

fn requests(h: &Heap, spec: &Js) -> R<Vec<Request>> {
    let mut out = Vec::new();
    for (path, item) in h.entries(&h.get(spec, "paths").or(h.obj(Vec::new())))? {
        let path_params = h.prop(&item, "parameters")?.or(h.arr(Vec::new()));
        for (method, op) in h.entries(&item)? {
            if !METHODS.contains(&method.to_lowercase().as_str()) {
                continue;
            }
            let own = h.prop(&op, "parameters")?.or(h.arr(Vec::new()));
            let merged = merge_params(h, &path_params, &own)?;
            out.push(Request {
                method: method.clone(),
                path: colon_path(&path),
                original_path: path.clone(),
                op: h.spread(&op, &[("parameters", merged)]),
                servers: Js::Null,
                key: String::new(),
            });
        }
    }
    Ok(out)
}

struct Converter<'a> {
    h: &'a Heap,
    definitions: &'a Js,
    consumes: Js,
    produces: Js,
}

impl Converter<'_> {
    fn transform(&self, request: &Request, used: &mut HashSet<String>) -> R<Item> {
        let h = self.h;
        let op = &request.op;
        let consumes = h.get(op, "consumes").or(self.consumes.clone()).or(h.arr(vec![Js::str("application/json")]));
        let produces = h.get(op, "produces").or(self.produces.clone()).or(h.arr(vec![Js::str("application/json")]));
        let mut name = operation_name(h, op)?;
        if name.is_empty() {
            name = format!("{} {}", request.method, request.path);
        }
        let name = unique_name(name, &request.method, used);
        let tags = sanitize_tags(h, &h.get(op, "tags").or(h.arr(Vec::new())))?;
        let docs = to_spec_string(&h.get(op, "description"));
        let mut draft = Draft {
            url: ensure_url(&format!("{{{{baseUrl}}}}{}", request.path)),
            method: request.method.to_uppercase(),
            headers: Vec::new(),
            params: Vec::new(),
            body: Body::none(),
        };
        let mut auth = auth_template("inherit");

        let mut body_param = None;
        let mut form_params = Vec::new();
        for (_, param) in h.each(&h.get(op, "parameters").or(h.arr(Vec::new()))) {
            let location = h.prop(&param, "in")?;
            if location.is_str("body") {
                body_param = Some(param);
            } else if location.is_str("formData") {
                form_params.push(param);
            } else if location.is_str("query") || location.is_str("path") || location.is_str("header") {
                self.parameter(&param, &location, &mut draft)?;
            }
        }

        let body_schema = body_param.as_ref().map(|p| h.get(p, "schema")).filter(Js::truthy);
        let body_type = || -> R<Js> { Ok(h.index(&consumes, 0).or(Js::str("application/json"))) };
        if let Some(schema) = &body_schema {
            let mime = str_of(&body_type()?, "le type de contenu")?.to_lowercase();
            if let Some(handler) = Handler::find(&mime) {
                draft.body.mode = handler.mode();
                handle_body(h, handler, &mut draft.body, schema)?;
            }
        }
        if body_param.is_none() && !form_params.is_empty() {
            self.form_data(&form_params, &consumes, &mut draft.body)?;
        }

        let requirements = h.get(op, "security");
        if requirements.is_array() && !h.gt_zero(&h.get(&requirements, "length")) {
            auth.insert("mode".into(), json!("none"));
        }
        if requirements.truthy() && h.gt_zero(&h.get(&requirements, "length")) {
            let first = h.index(&requirements, 0);
            let scheme_name = first_key(h, &first)?;
            let scopes = h.get(&first, &scheme_name);
            let def = h.get(self.definitions, &scheme_name);
            if def.truthy() {
                self.apply_auth(&def, &scopes, &mut auth, &mut draft)?;
            }
        }

        let request_body = match (&body_schema, &body_param) {
            (Some(schema), _) => Some((schema.clone(), body_type()?)),
            _ => None,
        };
        let examples = self.examples(op, &produces, &draft, request_body)?;

        let mut req = Map::new();
        req.insert("docs".into(), json!(docs));
        req.insert("url".into(), json!(draft.url));
        req.insert("method".into(), json!(draft.method));
        req.insert("auth".into(), Value::Object(auth));
        req.insert("headers".into(), Value::Array(draft.headers));
        req.insert("params".into(), Value::Array(draft.params));
        req.insert("body".into(), draft.body.to_value());
        req.insert("script".into(), json!({ "res": null }));
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

    fn parameter(&self, param: &Js, location: &Js, draft: &mut Draft) -> R<()> {
        let h = self.h;
        let schema = h.get(param, "schema");
        let inline = h.get(param, "properties");
        let nested = h.get(&schema, "properties");
        let is_object =
            (h.get(param, "type").is_str("object") && inline.truthy()) || (schema.truthy() && nested.truthy());
        if !is_object {
            let description = h.get(param, "description").or(Js::str(""));
            for (value, enabled) in parameter_entries(h, param)? {
                push_param(h, draft, location, h.get(param, "name"), value, description.clone(), enabled)?;
            }
            return Ok(());
        }
        let properties = inline.or(nested).or(h.obj(Vec::new()));
        let required = h.get(param, "required").or(h.get(&schema, "required")).or(h.arr(Vec::new()));
        let example = h.get(&schema, "example").or(h.get(param, "example")).or(h.obj(Vec::new()));
        for (prop_name, prop) in h.each(&properties) {
            let is_required = h.items(&required).is_some_and(|r| r.iter().any(|x| x.strict_eq(&prop_name)));
            let parent_example = h.get(&example, &h.to_string(&prop_name));
            let with_example = if h.prop(&prop, "example")?.is_undef() && !parent_example.is_undef() {
                h.spread(&prop, &[("example", parent_example)])
            } else {
                prop.clone()
            };
            let temp = h.spread(
                &with_example,
                &[("name", prop_name.clone()), ("in", location.clone()), ("required", Js::Bool(is_required))],
            );
            let description = h.get(&prop, "description").or(Js::str(""));
            for (value, enabled) in parameter_entries(h, &temp)? {
                push_param(h, draft, location, prop_name.clone(), value, description.clone(), enabled)?;
            }
        }
        Ok(())
    }

    fn form_data(&self, params: &[Js], consumes: &Js, body: &mut Body) -> R<()> {
        let h = self.h;
        let has_file = params.iter().any(|p| h.get(p, "type").is_str("file"));
        let multipart = has_file || includes(h, consumes, "multipart/form-data")?;
        body.mode = if multipart { "multipartForm" } else { "formUrlEncoded" };
        for param in params {
            let is_file = h.get(param, "type").is_str("file");
            let value = Js::Str(parameter_value(h, param).into());
            let description = h.get(param, "description").or(Js::str(""));
            if multipart {
                let (kind, value) = if is_file { ("file", h.arr(Vec::new())) } else { ("text", value) };
                body.multipart_form.push(object(
                    h,
                    vec![
                        ("type", Js::str(kind)),
                        ("name", h.get(param, "name")),
                        ("value", value),
                        ("description", description),
                        ("enabled", Js::Bool(true)),
                    ],
                )?);
            } else {
                body.form_url_encoded.push(object(
                    h,
                    vec![
                        ("name", h.get(param, "name")),
                        ("value", value),
                        ("description", description),
                        ("enabled", Js::Bool(true)),
                    ],
                )?);
            }
        }
        Ok(())
    }

    fn apply_auth(&self, def: &Js, scopes: &Js, auth: &mut Map<String, Value>, draft: &mut Draft) -> R<()> {
        let h = self.h;
        let Some((mode, value)) = definition_auth(h, def, scopes)? else { return Ok(()) };
        auth.insert("mode".into(), json!(mode));
        auth.insert(mode.into(), value);
        if mode == "apikey" {
            let location = h.get(def, "in");
            let entry = vec![
                ("name", h.get(def, "name")),
                ("value", Js::str("{{apiKey}}")),
                ("description", h.get(def, "description").or(Js::str(""))),
                ("enabled", Js::Bool(true)),
            ];
            if location.is_str("header") {
                draft.headers.push(object(h, entry)?);
            } else if location.is_str("query") {
                let mut entry = entry;
                entry.push(("type", Js::str("query")));
                draft.params.push(object(h, entry)?);
            }
        }
        Ok(())
    }

    fn examples(&self, op: &Js, produces: &Js, draft: &Draft, request_body: Option<(Js, Js)>) -> R<Vec<Value>> {
        let h = self.h;
        let responses = h.get(op, "responses");
        if !responses.truthy() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for (status, response) in h.entries(&responses)? {
            if &*status == "default" {
                continue;
            }
            let content_type = h.index(produces, 0).or(Js::str("application/json"));
            let description = h.get(&response, "description").or(Js::str(""));
            let named = h.prop(&response, "examples")?;
            let example = |value, content_type| Example {
                value,
                name: Js::Str(format!("{status} Response").into()),
                description: description.clone(),
                status: status.clone(),
                content_type,
                request_body: request_body.clone(),
            };
            if named.truthy() {
                for (mime, value) in h.entries(&named)? {
                    out.push(create_example(h, draft, example(value, Js::Str(mime)))?);
                }
            } else if h.get(&response, "schema").truthy() {
                let value = example_from_schema(h, &h.get(&response, "schema"))?;
                out.push(create_example(h, draft, example(value, content_type))?);
            } else if h.get(&response, "description").truthy() {
                out.push(create_example(h, draft, example(Js::str(""), Js::Null))?);
            }
        }
        Ok(out)
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
    let mut entry =
        vec![("name", name), ("value", Js::Str(value.into())), ("description", description), ("enabled", enabled)];
    if location.is_str("header") {
        draft.headers.push(object(h, entry)?);
    } else if location.is_str("query") || location.is_str("path") {
        entry.push(("type", location.clone()));
        draft.params.push(object(h, entry)?);
    }
    Ok(())
}

/// `x.includes(needle)` sur un tableau ou une chaîne.
fn includes(h: &Heap, haystack: &Js, needle: &str) -> R<bool> {
    match haystack {
        Js::Str(s) => Ok(s.contains(needle)),
        _ => Ok(h.array_strict(haystack, "consumes")?.iter().any(|x| x.is_str(needle))),
    }
}

/// Port de `getParameterValue`.
fn parameter_value(h: &Heap, param: &Js) -> String {
    let example = h.get(param, "example");
    if !example.is_undef() {
        return h.to_string(&example);
    }
    let default = h.get(param, "default");
    if !default.is_undef() {
        return h.to_string(&default);
    }
    let enumeration = h.get(param, "enum");
    if enumeration.truthy() && h.gt_zero(&h.get(&enumeration, "length")) {
        return h.to_string(&h.index(&enumeration, 0));
    }
    String::new()
}

/// Port de `getParameterEntries` (Swagger 2) : couples (valeur, activé).
fn parameter_entries(h: &Heap, param: &Js) -> R<Vec<(String, Js)>> {
    let required = h.get(param, "required");
    let enumeration = h.get(param, "enum");
    if enumeration.is_array() && h.gt_zero(&h.get(&enumeration, "length")) {
        return Ok(enum_entries(h, &enumeration, &h.get(param, "default"), &required));
    }
    let items = h.get(param, "items");
    let item_enum = h.get(&items, "enum");
    if h.get(param, "type").is_str("array")
        && items.truthy()
        && item_enum.truthy()
        && h.gt_zero(&h.get(&item_enum, "length"))
    {
        let format = h.get(param, "collectionFormat").or(Js::str("csv"));
        let separator = match format.as_str() {
            Some("pipes") => "|",
            Some("ssv") => " ",
            Some("tsv") => "\t",
            _ => ",",
        };
        let multi = format.is_str("multi");
        let default = h.get(param, "default");
        if let Some(values) = h.items(&default) {
            let values: Vec<String> = values.iter().map(|v| h.to_string(v)).collect();
            return Ok(if multi {
                values.into_iter().map(|v| (v, Js::Bool(true))).collect()
            } else {
                vec![(values.join(separator), Js::Bool(true))]
            });
        }
        let values = h.array_strict(&item_enum, "items.enum")?;
        if multi {
            return Ok(enum_entries(h, &item_enum, &h.get(&items, "default"), &required));
        }
        let joined = values.iter().map(|v| h.to_string(v)).collect::<Vec<_>>().join(separator);
        return Ok(vec![(joined, required.or(Js::Bool(false)))]);
    }
    let value = parameter_value(h, param);
    let enabled = if value.is_empty() { required.or(Js::Bool(false)) } else { Js::Bool(true) };
    Ok(vec![(value, enabled)])
}

fn enum_entries(h: &Heap, enumeration: &Js, default: &Js, required: &Js) -> Vec<(String, Js)> {
    let default = (!default.is_undef()).then(|| h.to_string(default));
    h.items(enumeration)
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let text = h.to_string(v);
            let is_default = default.as_ref().is_some_and(|d| *d == text);
            (text, Js::Bool(is_default || (default.is_none() && i == 0 && required.truthy())))
        })
        .collect()
}

/// Authentification d'une définition de sécurité Swagger 2 : `(mode, configuration)`.
fn definition_auth(h: &Heap, def: &Js, scopes: &Js) -> R<Option<(&'static str, Value)>> {
    let kind = h.get(def, "type");
    Ok(Some(if kind.is_str("basic") {
        ("basic", json!({ "username": "{{username}}", "password": "{{password}}" }))
    } else if kind.is_str("apiKey") {
        let placement = if h.get(def, "in").is_str("query") { "queryparams" } else { "header" };
        (
            "apikey",
            object(
                h,
                vec![("key", h.get(def, "name")), ("value", Js::str("{{apiKey}}")), ("placement", Js::str(placement))],
            )?,
        )
    } else if kind.is_str("oauth2") {
        ("oauth2", oauth2(h, def, scopes)?)
    } else {
        return Ok(None);
    }))
}

fn oauth2(h: &Heap, def: &Js, scopes: &Js) -> R<Value> {
    let grant = match h.get(def, "flow").as_str() {
        Some("implicit") => "implicit",
        Some("password") => "password",
        Some("accessCode") => "authorization_code",
        _ => "client_credentials",
    };
    let scope = if scopes.truthy() && h.gt_zero(&h.get(scopes, "length")) {
        h.join(&h.array_strict(scopes, "scopes")?, " ")
    } else {
        h.keys(&h.get(def, "scopes").or(h.obj(Vec::new())))?.join(" ")
    };
    object(
        h,
        vec![
            ("grantType", Js::str(grant)),
            ("authorizationUrl", h.get(def, "authorizationUrl").or(Js::str("{{oauth_authorize_url}}"))),
            ("accessTokenUrl", h.get(def, "tokenUrl").or(Js::str("{{oauth_token_url}}"))),
            ("refreshTokenUrl", Js::str("{{oauth_refresh_url}}")),
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
