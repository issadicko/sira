use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rquickjs::context::EvalOptions;
use rquickjs::{CatchResultExt, CaughtError, Context, Ctx, Promise, Runtime, Value};

/// Plafonds d'un script : mémoire, pile et temps de calcul (l'attente d'un `sleep` ou d'une requête n'en fait pas partie).
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub memory: usize,
    pub stack: usize,
    pub compute: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self { memory: 256 << 20, stack: 8 << 20, compute: Duration::from_secs(30) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ScriptError {
    pub message: String,
    pub stack: Option<String>,
}

impl ScriptError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into(), stack: None }
    }
}

impl From<CaughtError<'_>> for ScriptError {
    fn from(e: CaughtError<'_>) -> Self {
        match e {
            CaughtError::Exception(ex) => {
                let message = ex.message().unwrap_or_default();
                Self { message: if message.is_empty() { ex.to_string() } else { message }, stack: ex.stack() }
            }
            CaughtError::Value(v) => {
                Self::new(v.as_string().and_then(|s| s.to_string().ok()).unwrap_or_else(|| format!("{v:?}")))
            }
            CaughtError::Error(e) => Self::new(e.to_string()),
        }
    }
}

/// Horloge de calcul : le temps passé dans l'hôte (attente d'une requête, `sleep`) est retiré du temps écoulé.
#[derive(Clone)]
pub struct Clock {
    started: Instant,
    waited: Arc<AtomicU64>,
}

impl Clock {
    fn new() -> Self {
        Self { started: Instant::now(), waited: Arc::default() }
    }

    fn computing(&self) -> Duration {
        self.started.elapsed().saturating_sub(Duration::from_micros(self.waited.load(Ordering::Relaxed)))
    }

    /// Exécute `wait` (une attente de l'hôte) sans la compter dans le temps de calcul.
    pub fn pause<T>(&self, wait: impl FnOnce() -> T) -> T {
        let begun = Instant::now();
        let out = wait();
        self.waited.fetch_add(u64::try_from(begun.elapsed().as_micros()).unwrap_or(u64::MAX), Ordering::Relaxed);
        out
    }
}

/// Un contexte QuickJS isolé : aucun accès au disque, au réseau ni aux processus, hors des fonctions que l'hôte y pose.
pub struct Sandbox {
    runtime: Runtime,
    context: Context,
    cancelled: Arc<AtomicBool>,
    timed_out: Arc<AtomicBool>,
    clock: Clock,
    limits: Limits,
}

impl Sandbox {
    pub fn new(limits: Limits) -> Result<Self, ScriptError> {
        let init = |e: rquickjs::Error| ScriptError::new(format!("moteur de scripts : {e}"));
        let runtime = Runtime::new().map_err(init)?;
        runtime.set_memory_limit(limits.memory);
        runtime.set_max_stack_size(limits.stack);
        let (cancelled, timed_out) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
        let clock = Clock::new();
        let (stop, late, time) = (Arc::clone(&cancelled), Arc::clone(&timed_out), clock.clone());
        runtime.set_interrupt_handler(Some(Box::new(move || {
            let over = time.computing() > limits.compute;
            late.fetch_or(over, Ordering::Relaxed);
            over || stop.load(Ordering::Relaxed)
        })));
        let context = Context::full(&runtime).map_err(init)?;
        Ok(Self { runtime, context, cancelled, timed_out, clock, limits })
    }

    /// Interrompt le script en cours depuis un autre fil (annulation de la requête).
    pub fn canceller(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancelled)
    }

    pub fn clock(&self) -> Clock {
        self.clock.clone()
    }

    pub fn with<R>(&self, f: impl FnOnce(Ctx<'_>) -> R) -> R {
        self.context.with(f)
    }

    /// Évalue `source` en mode non strict, comme Bruno ; s'il rend une promesse, exécute les tâches en attente jusqu'à
    /// son règlement.
    pub fn run(&self, source: &str) -> Result<(), ScriptError> {
        self.context.with(|ctx| settle(&ctx, source)).map_err(|e| self.explain(e))
    }

    /// Dit pourquoi un script s'est arrêté quand c'est le sandbox qui l'a interrompu.
    pub fn explain(&self, error: ScriptError) -> ScriptError {
        if self.cancelled.load(Ordering::Relaxed) {
            ScriptError::new("script interrompu")
        } else if self.timed_out.load(Ordering::Relaxed) {
            ScriptError::new(format!("script interrompu : plus de {} s de calcul", self.limits.compute.as_secs_f64()))
        } else {
            error
        }
    }

    pub fn drain(&self) {
        while self.runtime.execute_pending_job().unwrap_or(false) {}
    }
}

/// Évalue `source` (une promesse, en général : le script enveloppé dans une fonction asynchrone) puis attend son
/// règlement.
pub fn settle<'js>(ctx: &Ctx<'js>, source: &str) -> Result<(), ScriptError> {
    let mut options = EvalOptions::default();
    options.strict = false;
    let promise: Promise = ctx.eval_with_options(source, options).catch(ctx)?;
    promise.finish::<Value>().catch(ctx)?;
    Ok(())
}
