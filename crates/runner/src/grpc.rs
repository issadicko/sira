//! Ouvrir une requête gRPC du fichier : variables, métadonnées, auth, réglages réseau, schéma protobuf (fichiers `.proto`
//! de la requête ou de la collection, sinon réflexion du serveur), puis l'appel. Les scripts ne tournent pas sur un appel
//! gRPC (Bruno non plus).

use std::path::{Path, PathBuf};
use std::time::Duration;

use xc_core::vars::Context;
use xc_core::{prepare_with, protobuf_config, PreparedMessage, RequestDoc, SendAuth};
use xc_engine::{connect_grpc, GrpcCall, GrpcEvent, GrpcRequest, Network};
use xc_proto::{
    file_by_symbol_request, list_services_request, parse_reflection_response, MethodInfo, ReflectionAnswer, Schema,
    REFLECTION_METHODS,
};

use crate::session::Session;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const REFLECTION_TIMEOUT: Duration = Duration::from_secs(15);

/// Un appel ouvert, avec le schéma pour lire ses messages et les messages du fichier prêts à partir.
#[derive(Debug)]
pub struct GrpcStart {
    pub call: GrpcCall,
    pub schema: Schema,
    pub method: MethodInfo,
    /// D'où vient le schéma : le fichier `.proto` ou « réflexion du serveur ».
    pub schema_source: String,
    /// Les messages du fichier, variables résolues, dans l'ordre (JSON, pas encore encodé).
    pub messages: Vec<PreparedMessage>,
    pub unresolved: Vec<String>,
}

fn auth_name(auth: &SendAuth) -> Option<&'static str> {
    match auth {
        SendAuth::None => None,
        SendAuth::Digest { .. } => Some("Digest"),
        SendAuth::Aws(_) => Some("AWS Signature V4"),
        SendAuth::Oauth2(_) => Some("OAuth 2.0"),
    }
}

fn absolute(root: &Path, relative: &str) -> PathBuf {
    xc_proto::resolve(root, relative)
}

/// Le schéma des fichiers `.proto` de la requête, ou à défaut ceux de la collection ; `None` quand aucun n'est déclaré.
fn local_schema(root: &Path, ctx: &Context, doc: &RequestDoc) -> Result<Option<(Schema, String)>, String> {
    let config = protobuf_config(&ctx.collection);
    let files: Vec<String> =
        if doc.proto_file.is_empty() { config.proto_files.clone() } else { vec![doc.proto_file.clone()] };
    if files.is_empty() {
        return Ok(None);
    }
    let mut includes: Vec<PathBuf> = config.import_paths.iter().map(|path| absolute(root, path)).collect();
    includes.push(root.to_path_buf());
    let paths: Vec<PathBuf> = files.iter().map(|file| absolute(root, file)).collect();
    let schema = Schema::from_files(&paths, &includes).map_err(|e| e.to_string())?;
    Ok(Some((schema, files.join(", "))))
}

/// Le schéma que le serveur décrit lui-même (réflexion gRPC `v1`, puis `v1alpha` pour les anciens serveurs).
pub async fn reflect(url: &str, metadata: &[(String, String)], network: &Network) -> Result<Schema, String> {
    let mut last = String::from("réflexion indisponible");
    for method in REFLECTION_METHODS {
        match tokio::time::timeout(REFLECTION_TIMEOUT, reflect_with(url, method, metadata, network)).await {
            Ok(Ok(schema)) => return Ok(schema),
            Ok(Err(error)) => last = error,
            Err(_) => last = "la réflexion n'a pas répondu à temps".into(),
        }
    }
    Err(last)
}

async fn reflect_with(
    url: &str,
    method: &str,
    metadata: &[(String, String)],
    network: &Network,
) -> Result<Schema, String> {
    let request = GrpcRequest {
        url: url.to_owned(),
        method: method.to_owned(),
        metadata: metadata.to_vec(),
        timeout: CONNECT_TIMEOUT,
        deadline: None,
        network: network.clone(),
    };
    let mut call = connect_grpc(request).await.map_err(|e| e.to_string())?;
    call.sender.send(list_services_request()).map_err(|e| e.to_string())?;
    let mut files: Vec<Vec<u8>> = Vec::new();
    let mut pending = 0usize;
    let mut listed = false;
    while let Some(event) = call.events.recv().await {
        match event {
            GrpcEvent::Message { data } => match parse_reflection_response(&data).map_err(|e| e.to_string())? {
                ReflectionAnswer::Services(names) => {
                    listed = true;
                    pending = names.len();
                    for name in &names {
                        call.sender.send(file_by_symbol_request(name)).map_err(|e| e.to_string())?;
                    }
                    if names.is_empty() {
                        break;
                    }
                }
                ReflectionAnswer::Files(mut received) => {
                    files.append(&mut received);
                    pending = pending.saturating_sub(1);
                }
                ReflectionAnswer::Error { message, .. } => {
                    if !listed {
                        return Err(format!("réflexion refusée : {message}"));
                    }
                    pending = pending.saturating_sub(1);
                }
                ReflectionAnswer::Other => {}
            },
            GrpcEvent::Status { code, message, .. } if code != 0 => {
                return Err(format!("réflexion indisponible ({} : {message})", status_name(code)));
            }
            GrpcEvent::Error { message } => return Err(message),
            _ => {}
        }
        if listed && pending == 0 {
            break;
        }
    }
    call.sender.cancel();
    if files.is_empty() {
        return Err("le serveur n'a décrit aucun service".into());
    }
    Schema::from_descriptors(&files).map_err(|e| e.to_string())
}

/// Prépare `doc` avec `session` (variables du fichier et de l'environnement `env`, réglages réseau) et ouvre l'appel. Une
/// authentification qui demande un échange avec le serveur (Digest, AWS, OAuth 2.0) est refusée plutôt qu'ignorée.
pub async fn open_grpc(
    root: &Path,
    path: &str,
    doc: &RequestDoc,
    env: Option<&str>,
    session: &Session,
) -> Result<GrpcStart, String> {
    if doc.request_type != "grpc" {
        return Err(format!("« {} » n'est pas une requête gRPC (type : {})", doc.name, doc.request_type));
    }
    let overrides = session.request_overrides(None, env);
    let prepared =
        prepare_with(root, path, doc, env, &session.runtime_strings(), overrides).map_err(|e| e.to_string())?;
    if let Some(name) = auth_name(&prepared.auth) {
        return Err(format!("l'authentification {name} n'est pas prise en charge sur un appel gRPC"));
    }
    let method_name = prepared.request.method.clone();
    if method_name.trim().is_empty() {
        return Err("aucune méthode : écrire « paquet.Service/Méthode »".into());
    }
    let ctx = Context::load(root, path).map_err(|e| e.to_string())?;
    let local = local_schema(root, &ctx, doc)?;
    let (schema, schema_source) = match local {
        Some((schema, source)) if schema.method(&method_name).is_ok() => (schema, source),
        Some(_) if !doc.proto_file.is_empty() => {
            return Err(format!("la méthode {method_name} n'existe pas dans {}", doc.proto_file));
        }
        _ => {
            let schema = reflect(&prepared.request.url, &prepared.request.headers, &prepared.request.network)
                .await
                .map_err(|e| format!("aucun fichier .proto pour {method_name} et {e}"))?;
            (schema, "réflexion du serveur".to_owned())
        }
    };
    let method = schema.method(&method_name).map_err(|e| e.to_string())?;
    let request = GrpcRequest {
        url: prepared.request.url.clone(),
        method: method.full_name.clone(),
        metadata: prepared.request.headers.clone(),
        timeout: doc.timeout_ms.map_or(CONNECT_TIMEOUT, Duration::from_millis),
        deadline: None,
        network: prepared.request.network.clone(),
    };
    let call = connect_grpc(request).await.map_err(|e| e.to_string())?;
    Ok(GrpcStart { call, schema, method, schema_source, messages: prepared.messages, unresolved: prepared.unresolved })
}

/// Le JSON d'un message, converti en octets protobuf selon le type d'entrée de la méthode.
pub fn encode_message(schema: &Schema, method: &MethodInfo, json: &str) -> Result<Vec<u8>, String> {
    schema.encode_request(&method.full_name, json).map_err(|e| e.to_string())
}

/// Un message reçu, en JSON lisible ; une erreur de lecture est dite à la place du message.
pub fn decode_message(schema: &Schema, method: &MethodInfo, bytes: &[u8]) -> String {
    schema.decode_response(&method.full_name, bytes).unwrap_or_else(|e| e.to_string())
}

/// Le nom du code de statut gRPC (`NOT_FOUND`, `UNAVAILABLE`…).
pub const fn status_name(code: u32) -> &'static str {
    match code {
        0 => "OK",
        1 => "CANCELLED",
        2 => "UNKNOWN",
        3 => "INVALID_ARGUMENT",
        4 => "DEADLINE_EXCEEDED",
        5 => "NOT_FOUND",
        6 => "ALREADY_EXISTS",
        7 => "PERMISSION_DENIED",
        8 => "RESOURCE_EXHAUSTED",
        9 => "FAILED_PRECONDITION",
        10 => "ABORTED",
        11 => "OUT_OF_RANGE",
        12 => "UNIMPLEMENTED",
        13 => "INTERNAL",
        14 => "UNAVAILABLE",
        15 => "DATA_LOSS",
        16 => "UNAUTHENTICATED",
        _ => "UNKNOWN_CODE",
    }
}
