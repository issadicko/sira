//! Introspection GraphQL : le schéma du serveur d'une requête, demandé avec ses en-têtes, son auth et ses variables.
//! Aucun script ne tourne : comme Bruno, seuls les en-têtes, l'auth et les variables comptent.

use std::path::Path;

use xc_core::graphql::{self, StoredSchema};
use xc_core::{prepare_with, RequestDoc};

use crate::session::Session;

const INTROSPECTION_TIMEOUT_MS: u64 = 30_000;

pub struct SchemaSource<'a> {
    pub root: &'a Path,
    pub path: &'a str,
    pub doc: &'a RequestDoc,
    pub env: Option<&'a str>,
}

/// L'URL résolue de la requête : c'est elle qui désigne le schéma gardé.
pub fn schema_url(source: &SchemaSource<'_>, session: &Session) -> Result<String, String> {
    Ok(prepare(source, session)?.request.url)
}

fn prepare(source: &SchemaSource<'_>, session: &Session) -> Result<xc_core::Prepared, String> {
    let mut doc = source.doc.clone();
    doc.request_type = "graphql".into();
    doc.method = "POST".into();
    doc.body = graphql::introspection();
    doc.timeout_ms = doc.timeout_ms.or(Some(INTROSPECTION_TIMEOUT_MS));
    let overrides = session.request_overrides(None, source.env);
    prepare_with(source.root, source.path, &doc, source.env, &session.runtime_strings(), overrides)
        .map_err(|e| e.to_string())
}

/// Demande le schéma au serveur, le garde dans `.oc-sync/graphql/` et le rend.
pub async fn fetch_schema(source: SchemaSource<'_>, session: &mut Session) -> Result<StoredSchema, String> {
    let prepared = prepare(&source, session)?;
    let url = prepared.request.url.clone();
    let sent = crate::auth::send(prepared.request, &prepared.auth, session).await?;
    let status = sent.response.status;
    let body = xc_engine::lossy_text(sent.response.body);
    let introspection = graphql::schema_from_response(status, &body)?;
    let schema = StoredSchema { url, fetched_at: crate::report::now_iso(), introspection };
    graphql::store(source.root, &schema).map_err(|e| e.to_string())?;
    Ok(schema)
}
