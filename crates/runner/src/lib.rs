//! Exécution d'une requête de bout en bout (scripts pré-requête, envoi, scripts post-réponse, assertions, tests), puis
//! d'une collection entière : ordre, délai, arrêt au premier échec, itérations de données et rapports.

mod auth;
mod convert;
mod data;
mod envfile;
mod filter;
mod grpc;
mod nested;
mod oauth2;
mod persist;
mod pipeline;
mod report;
mod run;
mod schema;
mod scripts;
mod sequence;
mod session;
mod websocket;

pub use data::{parse_csv, parse_json, read_rows, DataError, Row};
pub use envfile::load_env_file;
pub use filter::{filter_items, has_executable_test, Filter};
pub use grpc::{decode_message, describe_grpc, encode_message, open_grpc, reflect, status_name, GrpcStart};
pub use oauth2::{
    fetch as fetch_oauth2_token, redirect_params, refresh as refresh_oauth2_token, token_key, Authorization,
    AuthorizationRequest, Authorizer, SharedAuthorizer, Token, TokenInfo,
};
pub use persist::{persist_variables, Persisted};
pub use pipeline::{run_request, AssertionResult, Outcome, PhaseReport, Request, RunError, Stage};
pub use report::{
    html_page, json, junit, now_iso, AssertionEntry, Entry, Meta, Redact, RequestEntry, ResponseEntry, Summary,
    TestEntry,
};
pub use run::{
    run_collection, select, select_sendable, Event, Halt, Item, Iteration, Job, RequestResult, RunReport, SelectError,
    Skip,
};
pub use schema::{fetch_schema, schema_url, SchemaSource};
pub use scripts::{flow_of, merged_script, Flow};
pub use sequence::{next_step, Step, MAX_JUMPS};
pub use session::{EnvWrites, Session};
pub use websocket::{describe_opened, open_websocket, resolve_message, WsStart};
pub use xc_engine::iso_from_millis;
