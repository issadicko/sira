//! Run d'une collection ou d'un dossier : l'ordre des requêtes, le délai entre deux requêtes, l'arrêt au premier
//! échec, les itérations pilotées par des données et les sauts de `bru.setNextRequest`.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use xc_core::{read_request, TreeItem};

use crate::data::Row;
use crate::pipeline::{run_request, Outcome, Request, RunError, Stage};
use crate::report::{Entry, Summary};
use crate::sequence::next_step;
use crate::session::Session;

/// Une requête à exécuter, d'après l'arbre de la collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Chemin relatif à la racine, avec l'extension.
    pub path: String,
    pub name: String,
    pub method: String,
    pub url: String,
    /// `http`, `graphql`, `grpc` ou `websocket` : seuls les deux premiers s'exécutent pour l'instant.
    pub request_type: String,
    /// Pourquoi le fichier n'a pas pu être lu ; la requête est alors ignorée.
    pub unreadable: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SelectError {
    #[error("aucune requête ni aucun dossier à {0}")]
    NotFound(String),
    #[error("aucune requête à exécuter dans {0}")]
    Empty(String),
}

fn without_extension(path: &str) -> &str {
    path.strip_suffix(".yml").unwrap_or(path)
}

fn collect(items: &[TreeItem], inside: bool, target: &str, out: &mut Vec<Item>, found: &mut bool) {
    for item in items {
        match item {
            TreeItem::Folder { path, children, .. } => {
                let here = inside || path == target;
                *found |= path == target;
                collect(children, here, target, out, found);
            }
            TreeItem::Request { path, name, method, url, request_type, error, .. } => {
                let hit = path == target || without_extension(path) == target;
                *found |= hit;
                if inside || hit {
                    out.push(Item {
                        path: path.clone(),
                        name: name.clone(),
                        method: method.clone(),
                        url: url.clone(),
                        request_type: request_type.clone(),
                        unreadable: error.clone(),
                    });
                }
            }
        }
    }
}

/// Les requêtes HTTP à exécuter, dans l'ordre de l'arbre : toute la collection si `targets` est vide, sinon chaque
/// requête ou dossier (avec ses sous-dossiers) nommé par son chemin relatif, l'un après l'autre.
pub fn select(items: &[TreeItem], targets: &[String]) -> Result<Vec<Item>, SelectError> {
    let mut out = Vec::new();
    if targets.is_empty() {
        collect(items, true, "", &mut out, &mut false);
    }
    for target in targets {
        let target = target.trim_matches('/');
        let (before, mut found) = (out.len(), false);
        collect(items, false, target, &mut out, &mut found);
        if !found {
            return Err(SelectError::NotFound(target.to_owned()));
        }
        if out.len() == before {
            return Err(SelectError::Empty(target.to_owned()));
        }
    }
    if out.is_empty() {
        return Err(SelectError::Empty("la collection".into()));
    }
    Ok(out)
}

impl Item {
    /// `true` pour une requête que le runner sait envoyer (HTTP, GraphQL).
    pub fn is_sendable(&self) -> bool {
        matches!(self.request_type.as_str(), "http" | "graphql")
    }
}

/// Comme [`select`], mais sans les requêtes que le runner ne sait pas envoyer (gRPC, WebSocket) : l'application ne les
/// propose pas, alors que `xc run` les rapporte en erreur comme `bru run`.
pub fn select_sendable(items: &[TreeItem], targets: &[String]) -> Result<Vec<Item>, SelectError> {
    let mut out = select(items, targets)?;
    out.retain(Item::is_sendable);
    if out.is_empty() {
        return Err(SelectError::Empty("la sélection".into()));
    }
    Ok(out)
}

/// Pourquoi une requête n'a pas été exécutée.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// `bru.runner.skipRequest()` dans un script pré-requête.
    Script,
    /// Le run s'est arrêté avant elle sur un échec (`bail`).
    Bail,
    /// Un script a appelé `bru.runner.stopExecution()`.
    StopExecution,
    Unreadable(String),
    /// La requête demande des variables à saisir (`{{?nom}}`) : un run ne peut pas les demander, comme `bru run`.
    Prompts(Vec<String>),
}

/// La requête telle que le run l'a menée, avec son rang d'itération.
pub struct RequestResult {
    pub iteration: usize,
    pub name: String,
    pub path: String,
    pub skip: Option<Skip>,
    pub duration: Duration,
    pub outcome: Outcome,
}

impl RequestResult {
    pub fn entry(&self) -> Entry {
        Entry::of(self)
    }
}

/// Ce qui a interrompu le run avant la fin de ses requêtes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Halt {
    /// `bail` : l'échec de `request`, ce qui reste n'a pas tourné.
    Bail {
        request: String,
        reason: &'static str,
        remaining: usize,
    },
    StopExecution {
        request: String,
        remaining: usize,
    },
    /// Plus de `MAX_JUMPS` sauts : probablement une boucle sans fin.
    Loop,
    Cancelled,
}

pub struct Iteration {
    pub index: usize,
    pub row: Option<Row>,
}

pub struct RunReport {
    pub results: Vec<RequestResult>,
    pub iterations: Vec<Iteration>,
    pub halt: Option<Halt>,
    pub elapsed: Duration,
    pub environment: Option<String>,
}

impl RunReport {
    pub fn entries(&self) -> Vec<Entry> {
        self.results.iter().map(RequestResult::entry).collect()
    }

    pub fn summary(&self) -> Summary {
        Summary::of(&self.entries())
    }

    /// Une requête a échoué ou sorti en erreur, ou le run s'est emballé : le code de sortie du CLI.
    pub fn failed(&self) -> bool {
        self.summary().failed() || self.halt == Some(Halt::Loop)
    }
}

/// Ce que `run_collection` annonce au fil de l'eau, pour un affichage en direct.
pub enum Event<'a> {
    Iteration { index: usize, total: usize, row: Option<&'a Row> },
    Started { iteration: usize, index: usize, item: &'a Item },
    Finished(&'a RequestResult),
    Waiting(Duration),
    Warning(String),
}

pub struct Job<'a> {
    pub root: &'a Path,
    pub collection_name: &'a str,
    pub env: Option<&'a str>,
    pub items: &'a [Item],
    /// Une itération par ligne ; vide : une seule itération, sans données.
    pub rows: &'a [Row],
    /// Au premier échec, ce qui reste est ignoré et le run s'arrête.
    pub bail: bool,
    /// Attente entre deux requêtes.
    pub delay: Duration,
    /// Au-delà de ce nombre de sauts (`bru.setNextRequest`), le run s'arrête : [`MAX_JUMPS`] en usage normal.
    pub max_jumps: usize,
    /// `cli` ou `runner`, ce que `req.getExecutionMode()` répond.
    pub execution_mode: &'a str,
    pub cancel: Arc<AtomicBool>,
}

/// La raison de l'échec d'une requête, dans l'ordre de gravité de Bruno ; `None` si tout a réussi.
fn failure(outcome: &Outcome) -> Option<&'static str> {
    if outcome.error.is_some() {
        Some("request failure")
    } else if outcome.assertions.iter().any(|a| !a.passed) {
        Some("assertion failure")
    } else if outcome.pre.failed() {
        Some("pre-request test failure")
    } else if outcome.post.failed() {
        Some("post-response test failure")
    } else if outcome.tests.failed() {
        Some("test failure")
    } else {
        None
    }
}

fn placeholder(iteration: usize, item: &Item, skip: Skip) -> RequestResult {
    RequestResult {
        iteration,
        name: item.name.clone(),
        path: item.path.clone(),
        skip: Some(skip),
        duration: Duration::ZERO,
        outcome: Outcome::placeholder(&item.method, &item.url),
    }
}

/// Rend la main quand le drapeau d'annulation est levé.
async fn cancelled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Exécute `item` ; `None` si le run a été annulé pendant la requête, que l'envoi en cours abandonne.
async fn run_item(job: &Job<'_>, iteration: usize, item: &Item, session: &mut Session) -> Option<RequestResult> {
    if let Some(reason) = &item.unreadable {
        return Some(placeholder(iteration, item, Skip::Unreadable(reason.clone())));
    }
    if !item.is_sendable() {
        let mut outcome = Outcome::placeholder(&item.method, &item.url);
        outcome.error = Some(RunError {
            stage: Stage::Prepare,
            message: format!("protocole non pris en charge par le runner : {}", item.request_type),
        });
        return Some(RequestResult {
            iteration,
            name: item.name.clone(),
            path: item.path.clone(),
            skip: None,
            duration: Duration::ZERO,
            outcome,
        });
    }
    let started = Instant::now();
    let outcome = match read_request(job.root, &item.path) {
        Ok(doc) => {
            let request = Request {
                root: job.root,
                path: &item.path,
                doc: &doc,
                env: job.env,
                collection_name: job.collection_name,
                execution_mode: job.execution_mode,
                cancel: Arc::clone(&job.cancel),
            };
            tokio::select! {
                outcome = run_request(request, session) => outcome,
                () = cancelled(&job.cancel) => return None,
            }
        }
        Err(e) => {
            let mut outcome = Outcome::placeholder(&item.method, &item.url);
            outcome.error = Some(RunError { stage: Stage::Prepare, message: e.to_string() });
            outcome
        }
    };
    Some(RequestResult {
        iteration,
        name: item.name.clone(),
        path: item.path.clone(),
        skip: outcome.skipped.then(|| match outcome.prompts.as_slice() {
            [] => Skip::Script,
            names => Skip::Prompts(names.to_vec()),
        }),
        duration: started.elapsed(),
        outcome,
    })
}

/// Attend `delay` par petits pas, pour réagir vite à une annulation.
async fn wait(delay: Duration, cancel: &AtomicBool) {
    let end = Instant::now() + delay;
    while !cancel.load(Ordering::Relaxed) {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        tokio::time::sleep(left.min(Duration::from_millis(100))).await;
    }
}

/// Exécute `job` : chaque itération parcourt les requêtes dans l'ordre, un script pouvant sauter ailleurs ou arrêter
/// le run. Les variables que les scripts écrivent restent dans `session` d'une requête et d'une itération à l'autre.
pub async fn run_collection(job: Job<'_>, session: &mut Session, on: &mut (dyn FnMut(Event<'_>) + Send)) -> RunReport {
    let begun = Instant::now();
    let names: Vec<String> = job.items.iter().map(|i| i.name.clone()).collect();
    let rows: Vec<Option<&Row>> = if job.rows.is_empty() { vec![None] } else { job.rows.iter().map(Some).collect() };
    let mut report = RunReport {
        results: Vec::new(),
        iterations: Vec::new(),
        halt: None,
        elapsed: Duration::ZERO,
        environment: job.env.map(str::to_owned),
    };
    let mut jumps = 0;

    'iterations: for (index, row) in rows.iter().enumerate() {
        if let Some(row) = row {
            session.runtime.extend(row.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        report.iterations.push(Iteration { index, row: row.cloned() });
        on(Event::Iteration { index, total: rows.len(), row: *row });

        let mut at = (!job.items.is_empty()).then_some(0);
        while let Some(position) = at {
            if job.cancel.load(Ordering::Relaxed) {
                report.halt = Some(Halt::Cancelled);
                break 'iterations;
            }
            let item = &job.items[position];
            on(Event::Started { iteration: index, index: position, item });
            let Some(result) = run_item(&job, index, item, session).await else {
                report.halt = Some(Halt::Cancelled);
                break 'iterations;
            };
            on(Event::Finished(&result));
            let step = next_step(&names, position, &result.outcome, &mut jumps, job.max_jumps);
            let (stop, failed) = (result.outcome.stop, failure(&result.outcome));
            report.results.push(result);

            let remaining = || job.items.iter().skip(position + 1);
            if job.bail && failed.is_some() {
                let reason = failed.unwrap_or_default();
                let rest: Vec<&Item> = remaining().collect();
                for item in &rest {
                    report.results.push(placeholder(index, item, Skip::Bail));
                }
                report.halt = Some(Halt::Bail { request: item.name.clone(), reason, remaining: rest.len() });
                break 'iterations;
            }
            if stop {
                let rest: Vec<&Item> = remaining().collect();
                for item in &rest {
                    report.results.push(placeholder(index, item, Skip::StopExecution));
                }
                report.halt = Some(Halt::StopExecution { request: item.name.clone(), remaining: rest.len() });
                break 'iterations;
            }
            if let Some(warning) = step.warning {
                on(Event::Warning(warning));
            }
            if step.endless {
                report.halt = Some(Halt::Loop);
                break 'iterations;
            }
            at = step.next;
            let more = at.is_some() || index + 1 < rows.len();
            if more && !job.delay.is_zero() {
                on(Event::Waiting(job.delay));
                wait(job.delay, &job.cancel).await;
            }
        }
    }
    report.elapsed = begun.elapsed();
    report
}
