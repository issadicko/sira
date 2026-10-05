//! Exécution d'une requête de bout en bout : scripts pré-requête, envoi, scripts post-réponse, assertions, tests.

mod convert;
mod pipeline;
mod scripts;
mod sequence;
mod session;

pub use pipeline::{run_request, AssertionResult, Outcome, PhaseReport, Request, RunError, Stage};
pub use scripts::{flow_of, merged_script, Flow};
pub use sequence::{next_step, Step, MAX_JUMPS};
pub use session::{EnvWrites, Session};
