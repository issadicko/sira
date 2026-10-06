use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::runtime::Handle;
use xc_core::vars::Context;
use xc_core::{read_request, NetworkPrefs};
use xc_engine::{CookieJar, CookieView, EngineError, HttpRequest, Redirects, ScriptCookie};
use xc_script::{Callbacks, Vars};

use crate::convert::script_response;
use crate::pipeline::{run_request_at, Request};
use crate::session::{EnvWrites, Session};

/// Au-delà, `bru.runRequest` qui s'appelle lui-même est refusé plutôt que de saturer la pile.
const MAX_DEPTH: usize = 8;

/// `bru.runRequest(chemin)` : exécute une autre requête de la collection, avec les variables du script appelant.
pub(crate) struct Nested {
    pub handle: Handle,
    pub root: PathBuf,
    pub env: Option<String>,
    pub collection_name: String,
    pub cancel: Arc<AtomicBool>,
    pub depth: usize,
    pub network: NetworkPrefs,
    pub cookies: CookieJar,
}

fn session_of(vars: &Vars, env: Option<&str>, network: NetworkPrefs, cookies: CookieJar) -> Session {
    Session {
        runtime: vars.runtime.clone(),
        global: vars.global.clone(),
        env: Some(EnvWrites { name: env.map(str::to_owned), vars: vars.env.clone() }),
        collection: Some(vars.collection.clone()),
        network,
        cookies,
        ..Session::default()
    }
}

fn merge_back(vars: &mut Vars, session: Session) {
    vars.runtime = session.runtime;
    vars.global = session.global;
    if let Some(env) = session.env {
        vars.env = env.vars;
    }
    if let Some(collection) = session.collection {
        vars.collection = collection;
    }
}

/// Le chemin d'une requête de la collection : relatif à sa racine, `.yml` ajouté s'il manque.
fn request_path(path: &str) -> String {
    let relative = path.trim_start_matches("./").trim_start_matches('/');
    if relative.ends_with(".yml") {
        relative.to_owned()
    } else {
        format!("{relative}.yml")
    }
}

const NO_TIMEOUT: Duration = Duration::from_secs(600);

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Une requête d'après une configuration à la façon d'axios (`method`, `url`, `baseURL`, `params`, `headers`, `data`,
/// `timeout`).
fn http_request(config: &Value) -> Result<HttpRequest, String> {
    let get = |key: &str| config.get(key).filter(|v| !v.is_null());
    let mut url = get("url").map(text).ok_or("url is required")?;
    if let (Some(base), false) = (get("baseURL").map(text), url.contains("://")) {
        url = format!("{}/{}", base.trim_end_matches('/'), url.trim_start_matches('/'));
    }
    if let Some(Value::Object(params)) = get("params") {
        let mut query = form_urlencoded::Serializer::new(String::new());
        for (name, value) in params {
            query.append_pair(name, &text(value));
        }
        let query = query.finish();
        if !query.is_empty() {
            url = format!("{url}{}{query}", if url.contains('?') { '&' } else { '?' });
        }
    }
    let mut headers: Vec<(String, String)> = match get("headers") {
        Some(Value::Object(map)) => map.iter().map(|(k, v)| (k.clone(), text(v))).collect(),
        _ => Vec::new(),
    };
    let body = match get("data") {
        None => None,
        Some(Value::String(data)) => Some(data.clone().into_bytes()),
        Some(other) => {
            if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("content-type")) {
                headers.push(("Content-Type".into(), "application/json".into()));
            }
            Some(other.to_string().into_bytes())
        }
    };
    Ok(HttpRequest {
        method: get("method").map(|m| text(m).to_uppercase()).unwrap_or_else(|| "GET".into()),
        url,
        headers,
        body,
        timeout: get("timeout").and_then(Value::as_u64).filter(|t| *t > 0).map_or(NO_TIMEOUT, Duration::from_millis),
        max_response_body: None,
        network: xc_engine::Network::default(),
    })
}

fn error_code(error: &EngineError) -> &'static str {
    match error {
        EngineError::Dns { .. } => "ENOTFOUND",
        EngineError::Connect { .. } => "ECONNREFUSED",
        EngineError::Timeout(_) => "ECONNABORTED",
        _ => "ERR_NETWORK",
    }
}

impl Nested {
    /// Les réglages réseau de l'hôte et de la collection pour `url`, comme pour une requête de la collection.
    fn network_for(&self, url: &str) -> Result<xc_engine::Network, String> {
        let ctx = Context::load(&self.root, "request.yml").map_err(|e| e.to_string())?;
        xc_core::network::resolve(
            &self.root,
            &ctx.collection,
            &self.network,
            url,
            Redirects::default(),
            &mut |text| text.to_owned(),
            &|name| std::env::var(name).ok(),
        )
        .map_err(|e| e.to_string())
    }
}

/// Un cookie vu d'un script : les champs de Bruno ; `expires` vaut `"Infinity"` pour un cookie de session.
fn script_cookie(cookie: &CookieView) -> Value {
    json!({
        "key": cookie.key,
        "value": cookie.value,
        "domain": cookie.domain,
        "path": cookie.path,
        "secure": cookie.secure,
        "httpOnly": cookie.http_only,
        "expires": cookie.expires.clone().unwrap_or_else(|| "Infinity".into()),
    })
}

impl Callbacks for Nested {
    fn cookies(&self, call: &Value) -> Result<Value, String> {
        let text = |field: &str| call.get(field).and_then(Value::as_str);
        let url = || -> Result<url::Url, String> {
            let given = text("url").ok_or("URL is required")?;
            url::Url::parse(given).map_err(|_| format!("Invalid URL: {given}"))
        };
        let named = |cookies: Vec<CookieView>| {
            let name = text("name").unwrap_or_default().to_owned();
            cookies.into_iter().filter(move |c| c.key == name)
        };
        match text("op").unwrap_or_default() {
            "matching" => Ok(Value::Array(self.cookies.matching(&url()?).iter().map(script_cookie).collect())),
            "get" => Ok(named(self.cookies.matching(&url()?)).next().map_or(Value::Null, |c| script_cookie(&c))),
            "has" => Ok(Value::Bool(named(self.cookies.matching(&url()?)).next().is_some())),
            "set" => {
                let url = url()?;
                let cookies = call.get("cookies").cloned().unwrap_or(Value::Null);
                let cookies: Vec<ScriptCookie> =
                    serde_json::from_value(cookies).map_err(|e| format!("cookie invalide : {e}"))?;
                cookies.iter().try_for_each(|cookie| self.cookies.set_for(&url, cookie))?;
                Ok(Value::Null)
            }
            "delete" => {
                self.cookies.remove_matching(&url()?, text("name"));
                Ok(Value::Null)
            }
            "clear" => {
                self.cookies.clear();
                Ok(Value::Null)
            }
            other => Err(format!("opération de cookies inconnue : {other}")),
        }
    }

    fn send(&self, config: &Value) -> Result<Value, Value> {
        let failure = |message: String| json!({ "message": message, "isAxiosError": true });
        let mut request = http_request(config).map_err(failure)?;
        request.network = self.network_for(&request.url).map_err(failure)?;
        let sent = json!({
            "url": request.url,
            "method": request.method.to_lowercase(),
            "headers": request.headers.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect::<serde_json::Map<_, _>>(),
            "data": request.body.as_ref().map(|b| String::from_utf8_lossy(b).into_owned()),
        });
        let url = request.url.clone();
        match self.handle.block_on(xc_engine::send(request)) {
            Err(error) => Err(json!({
                "message": error.to_string(),
                "code": error_code(&error),
                "isAxiosError": true,
                "config": sent,
            })),
            Ok(res) => {
                let view = script_response(&res, &url, true);
                let reply = json!({
                    "status": view.status,
                    "statusText": view.status_text,
                    "headers": view.headers,
                    "data": view.data,
                    "config": sent,
                });
                if (200..300).contains(&view.status) {
                    Ok(reply)
                } else {
                    Err(json!({
                        "message": format!("Request failed with status code {}", view.status),
                        "code": if view.status < 500 { "ERR_BAD_REQUEST" } else { "ERR_BAD_RESPONSE" },
                        "isAxiosError": true,
                        "response": reply,
                        "config": sent,
                    }))
                }
            }
        }
    }

    fn run_request(&self, path: &str, vars: &mut Vars) -> Value {
        if self.depth >= MAX_DEPTH {
            return json!({ "message": "bru.runRequest: too many nested requests" });
        }
        let relative = request_path(path);
        let Ok(doc) = read_request(&self.root, &relative) else { return json!({}) };
        let mut session = session_of(vars, self.env.as_deref(), self.network.clone(), self.cookies.clone());
        let request = Request {
            root: &self.root,
            path: &relative,
            doc: &doc,
            env: self.env.as_deref(),
            collection_name: &self.collection_name,
            execution_mode: "standalone",
            cancel: Arc::clone(&self.cancel),
        };
        let outcome = self.handle.block_on(run_request_at(request, &mut session, self.depth + 1));
        merge_back(vars, session);
        match (&outcome.error, &outcome.response) {
            (None, Some(res)) => {
                let view = script_response(res, &outcome.url, true);
                json!({
                    "status": view.status,
                    "statusText": view.status_text,
                    "headers": view.headers,
                    "data": view.data,
                    "url": view.url,
                    "responseTime": view.response_time,
                    "duration": res.timings.total_ms,
                    "size": view.size.total,
                })
            }
            (Some(error), _) => json!({ "message": error.message }),
            (None, None) => json!({ "message": "the request was skipped" }),
        }
    }
}
