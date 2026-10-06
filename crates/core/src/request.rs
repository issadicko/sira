use serde::{Deserialize, Serialize};

use crate::yaml::{self, Map, Value};

pub const TOP_ORDER: &[&str] =
    &["info", "http", "graphql", "grpc", "websocket", "runtime", "settings", "examples", "docs", "app"];
pub const BLANK_BEFORE: &[&str] = &[
    "info",
    "http",
    "graphql",
    "grpc",
    "websocket",
    "mock",
    "routes",
    "runtime",
    "settings",
    "examples",
    "docs",
    "items",
    "request",
];
pub const INFO_ORDER: &[&str] = &["name", "type", "seq", "tags", "description"];
pub const HTTP_ORDER: &[&str] = &["method", "url", "headers", "params", "body", "auth"];
const RUNTIME_ORDER: &[&str] = &["variables", "scripts", "assertions", "actions"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyValue {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamKind {
    Query,
    Path,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Param {
    pub name: String,
    pub value: String,
    pub kind: ParamKind,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Valeur d'un champ multipart : texte, ou liste de chemins de fichiers comme l'écrit Bruno.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "lowercase")]
pub enum MultipartValue {
    Text(String),
    File(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultipartField {
    pub name: String,
    #[serde(flatten)]
    pub value: MultipartValue,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Body {
    None,
    Json {
        data: String,
    },
    Text {
        data: String,
    },
    Xml {
        data: String,
    },
    #[serde(rename = "form-urlencoded")]
    FormUrlEncoded {
        fields: Vec<KeyValue>,
    },
    MultipartForm {
        fields: Vec<MultipartField>,
    },
    /// Corps d'une requête GraphQL : la requête et ses variables (texte JSON).
    Graphql {
        query: String,
        variables: String,
    },
    /// Type non édité ici : son nom, et le texte YAML canonique du reste de sa configuration.
    Other {
        label: String,
        #[serde(default)]
        config: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum Auth {
    Inherit,
    None,
    Bearer {
        token: String,
    },
    Basic {
        username: String,
        password: String,
    },
    Apikey {
        key: String,
        value: String,
        placement: String,
    },
    Digest {
        username: String,
        password: String,
    },
    Oauth2(Box<crate::oauth2::OAuth2>),
    /// AWS Signature V4 ; un champ vide est absent du fichier.
    Awsv4 {
        access_key_id: String,
        secret_access_key: String,
        session_token: String,
        service: String,
        region: String,
        profile_name: String,
    },
    /// Type non édité ici : son nom, et le texte YAML canonique du reste de sa configuration.
    Other {
        label: String,
        #[serde(default)]
        config: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assertion {
    pub expression: String,
    pub operator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    pub kind: String,
    pub code: String,
}

/// Variable posée après la réponse : `name` reçoit la valeur de l'expression (`res.body.id`), comme les « post-response
/// vars » de Bruno, stockées dans `runtime.actions` (`set-variable`, phase `after-response`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostVariable {
    pub name: String,
    pub expression: String,
    pub enabled: bool,
}

/// Vue typée d'un fichier de requête. Tout ce qui n'y figure pas reste intact dans l'arbre YAML.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDoc {
    pub name: String,
    pub request_type: String,
    pub seq: Option<i64>,
    pub method: String,
    pub url: String,
    pub params: Vec<Param>,
    pub headers: Vec<KeyValue>,
    pub body: Body,
    pub auth: Auth,
    pub assertions: Vec<Assertion>,
    pub variables: Vec<KeyValue>,
    pub scripts: Vec<Script>,
    /// Lues seulement : le fichier garde ses actions telles quelles à l'enregistrement.
    #[serde(default)]
    pub post_variables: Vec<PostVariable>,
    pub docs: Option<String>,
    pub timeout_ms: Option<u64>,
}

pub(crate) fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::Map(m)) => m.get("data").and_then(Value::scalar).unwrap_or_default(),
        Some(other) => other.scalar().unwrap_or_default(),
        None => String::new(),
    }
}

pub(crate) fn opt_text(m: &Map, key: &str) -> Option<String> {
    m.get(key).and_then(Value::scalar)
}

pub(crate) fn is_enabled(m: &Map) -> bool {
    !m.get("disabled").is_some_and(Value::is_true)
}

/// Élément d'une liste du fichier (en-tête, paramètre, champ de formulaire, assertion), lu et écrit sous forme de
/// table YAML. À l'écriture, la table existante du même élément est modifiée au lieu d'être reconstruite : ses clés
/// inconnues, son ordre et la forme de ses scalaires inchangés sont conservés.
pub(crate) trait Entry: Sized {
    const ORDER: &'static [&'static str];

    fn read(m: &Map) -> Self;
    fn build(&self) -> Map;
    /// Ce qui rapproche un élément de sa table existante.
    fn ident(&self) -> String;
}

pub(crate) fn table(pairs: Vec<(&str, Value)>) -> Map {
    Map(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

pub(crate) fn non_blank(v: &Option<String>) -> Option<&str> {
    v.as_deref().filter(|v| !v.trim().is_empty())
}

impl Entry for KeyValue {
    const ORDER: &'static [&'static str] = &["name", "value", "description", "disabled"];

    fn read(m: &Map) -> Self {
        Self {
            name: text(m.get("name")),
            value: text(m.get("value")),
            enabled: is_enabled(m),
            description: opt_text(m, "description"),
        }
    }

    fn build(&self) -> Map {
        let mut pairs = vec![("name", Value::str(&self.name)), ("value", Value::str(&self.value))];
        if let Some(d) = non_blank(&self.description) {
            pairs.push(("description", Value::str(d)));
        }
        if !self.enabled {
            pairs.push(("disabled", Value::Bool(true)));
        }
        table(pairs)
    }

    fn ident(&self) -> String {
        self.name.clone()
    }
}

impl Entry for Param {
    const ORDER: &'static [&'static str] = &["name", "value", "type", "description", "disabled"];

    fn read(m: &Map) -> Self {
        Self {
            name: text(m.get("name")),
            value: text(m.get("value")),
            kind: if m.str("type") == Some("path") { ParamKind::Path } else { ParamKind::Query },
            enabled: is_enabled(m),
            description: opt_text(m, "description"),
        }
    }

    fn build(&self) -> Map {
        let kind = match self.kind {
            ParamKind::Query => "query",
            ParamKind::Path => "path",
        };
        let mut pairs =
            vec![("name", Value::str(&self.name)), ("value", Value::str(&self.value)), ("type", Value::str(kind))];
        if let Some(d) = &self.description {
            pairs.push(("description", Value::str(d)));
        }
        if !self.enabled {
            pairs.push(("disabled", Value::Bool(true)));
        }
        table(pairs)
    }

    fn ident(&self) -> String {
        format!("{:?}/{}", self.kind, self.name)
    }
}

impl Entry for MultipartField {
    const ORDER: &'static [&'static str] = &["name", "type", "value", "contentType", "description", "disabled"];

    fn read(m: &Map) -> Self {
        Self {
            name: text(m.get("name")),
            value: match m.str("type") {
                Some("file") => MultipartValue::File(match m.get("value") {
                    Some(Value::Seq(paths)) => paths.iter().filter_map(Value::scalar).collect(),
                    other => other.and_then(Value::scalar).into_iter().collect(),
                }),
                _ => MultipartValue::Text(text(m.get("value"))),
            },
            enabled: is_enabled(m),
            content_type: opt_text(m, "contentType"),
            description: opt_text(m, "description"),
        }
    }

    fn build(&self) -> Map {
        let (kind, value) = match &self.value {
            MultipartValue::Text(text) => ("text", Value::str(text)),
            MultipartValue::File(paths) => ("file", Value::Seq(paths.iter().map(Value::str).collect())),
        };
        let mut pairs = vec![("name", Value::str(&self.name)), ("type", Value::str(kind)), ("value", value)];
        for (key, field) in [("contentType", &self.content_type), ("description", &self.description)] {
            if let Some(v) = non_blank(field) {
                pairs.push((key, Value::str(v)));
            }
        }
        if !self.enabled {
            pairs.push(("disabled", Value::Bool(true)));
        }
        table(pairs)
    }

    fn ident(&self) -> String {
        self.name.clone()
    }
}

impl Entry for Assertion {
    const ORDER: &'static [&'static str] = &["expression", "operator", "value", "description", "disabled"];

    fn read(m: &Map) -> Self {
        Self {
            expression: text(m.get("expression")),
            operator: text(m.get("operator")),
            value: opt_text(m, "value"),
            enabled: is_enabled(m),
            description: opt_text(m, "description"),
        }
    }

    fn build(&self) -> Map {
        let mut pairs = vec![("expression", Value::str(&self.expression)), ("operator", Value::str(&self.operator))];
        if let Some(v) = &self.value {
            pairs.push(("value", Value::str(v)));
        }
        if let Some(d) = &self.description {
            pairs.push(("description", Value::str(d)));
        }
        if !self.enabled {
            pairs.push(("disabled", Value::Bool(true)));
        }
        table(pairs)
    }

    fn ident(&self) -> String {
        format!("{}\0{}", self.expression, self.operator)
    }
}

impl Entry for Script {
    const ORDER: &'static [&'static str] = &["type", "code"];

    fn read(m: &Map) -> Self {
        Self { kind: text(m.get("type")), code: text(m.get("code")) }
    }

    fn build(&self) -> Map {
        table(vec![("type", Value::str(&self.kind)), ("code", Value::str(&self.code))])
    }

    fn ident(&self) -> String {
        self.kind.clone()
    }
}

fn post_variables(actions: &[Value]) -> Vec<PostVariable> {
    actions
        .iter()
        .filter_map(Value::as_map)
        .filter(|a| a.str("type") == Some("set-variable") && a.str("phase") == Some("after-response"))
        .map(|a| PostVariable {
            name: a.map("variable").and_then(|v| opt_text(v, "name")).unwrap_or_default(),
            expression: a.map("selector").and_then(|v| opt_text(v, "expression")).unwrap_or_default(),
            enabled: is_enabled(a),
        })
        .collect()
}

fn read_list<T: Entry>(items: &[Value]) -> Vec<T> {
    items.iter().filter_map(Value::as_map).map(T::read).collect()
}

pub(crate) fn key_values(items: &[Value]) -> Vec<KeyValue> {
    read_list(items)
}

/// Auth d'un fichier : `Other` garde le type et le reste de la configuration sous forme canonique, pour que deux
/// configurations différentes ne soient jamais prises pour la même.
pub(crate) fn auth_from(value: Option<&Value>) -> Auth {
    let Some(value) = value else { return Auth::None };
    if value.as_str() == Some("inherit") {
        return Auth::Inherit;
    }
    match value.as_map() {
        Some(m) => auth_of(m),
        None => Auth::Other { label: value.scalar().unwrap_or_default(), config: String::new() },
    }
}

fn auth_of(m: &Map) -> Auth {
    let field = |k: &str| text(m.get(k));
    match m.str("type").unwrap_or_default() {
        "bearer" => Auth::Bearer { token: field("token") },
        "basic" => Auth::Basic { username: field("username"), password: field("password") },
        "apikey" => Auth::Apikey {
            key: field("key"),
            value: field("value"),
            placement: m.str("placement").unwrap_or("header").into(),
        },
        "oauth2" => Auth::Oauth2(Box::new(crate::oauth2::read(m))),
        "digest" => Auth::Digest { username: field("username"), password: field("password") },
        "awsv4" => Auth::Awsv4 {
            access_key_id: field("accessKeyId"),
            secret_access_key: field("secretAccessKey"),
            session_token: field("sessionToken"),
            service: field("service"),
            region: field("region"),
            profile_name: field("profileName"),
        },
        "inherit" => Auth::Inherit,
        other => Auth::Other { label: other.into(), config: canonical(m) },
    }
}

fn body_of(b: &Map) -> Body {
    match b.str("type").unwrap_or_default() {
        "json" => Body::Json { data: text(b.get("data")) },
        "text" => Body::Text { data: text(b.get("data")) },
        "xml" => Body::Xml { data: text(b.get("data")) },
        "form-urlencoded" => Body::FormUrlEncoded { fields: key_values(b.seq("data")) },
        "multipart-form" => Body::MultipartForm { fields: read_list(b.seq("data")) },
        "" if b.get("query").is_some() || b.get("variables").is_some() => {
            Body::Graphql { query: text(b.get("query")), variables: text(b.get("variables")) }
        }
        other => Body::Other { label: other.into(), config: canonical(b) },
    }
}

/// Texte YAML de `m` sans son `type`, clés triées, ou le texte vide s'il ne reste rien.
fn canonical(m: &Map) -> String {
    let rest = Map(m.0.iter().filter(|(k, _)| k != "type").cloned().collect());
    if rest.is_empty() {
        return String::new();
    }
    yaml::emit(&Value::Map(rest).sorted(), &[]).trim_end().to_owned()
}

/// Table qui porte méthode, URL, en-têtes et corps : `http`, ou la section propre au protocole (`graphql`, `grpc`,
/// `websocket`) d'une requête qui n'a pas de table `http`.
fn section<'a>(root: &Map, request_type: &'a str) -> &'a str {
    if root.map("http").is_none() && root.map(request_type).is_some() {
        request_type
    } else {
        "http"
    }
}

impl RequestDoc {
    pub fn from_tree(root: &Map) -> Self {
        let empty = Map::default();
        let info = root.map("info").unwrap_or(&empty);
        let request_type = info.str("type").unwrap_or("http").to_owned();
        let http = root.map(section(root, &request_type)).unwrap_or(&empty);
        let runtime = root.map("runtime").unwrap_or(&empty);
        let settings = root.map("settings").unwrap_or(&empty);

        let scripts = read_list(runtime.seq("scripts"));

        Self {
            name: info.str("name").unwrap_or_default().to_owned(),
            request_type,
            seq: info.get("seq").and_then(Value::as_i64),
            method: http.str("method").unwrap_or("GET").to_uppercase(),
            url: text(http.get("url")),
            params: read_list(http.seq("params")),
            headers: key_values(http.seq("headers")),
            body: http.map("body").map_or(Body::None, body_of),
            auth: auth_from(http.get("auth")),
            assertions: read_list(runtime.seq("assertions")),
            variables: key_values(runtime.seq("variables")),
            scripts,
            post_variables: post_variables(runtime.seq("actions")),
            docs: root.get("docs").and_then(Value::scalar),
            timeout_ms: settings.get("timeout").and_then(Value::as_i64).filter(|t| *t > 0).map(|t| t as u64),
        }
    }

    /// Écrit dans l'arbre uniquement les sections qui diffèrent de `previous`,
    /// pour qu'un fichier écrit à la main ne soit pas renormalisé sans raison. Une liste, un corps ou une auth
    /// réécrits reprennent leurs tables existantes : les clés inconnues qu'elles portent sont conservées.
    pub fn apply(&self, root: &mut Map, previous: &RequestDoc) {
        if self.name != previous.name {
            root.map_mut_or_insert("info", TOP_ORDER).set("name", Value::str(&self.name), INFO_ORDER);
        }
        let changed_http = self.method != previous.method
            || self.url != previous.url
            || self.headers != previous.headers
            || self.params != previous.params
            || self.body != previous.body
            || self.auth != previous.auth;
        if changed_http {
            let http = root.map_mut_or_insert(section(root, &previous.request_type), TOP_ORDER);
            if self.method != previous.method {
                http.set("method", Value::str(&self.method), HTTP_ORDER);
            }
            if self.url != previous.url {
                http.set("url", Value::str(&self.url), HTTP_ORDER);
            }
            if self.headers != previous.headers {
                set_list(http, "headers", &self.headers, HTTP_ORDER);
            }
            if self.params != previous.params {
                set_list(http, "params", &self.params, HTTP_ORDER);
            }
            if self.body != previous.body {
                write_body(http, &self.body);
            }
            if self.auth != previous.auth {
                write_auth(http, &self.auth);
            }
        }
        if self.assertions != previous.assertions || self.scripts != previous.scripts {
            let runtime = root.map_mut_or_insert("runtime", TOP_ORDER);
            if self.scripts != previous.scripts {
                let written: Vec<Script> = self.scripts.iter().filter(|s| !s.code.trim().is_empty()).cloned().collect();
                set_list(runtime, "scripts", &written, RUNTIME_ORDER);
            }
            if self.assertions != previous.assertions {
                set_list(runtime, "assertions", &self.assertions, RUNTIME_ORDER);
            }
            if runtime.is_empty() {
                root.remove("runtime");
            }
        }
        if self.docs != previous.docs {
            match self.docs.as_deref().filter(|d| !d.is_empty()) {
                Some(d) => root.set("docs", Value::str(d), TOP_ORDER),
                None => {
                    root.remove("docs");
                }
            }
        }
    }
}

/// Applique à `raw` ce qui sépare `old` de `new`, deux rendus canoniques d'une même table : les clés qui n'ont pas
/// changé gardent leur valeur d'origine, quelle que soit sa forme dans le fichier.
fn patch(raw: &mut Map, old: &Map, new: &Map, order: &[&str]) {
    for (key, value) in &new.0 {
        if old.get(key) != Some(value) {
            raw.set(key, value.clone(), order);
        }
    }
    for (key, _) in &old.0 {
        if new.get(key).is_none() {
            raw.remove(key);
        }
    }
}

fn merge_entries<T: Entry>(existing: &[Value], items: &[T]) -> Vec<Value> {
    let mut known: Vec<Option<(&Map, T)>> =
        existing.iter().filter_map(Value::as_map).map(|m| Some((m, T::read(m)))).collect();
    let table = items.iter().map(|item| {
        let found = known.iter_mut().find(|slot| slot.as_ref().is_some_and(|(_, old)| old.ident() == item.ident()));
        match found.and_then(Option::take) {
            Some((raw, old)) => {
                let mut merged = raw.clone();
                patch(&mut merged, &old.build(), &item.build(), T::ORDER);
                merged
            }
            None => item.build(),
        }
    });
    table.map(Value::Map).collect()
}

pub(crate) fn set_list<T: Entry>(map: &mut Map, key: &str, items: &[T], order: &[&str]) {
    if items.is_empty() {
        map.remove(key);
    } else {
        let merged = merge_entries(map.seq(key), items);
        map.set(key, Value::Seq(merged), order);
    }
}

const BODY_ORDER: &[&str] = &["type", "data"];
const AUTH_ORDER: &[&str] = &[
    "type",
    "token",
    "username",
    "password",
    "key",
    "value",
    "placement",
    "accessKeyId",
    "secretAccessKey",
    "sessionToken",
    "service",
    "region",
    "profileName",
    "flow",
    "authorizationUrl",
    "accessTokenUrl",
    "refreshTokenUrl",
    "callbackUrl",
    "credentials",
    "resourceOwner",
    "scope",
    "state",
    "additionalParameters",
    "pkce",
    "tokenConfig",
    "settings",
];

/// Type et contenu texte du corps ; les champs d'un formulaire sont écrits à part, par [`set_list`].
fn body_scalars(body: &Body) -> Option<Map> {
    let (kind, data) = match body {
        Body::None | Body::Other { .. } | Body::Graphql { .. } => return None,
        Body::Json { data } => ("json", Some(data)),
        Body::Text { data } => ("text", Some(data)),
        Body::Xml { data } => ("xml", Some(data)),
        Body::FormUrlEncoded { .. } => ("form-urlencoded", None),
        Body::MultipartForm { .. } => ("multipart-form", None),
    };
    let mut pairs = vec![("type", Value::str(kind))];
    pairs.extend(data.map(|d| ("data", Value::str(d))));
    Some(table(pairs))
}

const GRAPHQL_ORDER: &[&str] = &["query", "variables"];

fn write_graphql(http: &mut Map, query: &str, variables: &str) {
    if query.is_empty() && variables.is_empty() {
        http.remove("body");
        return;
    }
    let mut merged = http.map("body").cloned().unwrap_or_default();
    for (key, value) in [("query", query), ("variables", variables)] {
        match (value.is_empty(), merged.get(key).is_some()) {
            (true, true) => {
                merged.remove(key);
            }
            (true, false) => {}
            (false, _) if text(merged.get(key)) == value => {}
            (false, _) => merged.set(key, Value::str(value), GRAPHQL_ORDER),
        }
    }
    http.set("body", Value::Map(merged), HTTP_ORDER);
}

fn write_body(http: &mut Map, body: &Body) {
    if let Body::Graphql { query, variables } = body {
        return write_graphql(http, query, variables);
    }
    let Some(new) = body_scalars(body) else {
        if *body == Body::None {
            http.remove("body");
        }
        return;
    };
    let raw = http.map("body").filter(|raw| raw.str("type") == new.str("type"));
    let mut merged = raw.cloned().unwrap_or_default();
    let old = raw.and_then(|raw| body_scalars(&body_of(raw))).unwrap_or_default();
    patch(&mut merged, &old, &new, BODY_ORDER);
    match body {
        Body::FormUrlEncoded { fields } => set_list(&mut merged, "data", fields, BODY_ORDER),
        Body::MultipartForm { fields } => set_list(&mut merged, "data", fields, BODY_ORDER),
        _ => {}
    }
    http.set("body", Value::Map(merged), HTTP_ORDER);
}

fn auth_table(auth: &Auth) -> Option<Map> {
    Some(match auth {
        Auth::None | Auth::Inherit | Auth::Other { .. } => return None,
        Auth::Bearer { token } => table(vec![("type", Value::str("bearer")), ("token", Value::str(token))]),
        Auth::Basic { username, password } => table(vec![
            ("type", Value::str("basic")),
            ("username", Value::str(username)),
            ("password", Value::str(password)),
        ]),
        Auth::Apikey { key, value, placement } => table(vec![
            ("type", Value::str("apikey")),
            ("key", Value::str(key)),
            ("value", Value::str(value)),
            ("placement", Value::str(placement)),
        ]),
        Auth::Oauth2(config) => crate::oauth2::write(config),
        Auth::Digest { username, password } => table(vec![
            ("type", Value::str("digest")),
            ("username", Value::str(username)),
            ("password", Value::str(password)),
        ]),
        Auth::Awsv4 { access_key_id, secret_access_key, session_token, service, region, profile_name } => table(vec![
            ("type", Value::str("awsv4")),
            ("accessKeyId", Value::str(access_key_id)),
            ("secretAccessKey", Value::str(secret_access_key)),
            ("sessionToken", Value::str(session_token)),
            ("service", Value::str(service)),
            ("region", Value::str(region)),
            ("profileName", Value::str(profile_name)),
        ]),
    })
}

fn write_auth(http: &mut Map, auth: &Auth) {
    match auth {
        Auth::None => {
            http.remove("auth");
        }
        Auth::Inherit => http.set("auth", Value::str("inherit"), HTTP_ORDER),
        Auth::Other { .. } => {}
        _ => {
            let Some(new) = auth_table(auth) else { return };
            let raw = http.map("auth").filter(|raw| raw.str("type") == new.str("type"));
            let mut merged = raw.cloned().unwrap_or_default();
            let old = raw.and_then(|raw| auth_table(&auth_of(raw))).unwrap_or_default();
            patch(&mut merged, &old, &new, AUTH_ORDER);
            http.set("auth", Value::Map(merged), HTTP_ORDER);
        }
    }
}
