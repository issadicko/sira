use std::cell::RefCell;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rquickjs::{CatchResultExt, Ctx, Exception, Function, Object, Value as JsValue};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::sandbox::{settle, Clock, Limits, Sandbox, ScriptError};
use crate::vars::{Dirty, Vars, ENV_NAME};

const PRELUDE: &str = include_str!("../js/prelude.js");
const LIBS: [(&str, &str); 8] = [
    ("chai", include_str!("../js/libs/chai.js")),
    ("ajv", include_str!("../js/libs/ajv.js")),
    ("crypto-js", include_str!("../js/libs/crypto-js.js")),
    ("moment", include_str!("../js/libs/moment.js")),
    ("tv4", include_str!("../js/libs/tv4.js")),
    ("uuid", include_str!("../js/libs/uuid.js")),
    ("nanoid", include_str!("../js/libs/nanoid.js")),
    ("buffer", include_str!("../js/libs/buffer.js")),
];

/// Enveloppe de Bruno pour QuickJS : fonction asynchrone, `setTimeout` réduit à une promesse, premier tour de boucle
/// cédé avant le code de l'utilisateur.
const PREFIX: &str = "\n      (async () => {\n        const setTimeout = async(fn, timer) => {\n          v = await bru.sleep(timer);\n          fn.apply();\n        }\n\n        await bru.sleep(0);\n        try {\n          ";
const SUFFIX: &str =
    "\n        }\n        catch(error) {\n          throw error;\n        }\n        return 'done';\n      })()\n    ";
const SLEEP_SLICE: Duration = Duration::from_millis(20);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Pre,
    Post,
    Tests,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PathParam {
    pub name: String,
    pub value: String,
    #[serde(rename = "type")]
    pub kind: String,
}

/// La requête telle que les scripts la voient et la modifient. Les en-têtes gardent l'ordre et la casse du fichier.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptRequest {
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    pub tags: Vec<String>,
    pub path_params: Vec<PathParam>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_mode: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResponseSize {
    pub header: u64,
    pub body: u64,
    pub total: u64,
}

/// La réponse : en-têtes en minuscules, corps déjà décodé (JSON si possible, texte sinon).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptResponse {
    pub status: u16,
    pub status_text: String,
    pub headers: Map<String, Value>,
    pub data: Value,
    pub response_time: u64,
    pub url: String,
    pub size: ResponseSize,
}

/// Ce que l'hôte offre aux scripts au-delà des variables : lancer une autre requête de la collection.
pub trait Callbacks: Send + Sync {
    /// Exécute la requête `path` avec les variables `vars` (qu'elle peut modifier) et rend sa réponse en JSON, ou
    /// `{ "message": … }` quand elle n'aboutit pas.
    fn run_request(&self, path: &str, vars: &mut Vars) -> Value;

    /// Envoie la requête décrite à la façon d'axios (`url`, `method`, `headers`, `data`, `params`, `timeout`) : la
    /// réponse (`status`, `statusText`, `headers`, `data`), ou l'erreur (`message`, `code`, `response`).
    fn send(&self, config: &Value) -> Result<Value, Value>;

    /// Une opération sur le pot de cookies de l'hôte, décrite par `call` (`op`, `url`, …) ; son résultat, ou la raison
    /// de l'échec.
    fn cookies(&self, call: &Value) -> Result<Value, String>;
}

pub struct Input {
    pub phase: Phase,
    pub script: String,
    pub request: ScriptRequest,
    pub response: Option<ScriptResponse>,
    pub vars: Vars,
    pub collection_name: String,
    pub collection_path: String,
    pub execution_mode: String,
    pub dynamic: fn(&str) -> Option<String>,
    pub limits: Limits,
    /// Levé par l'appelant (annulation de la requête), il interrompt le script.
    pub cancel: Arc<AtomicBool>,
    /// `bru.runRequest` n'existe que si l'hôte le fournit.
    pub callbacks: Option<Arc<dyn Callbacks>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestResult {
    pub description: String,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<Value>,
}

/// Une assertion déclarative (`res.status eq 200`) : expression gauche, opérateur, opérande droit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssertionSpec {
    pub expression: String,
    pub operator: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssertionOutcome {
    pub expression: String,
    pub operator: String,
    pub value: Option<String>,
    pub passed: bool,
    pub error: Option<String>,
    /// Valeur de l'expression gauche, en JSON.
    pub actual: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogLine {
    pub level: String,
    pub args: Value,
}

/// Suite du run demandée par `bru.setNextRequest` : rien, arrêter après cette requête, ou sauter à la requête nommée.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum NextRequest {
    #[default]
    Unset,
    Stop,
    Named(String),
}

#[derive(Debug, Clone)]
pub struct Output {
    pub request: ScriptRequest,
    pub max_redirects: Option<u64>,
    pub headers_to_delete: Vec<String>,
    pub disable_json_parsing: bool,
    /// Corps de réponse posé par `res.setBody()`.
    pub response_data: Option<Value>,
    pub vars: Vars,
    pub dirty: Dirty,
    pub results: Vec<TestResult>,
    pub assertions: Vec<AssertionOutcome>,
    pub logs: Vec<LogLine>,
    pub next_request: NextRequest,
    pub skip_request: bool,
    pub stop_execution: bool,
    /// Erreur du script : tout le reste (variables écrites, tests finis) est gardé.
    pub error: Option<ScriptError>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Collected {
    request: CollectedRequest,
    response: Option<CollectedResponse>,
    results: Vec<TestResult>,
    assertions: Vec<AssertionOutcome>,
    next_request: Value,
    skip_request: bool,
    stop_execution: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CollectedRequest {
    url: String,
    method: String,
    headers: Map<String, Value>,
    data: Option<Value>,
    timeout: Option<u64>,
    max_redirects: Option<u64>,
    headers_to_delete: Vec<String>,
    disable_json_parsing: bool,
}

#[derive(Deserialize)]
struct CollectedResponse {
    data: Value,
}

struct Host {
    vars: Vars,
    dirty: Dirty,
    logs: Vec<LogLine>,
    clock: Clock,
    cancelled: Arc<AtomicBool>,
    dynamic: fn(&str) -> Option<String>,
}

impl Host {
    fn mark(&mut self, scope: &str) {
        match scope {
            "env" => self.dirty.env = true,
            "runtime" => self.dirty.runtime = true,
            "global" => self.dirty.global = true,
            "collection" => self.dirty.collection = true,
            _ => {}
        }
    }

    fn set(&mut self, scope: &str, key: String, value: Value) {
        let Some(map) = self.vars.scope_mut(scope) else { return };
        if map.get(&key) != Some(&value) {
            map.insert(key, value);
            self.mark(scope);
        }
    }

    fn remove(&mut self, scope: &str, key: &str) {
        if self.vars.scope_mut(scope).is_some_and(|map| map.remove(key).is_some()) {
            self.mark(scope);
        }
    }

    fn clear(&mut self, scope: &str) {
        let Some(map) = self.vars.scope_mut(scope) else { return };
        let kept = (scope == "env").then(|| map.get(ENV_NAME).cloned()).flatten();
        let had_any = map.keys().any(|k| !(scope == "env" && k == ENV_NAME));
        map.clear();
        if let Some(name) = kept {
            map.insert(ENV_NAME.to_owned(), name);
        }
        if had_any {
            self.mark(scope);
        }
    }

    fn sleep(&self, ms: u64) {
        self.clock.pause(|| {
            let mut left = Duration::from_millis(ms);
            while !left.is_zero() && !self.cancelled.load(Ordering::Relaxed) {
                let slice = left.min(SLEEP_SLICE);
                std::thread::sleep(slice);
                left -= slice;
            }
        });
    }
}

fn js<T>(ctx: &Ctx<'_>, outcome: rquickjs::Result<T>) -> Result<T, ScriptError> {
    outcome.catch(ctx).map_err(ScriptError::from)
}

fn install<'js>(
    ctx: &Ctx<'js>,
    host: &Rc<RefCell<Host>>,
    collection_path: Rc<String>,
    callbacks: Option<Arc<dyn Callbacks>>,
) -> rquickjs::Result<()> {
    let h = Object::new(ctx.clone())?;
    let st = Rc::clone(host);
    h.set(
        "get",
        Function::new(ctx.clone(), move |scope: String, key: String| -> Option<String> {
            st.borrow().vars.scope(&scope)?.get(&key).map(Value::to_string)
        })?,
    )?;
    let st = Rc::clone(host);
    h.set(
        "has",
        Function::new(ctx.clone(), move |scope: String, key: String| -> bool {
            st.borrow().vars.scope(&scope).is_some_and(|map| map.contains_key(&key))
        })?,
    )?;
    let st = Rc::clone(host);
    h.set(
        "set",
        Function::new(ctx.clone(), move |scope: String, key: String, json: String| {
            let value = serde_json::from_str(&json).unwrap_or(Value::Null);
            st.borrow_mut().set(&scope, key, value);
        })?,
    )?;
    let st = Rc::clone(host);
    h.set(
        "remove",
        Function::new(ctx.clone(), move |scope: String, key: String| st.borrow_mut().remove(&scope, &key))?,
    )?;
    let st = Rc::clone(host);
    h.set("clear", Function::new(ctx.clone(), move |scope: String| st.borrow_mut().clear(&scope))?)?;
    let st = Rc::clone(host);
    h.set(
        "all",
        Function::new(ctx.clone(), move |scope: String| -> String {
            let all = st.borrow().vars.scope(&scope).cloned().unwrap_or_default();
            Value::Object(all).to_string()
        })?,
    )?;
    let st = Rc::clone(host);
    h.set(
        "interpolate",
        Function::new(ctx.clone(), move |text: String| -> String {
            let host = st.borrow();
            host.vars.interpolate(&text, &host.dynamic)
        })?,
    )?;
    let st = Rc::clone(host);
    h.set(
        "log",
        Function::new(ctx.clone(), move |level: String, json: String| {
            let args = serde_json::from_str(&json).unwrap_or(Value::Null);
            st.borrow_mut().logs.push(LogLine { level, args });
        })?,
    )?;
    let st = Rc::clone(host);
    h.set("sleep", Function::new(ctx.clone(), move |ms: f64| st.borrow().sleep(ms.max(0.0) as u64))?)?;
    let st = Rc::clone(host);
    let run_callbacks = callbacks.clone();
    h.set(
        "runRequest",
        Function::new(ctx.clone(), move |path: String| -> String {
            let Some(callbacks) = &run_callbacks else { return "{}".into() };
            let mut host = st.borrow_mut();
            let before = host.vars.clone();
            let clock = host.clock.clone();
            let result = clock.pause(|| callbacks.run_request(&path, &mut host.vars));
            for scope in ["env", "runtime", "global", "collection"] {
                if host.vars.scope(scope) != before.scope(scope) {
                    host.mark(scope);
                }
            }
            result.to_string()
        })?,
    )?;
    let send_callbacks = callbacks.clone();
    let st = Rc::clone(host);
    h.set(
        "send",
        Function::new(ctx.clone(), move |config: String| -> String {
            let Some(callbacks) = &send_callbacks else {
                return json!({ "error": { "message": "unavailable" } }).to_string();
            };
            let config = serde_json::from_str(&config).unwrap_or(Value::Null);
            let clock = st.borrow().clock.clone();
            match clock.pause(|| callbacks.send(&config)) {
                Ok(reply) => json!({ "ok": reply }),
                Err(error) => json!({ "error": error }),
            }
            .to_string()
        })?,
    )?;
    let cookie_callbacks = callbacks.clone();
    h.set(
        "cookies",
        Function::new(ctx.clone(), move |call: String| -> String {
            let Some(callbacks) = &cookie_callbacks else {
                return json!({ "error": "unavailable" }).to_string();
            };
            let call = serde_json::from_str(&call).unwrap_or(Value::Null);
            match callbacks.cookies(&call) {
                Ok(value) => json!({ "ok": value }),
                Err(message) => json!({ "error": message }),
            }
            .to_string()
        })?,
    )?;
    h.set(
        "random",
        Function::new(ctx.clone(), |size: usize| -> String {
            let bytes = (0..size.div_ceil(16)).flat_map(|_| uuid::Uuid::new_v4().into_bytes()).take(size);
            bytes.map(|b| format!("{b:02x}")).collect()
        })?,
    )?;
    h.set(
        "resolve",
        Function::new(ctx.clone(), |json: String| -> String {
            let parts: Vec<String> = serde_json::from_str(&json).unwrap_or_default();
            resolve(&parts)
        })?,
    )?;
    let root = Rc::clone(&collection_path);
    h.set(
        "module",
        Function::new(ctx.clone(), move |ctx: Ctx<'js>, module: String| -> rquickjs::Result<String> {
            load_module(&root, &module).map_err(|message| Exception::throw_message(&ctx, &message))
        })?,
    )?;
    h.set(
        "lib",
        Function::new(ctx.clone(), |ctx: Ctx<'js>, name: String| -> rquickjs::Result<JsValue<'js>> {
            let Some((_, source)) = LIBS.iter().find(|(lib, _)| *lib == name) else {
                return Err(Exception::throw_message(&ctx, &format!("Cannot find module {name}")));
            };
            ctx.eval::<(), _>(*source)?;
            let globals = ctx.globals();
            let lib: JsValue = globals.get("__lib")?;
            globals.remove("__lib")?;
            Ok(lib)
        })?,
    )?;
    ctx.globals().set("__h", h)
}

fn nothing(input: &Input) -> Output {
    Output {
        request: input.request.clone(),
        max_redirects: None,
        headers_to_delete: Vec::new(),
        disable_json_parsing: false,
        response_data: None,
        vars: input.vars.clone(),
        dirty: Dirty::default(),
        results: Vec::new(),
        assertions: Vec::new(),
        logs: Vec::new(),
        next_request: NextRequest::Unset,
        skip_request: false,
        stop_execution: false,
        error: None,
    }
}

/// Pile native en plus de celle que QuickJS s'accorde : appels de l'hôte et de la bibliothèque standard.
const NATIVE_STACK_MARGIN: usize = 4 << 20;

/// Exécute le script d'une phase dans un sandbox neuf, sur un fil à la pile assez grande pour la limite de QuickJS.
/// Une erreur du script n'est pas une erreur d'exécution : elle est dans `Output::error`, avec les variables et les
/// tests qui précèdent.
pub fn run(input: Input) -> Result<Output, ScriptError> {
    if input.script.trim().is_empty() {
        return Ok(nothing(&input));
    }
    let stack = input.limits.stack + NATIVE_STACK_MARGIN;
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .name("script".into())
            .stack_size(stack)
            .spawn_scoped(scope, || execute(input))
            .map_err(|e| ScriptError::new(format!("fil du script : {e}")))?
            .join()
            .unwrap_or_else(|_| Err(ScriptError::new("le moteur de scripts a planté")))
    })
}

fn execute(input: Input) -> Result<Output, ScriptError> {
    let sandbox = Sandbox::cancellable(input.limits, Arc::clone(&input.cancel))?;
    let host = Rc::new(RefCell::new(Host {
        vars: input.vars.clone(),
        dirty: Dirty::default(),
        logs: Vec::new(),
        clock: sandbox.clock(),
        cancelled: sandbox.canceller(),
        dynamic: input.dynamic,
    }));
    let response = if input.phase == Phase::Pre { None } else { input.response.clone() };
    let init = json!({
        "request": input.request,
        "response": response,
        "meta": {
            "collectionName": input.collection_name,
            "collectionPath": input.collection_path,
            "executionMode": input.execution_mode,
            "canRunRequest": input.callbacks.is_some(),
        },
    });
    let wrapped = format!("{PREFIX}{}{SUFFIX}", input.script.trim());

    let (error, collected) = sandbox.with(|ctx| -> Result<(Option<ScriptError>, Collected), ScriptError> {
        js(&ctx, install(&ctx, &host, Rc::new(input.collection_path.clone()), input.callbacks.clone()))?;
        js(&ctx, ctx.globals().set("__init", init.to_string()))?;
        let api: Object = js(&ctx, ctx.eval(PRELUDE))?;
        let error = settle(&ctx, &wrapped).err().map(|e| sandbox.explain(e));
        while ctx.execute_pending_job() {}
        let collect: Function = js(&ctx, api.get("collect"))?;
        let json: String = js(&ctx, collect.call(()))?;
        let collected =
            serde_json::from_str(&json).map_err(|e| ScriptError::new(format!("résultat du script : {e}")))?;
        Ok((error, collected))
    })?;

    let host = host.borrow();
    let Collected { request, response, results, assertions, next_request, skip_request, stop_execution } = collected;
    Ok(Output {
        request: ScriptRequest {
            url: request.url,
            method: request.method,
            headers: request.headers,
            data: request.data,
            timeout: request.timeout,
            ..input.request
        },
        max_redirects: request.max_redirects,
        headers_to_delete: request.headers_to_delete,
        disable_json_parsing: request.disable_json_parsing,
        response_data: response.map(|r| r.data),
        vars: host.vars.clone(),
        dirty: host.dirty,
        results,
        assertions,
        logs: host.logs.clone(),
        next_request: match next_request.get("name") {
            None => NextRequest::Unset,
            Some(Value::String(name)) => NextRequest::Named(name.clone()),
            Some(_) => NextRequest::Stop,
        },
        skip_request,
        stop_execution,
        error,
    })
}

/// `path.resolve` de Node, sans toucher au disque : la dernière partie absolue l'emporte, `.` et `..` sont réduits.
fn resolve(parts: &[String]) -> String {
    let mut path = PathBuf::new();
    for part in parts.iter().filter(|p| !p.is_empty()) {
        let next = Path::new(part);
        if next.is_absolute() {
            path = next.to_path_buf();
        } else {
            path.push(next);
        }
    }
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.to_string_lossy().into_owned()
}

/// Source d'un module local de la collection (`.js` ajouté sans extension), refusé s'il sort de la collection : le
/// chemin est résolu depuis la racine de la collection, liens symboliques suivis.
fn load_module(root: &str, module: &str) -> Result<String, String> {
    let missing = || format!("Cannot find module {module}");
    let root = fs::canonicalize(root).map_err(|_| missing())?;
    let mut wanted = root.join(module);
    if wanted.extension().is_none() {
        wanted.set_extension("js");
    }
    let wanted = fs::canonicalize(wanted).map_err(|_| missing())?;
    if !wanted.starts_with(&root) {
        return Err("Access to files outside of the collectionPath is not allowed.".into());
    }
    fs::read_to_string(wanted).map_err(|_| missing())
}

/// Évalue les assertions déclaratives comme Bruno, avec chai : expression gauche en JavaScript sur la réponse et les
/// variables, opérande droit interpolé puis évalué comme littéral, opérateur appliqué par chai.
pub fn assert(specs: &[AssertionSpec], mut input: Input) -> Result<Vec<AssertionOutcome>, ScriptError> {
    if specs.is_empty() {
        return Ok(Vec::new());
    }
    let json = serde_json::to_string(specs).map_err(|e| ScriptError::new(e.to_string()))?;
    input.script = format!("__assert({json});");
    input.phase = Phase::Tests;
    let out = run(input)?;
    match out.error {
        Some(error) => Err(error),
        None => Ok(out.assertions),
    }
}
