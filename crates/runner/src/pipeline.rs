use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Map, Value};
use xc_core::vars::{dynamic_value, Context, Scope};
use xc_core::{prepare_with, Overrides, RequestDoc};
use xc_engine::HttpResponse;
use xc_script::{AssertionSpec, Input, Limits, LogLine, NextRequest, Output, Phase, ScriptRequest, TestResult, Vars};

use crate::convert::{apply_request, body_bytes, script_request, script_response};
use crate::nested::Nested;
use crate::scripts::{merged_script, AFTER_RESPONSE, BEFORE_REQUEST, TESTS};
use crate::session::{EnvWrites, Session};

/// Où la requête s'est arrêtée quand elle n'a pas abouti.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    Prepare,
    PreRequestScript,
    Send,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunError {
    pub stage: Stage,
    pub message: String,
}

/// Ce que les scripts d'une phase ont produit.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PhaseReport {
    pub results: Vec<TestResult>,
    pub logs: Vec<LogLine>,
    pub error: Option<String>,
}

impl PhaseReport {
    pub fn failed(&self) -> bool {
        self.error.is_some() || self.results.iter().any(|r| r.status != "pass")
    }
}

/// Résultat d'une assertion déclarative : ce qui est évalué, ce qui est attendu, la valeur reçue et pourquoi elle échoue.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionResult {
    pub expression: String,
    pub operator: String,
    pub expected: Option<String>,
    pub actual: String,
    pub passed: bool,
    pub error: Option<String>,
}

pub struct Outcome {
    pub method: String,
    pub url: String,
    pub unresolved: Vec<String>,
    pub response: Option<HttpResponse>,
    pub pre: PhaseReport,
    pub post: PhaseReport,
    pub tests: PhaseReport,
    pub assertions: Vec<AssertionResult>,
    pub next_request: NextRequest,
    pub skipped: bool,
    pub stop: bool,
    pub error: Option<RunError>,
}

impl Outcome {
    pub(crate) fn new(doc: &RequestDoc) -> Self {
        Self {
            method: doc.method.to_uppercase(),
            url: doc.url.clone(),
            unresolved: Vec::new(),
            response: None,
            pre: PhaseReport::default(),
            post: PhaseReport::default(),
            tests: PhaseReport::default(),
            assertions: Vec::new(),
            next_request: NextRequest::Unset,
            skipped: false,
            stop: false,
            error: None,
        }
    }

    fn fail(mut self, stage: Stage, message: impl ToString) -> Self {
        self.error = Some(RunError { stage, message: message.to_string() });
        self
    }

    /// Le résultat d'une requête qui n'a pas pu être lue ni préparée.
    pub fn failed(doc: &RequestDoc, stage: Stage, message: impl ToString) -> Self {
        Self::new(doc).fail(stage, message)
    }

    /// Tout a réussi : envoi, scripts, assertions et tests.
    pub fn passed(&self) -> bool {
        self.error.is_none()
            && !self.pre.failed()
            && !self.post.failed()
            && !self.tests.failed()
            && self.assertions.iter().all(|a| a.passed)
    }
}

pub struct Request<'a> {
    pub root: &'a Path,
    pub path: &'a str,
    pub doc: &'a RequestDoc,
    pub env: Option<&'a str>,
    pub collection_name: &'a str,
    /// `standalone` (application), `runner` ou `cli`, ce que `req.getExecutionMode()` répond.
    pub execution_mode: &'a str,
    pub cancel: Arc<AtomicBool>,
}

fn typed(pairs: Vec<(String, String)>) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k, Value::String(v))).collect()
}

/// Les variables que les scripts voient : celles des fichiers, recouvertes par ce que des scripts ont déjà écrit.
fn vars_for(scope: &Scope, session: &Session, env: Option<&str>) -> Vars {
    let mut env_vars = session.env_for(env).cloned().unwrap_or_else(|| typed(scope.vars_of("Environnement")));
    if let Some(name) = env {
        env_vars.insert(xc_script::ENV_NAME.into(), Value::String(name.to_owned()));
    }
    let mut process_env: Map<String, Value> = std::env::vars().map(|(k, v)| (k, Value::String(v))).collect();
    process_env.extend(scope.dotenv().iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))));
    Vars {
        env: env_vars,
        runtime: session.runtime.clone(),
        global: session.global.clone(),
        collection: session.collection.clone().unwrap_or_else(|| typed(scope.vars_of("Collection"))),
        folder: typed(scope.vars_of("Dossier")),
        request: typed(scope.vars_of("Requête")),
        oauth2: Map::new(),
        process_env,
    }
}

async fn script(input: Input) -> Result<Output, String> {
    tokio::task::spawn_blocking(move || xc_script::run(input))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn remember(session: &mut Session, out: &Output, env: Option<&str>) {
    session.runtime = out.vars.runtime.clone();
    session.global = out.vars.global.clone();
    if out.dirty.env {
        session.env = Some(EnvWrites { name: env.map(str::to_owned), vars: out.vars.env.clone() });
    }
    if out.dirty.collection {
        session.collection = Some(out.vars.collection.clone());
    }
}

fn report(out: &Output) -> PhaseReport {
    PhaseReport {
        results: out.results.clone(),
        logs: out.logs.clone(),
        error: out.error.as_ref().map(|e| e.message.clone()),
    }
}

struct Phases<'a> {
    request: &'a Request<'a>,
    scope: &'a Scope,
    ctx: &'a Context,
    depth: usize,
}

impl Phases<'_> {
    fn input(
        &self,
        phase: Phase,
        code: String,
        session: &Session,
        request: ScriptRequest,
        response: Option<xc_script::ScriptResponse>,
    ) -> Input {
        let r = self.request;
        Input {
            phase,
            script: code,
            request,
            response,
            vars: vars_for(self.scope, session, r.env),
            collection_name: r.collection_name.to_owned(),
            collection_path: r.root.display().to_string(),
            execution_mode: r.execution_mode.to_owned(),
            dynamic: dynamic_value,
            limits: Limits::default(),
            cancel: Arc::clone(&r.cancel),
            callbacks: None,
        }
    }

    fn with_nested_requests(&self, mut input: Input) -> Input {
        let r = self.request;
        input.callbacks = Some(Arc::new(Nested {
            handle: tokio::runtime::Handle::current(),
            root: r.root.to_path_buf(),
            env: r.env.map(str::to_owned),
            collection_name: r.collection_name.to_owned(),
            cancel: Arc::clone(&r.cancel),
            depth: self.depth,
        }));
        input
    }

    async fn run(
        &self,
        phase: Phase,
        kind: &str,
        session: &Session,
        request: ScriptRequest,
        response: Option<xc_script::ScriptResponse>,
    ) -> Result<Output, String> {
        let r = self.request;
        let mut code = merged_script(self.ctx, r.root, r.path, r.doc, kind);
        if kind == AFTER_RESPONSE {
            code = format!("{}{code}", post_variables_call(r.doc));
        }
        script(self.with_nested_requests(self.input(phase, code, session, request, response))).await
    }

    async fn assertions(
        &self,
        specs: Vec<AssertionSpec>,
        session: &Session,
        request: ScriptRequest,
        response: xc_script::ScriptResponse,
    ) -> Vec<AssertionResult> {
        let input = self.input(Phase::Tests, String::new(), session, request, Some(response));
        let failed = |message: String, spec: &AssertionSpec| AssertionResult {
            expression: spec.expression.clone(),
            operator: spec.operator.clone(),
            expected: spec.value.clone(),
            actual: "undefined".into(),
            passed: false,
            error: Some(message),
        };
        let evaluated = {
            let specs = specs.clone();
            tokio::task::spawn_blocking(move || xc_script::assert(&specs, input)).await
        };
        match evaluated {
            Ok(Ok(outcomes)) => outcomes
                .into_iter()
                .map(|o| AssertionResult {
                    expression: o.expression,
                    operator: o.operator,
                    expected: o.value,
                    actual: o.actual,
                    passed: o.passed,
                    error: o.error,
                })
                .collect(),
            Ok(Err(e)) => specs.iter().map(|spec| failed(e.message.clone(), spec)).collect(),
            Err(e) => specs.iter().map(|spec| failed(e.to_string(), spec)).collect(),
        }
    }
}

/// L'appel qui pose les variables « après la réponse » du fichier, avant les scripts de la phase.
fn post_variables_call(doc: &RequestDoc) -> String {
    let specs: Vec<Value> = doc
        .post_variables
        .iter()
        .filter(|v| v.enabled)
        .map(|v| serde_json::json!({ "name": v.name, "expression": v.expression }))
        .collect();
    if specs.is_empty() {
        String::new()
    } else {
        format!("__vars({});\n", Value::Array(specs))
    }
}

fn sent_request(before: &ScriptRequest, prepared: &xc_engine::HttpRequest) -> ScriptRequest {
    ScriptRequest {
        method: prepared.method.clone(),
        url: prepared.url.clone(),
        headers: prepared.headers.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect(),
        data: prepared.body.as_ref().map(|b| Value::String(xc_engine::lossy_text(b.clone()))),
        timeout: Some(prepared.timeout.as_millis() as u64).filter(|_| false),
        ..before.clone()
    }
}

fn steer(out: &mut Outcome, script: &Output) {
    if script.next_request != NextRequest::Unset {
        out.next_request = script.next_request.clone();
    }
    out.stop |= script.stop_execution;
}

/// Exécute une requête de bout en bout comme Bruno : script pré-requête, envoi, script post-réponse, assertions et
/// tests. Les variables que les scripts écrivent restent dans `session` pour les requêtes suivantes.
pub async fn run_request(req: Request<'_>, session: &mut Session) -> Outcome {
    run_request_at(req, session, 0).await
}

/// [`run_request`] à la profondeur `depth` de `bru.runRequest` imbriqués.
pub(crate) async fn run_request_at(req: Request<'_>, session: &mut Session, depth: usize) -> Outcome {
    let mut out = Outcome::new(req.doc);
    let ctx = match Context::load(req.root, req.path) {
        Ok(ctx) => ctx,
        Err(e) => return out.fail(Stage::Prepare, e),
    };
    let scope = match Scope::build(req.root, &ctx, req.path, req.doc, req.env, &session.runtime_strings()) {
        Ok(scope) => scope,
        Err(e) => return out.fail(Stage::Prepare, e),
    };
    let phases = Phases { request: &req, scope: &scope, ctx: &ctx, depth };
    let before = script_request(&ctx, req.doc);

    let pre = match phases.run(Phase::Pre, BEFORE_REQUEST, session, before.clone(), None).await {
        Ok(pre) => pre,
        Err(e) => return out.fail(Stage::PreRequestScript, e),
    };
    remember(session, &pre, req.env);
    out.pre = report(&pre);
    out.skipped = pre.skip_request;
    steer(&mut out, &pre);
    if let Some(error) = &pre.error {
        return out.fail(Stage::PreRequestScript, &error.message);
    }
    if pre.skip_request {
        return out;
    }

    let (doc, headers) = apply_request(req.doc, &before, &pre.request);
    let overrides = Overrides { headers, vars: session.overrides(req.env) };
    let prepared = match prepare_with(req.root, req.path, &doc, req.env, &session.runtime_strings(), overrides) {
        Ok(prepared) => prepared,
        Err(e) => return out.fail(Stage::Prepare, e),
    };
    out.method.clone_from(&prepared.request.method);
    out.url.clone_from(&prepared.request.url);
    out.unresolved = prepared.unresolved;
    let sent = sent_request(&pre.request, &prepared.request);

    let mut response = match xc_engine::send(prepared.request).await {
        Ok(response) => response,
        Err(e) => return out.fail(Stage::Send, e),
    };

    let parse_json = !pre.disable_json_parsing;
    let view = script_response(&response, &out.url, parse_json);
    match phases.run(Phase::Post, AFTER_RESPONSE, session, sent.clone(), Some(view)).await {
        Ok(post) => {
            remember(session, &post, req.env);
            out.post = report(&post);
            steer(&mut out, &post);
            if let Some(data) = &post.response_data {
                response.body = body_bytes(data);
            }
        }
        Err(e) => out.post.error = Some(e),
    }

    let view = script_response(&response, &out.url, parse_json);
    let specs = doc
        .assertions
        .iter()
        .filter(|a| a.enabled)
        .map(|a| AssertionSpec {
            expression: a.expression.clone(),
            operator: a.operator.clone(),
            value: a.value.clone(),
        })
        .collect();
    out.assertions = phases.assertions(specs, session, sent.clone(), view.clone()).await;
    match phases.run(Phase::Tests, TESTS, session, sent, Some(view)).await {
        Ok(tests) => {
            remember(session, &tests, req.env);
            out.tests = report(&tests);
            steer(&mut out, &tests);
        }
        Err(e) => out.tests.error = Some(e),
    }
    out.response = Some(response);
    out
}
