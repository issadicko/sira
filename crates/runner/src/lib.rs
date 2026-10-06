//! Exécution d'une requête de bout en bout (scripts pré-requête, envoi, scripts post-réponse, assertions, tests), puis
//! d'une collection entière : ordre, délai, arrêt au premier échec, itérations de données et rapports.

mod convert;
mod data;
mod nested;
mod pipeline;
mod report;
mod run;
mod scripts;
mod sequence;
mod session;

pub use data::{parse_csv, parse_json, read_rows, DataError, Row};
pub use pipeline::{run_request, AssertionResult, Outcome, PhaseReport, Request, RunError, Stage};
pub use report::{
    html_page, iso_from_millis, json, junit, now_iso, AssertionEntry, Entry, Meta, Redact, RequestEntry, ResponseEntry,
    Summary, TestEntry,
};
pub use run::{run_collection, select, Event, Halt, Item, Iteration, Job, RequestResult, RunReport, SelectError, Skip};
pub use scripts::{flow_of, merged_script, Flow};
pub use sequence::{next_step, Step, MAX_JUMPS};
pub use session::{EnvWrites, Session};
