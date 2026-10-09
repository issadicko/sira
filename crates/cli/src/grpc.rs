//! `xc grpc` : ouvre une requête gRPC de la collection, envoie ses messages (et ceux de `--send`), demi-ferme le flux
//! d'envoi, affiche les réponses puis le statut. Sert à éprouver une requête sans l'application.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::Args;
use xc_core::{open_collection, read_request};
use xc_engine::GrpcEvent;
use xc_runner::{decode_message, encode_message, open_grpc, status_name, Session};
use xc_secrets::{Keychain, SecretStore};

use crate::run::{input_error, parse_pair, secrets_of, NetArgs};

#[derive(Args)]
pub struct GrpcArgs {
    /// Dossier de la collection (contient opencollection.yml)
    collection: PathBuf,
    /// Chemin de la requête gRPC dans la collection, avec ou sans `.yml`
    request: String,
    /// Environnement à utiliser (nom du fichier dans environments/)
    #[arg(long)]
    env: Option<String>,
    /// Variable runtime, répétable : --env-var nom=valeur
    #[arg(long = "env-var", value_parser = parse_pair)]
    env_vars: Vec<(String, String)>,
    /// Message JSON à envoyer, répétable ; remplace ceux du fichier
    #[arg(long = "send", value_name = "JSON")]
    send: Vec<String>,
    /// Ferme l'appel au bout de ce nombre de secondes, quoi qu'il arrive
    #[arg(long, value_name = "SECONDES")]
    max_time: Option<f64>,
    #[command(flatten)]
    net: NetArgs,
}

fn indent(text: &str) -> String {
    text.lines().collect::<Vec<_>>().join("\n    ")
}

pub async fn grpc(args: GrpcArgs) -> ExitCode {
    grpc_with(args, &Keychain).await
}

pub async fn grpc_with(args: GrpcArgs, store: &dyn SecretStore) -> ExitCode {
    let root = args.collection.as_path();
    let collection = match open_collection(root) {
        Ok(c) => c,
        Err(e) => return input_error(e),
    };
    if let Some(env) = &args.env {
        if !collection.environments.contains(env) {
            return input_error(format!("environnement « {env} » introuvable"));
        }
    }
    let max_time = match args.max_time {
        None => None,
        Some(value) if value.is_finite() && value > 0.0 => Some(Duration::from_secs_f64(value)),
        Some(_) => return input_error("--max-time doit être un nombre de secondes positif"),
    };
    let network = match args.net.prefs() {
        Ok(network) => network,
        Err(e) => return input_error(e),
    };
    let env = args.env.clone().or(collection.default_environment.clone());
    let (secrets, notes) = env.as_deref().map(|name| secrets_of(store, root, name)).unwrap_or_default();
    for note in &notes {
        eprintln!("  ! {note}");
    }
    let mut session = Session { network, secrets, ..Session::default() };
    session.runtime.extend(args.env_vars.iter().map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone()))));

    let path = if Path::new(&args.request).extension().is_some() {
        args.request.clone()
    } else {
        format!("{}.yml", args.request)
    };
    let doc = match read_request(root, &path) {
        Ok(doc) => doc,
        Err(e) => return input_error(e),
    };
    let start = match open_grpc(root, &path, &doc, env.as_deref(), &session).await {
        Ok(start) => start,
        Err(e) => {
            eprintln!("✗ appel impossible : {e}");
            return ExitCode::from(1);
        }
    };
    let mut start = start;
    println!(
        "✓ {} sur {} ({}, schéma : {}) en {:.0} ms",
        start.method.full_name,
        start.call.opened.url,
        start.method.kind(),
        start.schema_source,
        start.call.opened.timings.total_ms
    );
    if !start.unresolved.is_empty() {
        eprintln!("  ! variables non résolues : {}", start.unresolved.join(", "));
    }

    let mut outgoing: Vec<String> =
        if args.send.is_empty() { start.messages.iter().map(|m| m.data.clone()).collect() } else { args.send.clone() };
    if outgoing.is_empty() {
        outgoing.push("{}".into());
    }
    if outgoing.len() > 1 && !start.method.client_streaming {
        start.call.sender.cancel();
        return input_error(format!(
            "{} est un appel {} : un seul message, {} donnés",
            start.method.full_name,
            start.method.kind(),
            outgoing.len()
        ));
    }
    for json in outgoing {
        let bytes = match encode_message(&start.schema, &start.method, &json) {
            Ok(bytes) => bytes,
            Err(e) => {
                start.call.sender.cancel();
                return input_error(e);
            }
        };
        println!("→ message ({} o) : {}", bytes.len(), indent(&json));
        if let Err(e) = start.call.sender.send(bytes) {
            eprintln!("✗ {e}");
            return ExitCode::from(1);
        }
    }
    start.call.sender.finish().ok();

    let ends = max_time.map(|limit| tokio::time::Instant::now() + limit);
    let mut cancelled = false;
    loop {
        let event = match ends {
            Some(end) if !cancelled => match tokio::time::timeout_at(end, start.call.events.recv()).await {
                Ok(event) => event,
                Err(_) => {
                    println!("→ annulation (--max-time)");
                    start.call.sender.cancel();
                    cancelled = true;
                    continue;
                }
            },
            _ => start.call.events.recv().await,
        };
        match event {
            Some(GrpcEvent::Headers { .. }) => {}
            Some(GrpcEvent::Message { data }) => {
                println!(
                    "← message ({} o) : {}",
                    data.len(),
                    indent(&decode_message(&start.schema, &start.method, &data))
                );
            }
            Some(GrpcEvent::Status { code, message, metadata }) => {
                let suffix = if message.is_empty() { String::new() } else { format!(" : {message}") };
                if code == 0 {
                    println!("← statut OK");
                } else {
                    println!("← statut {} ({code}){suffix}", status_name(code));
                }
                for (name, value) in metadata {
                    println!("  {name}: {value}");
                }
                return ExitCode::from(u8::from(code != 0));
            }
            Some(GrpcEvent::Error { message }) => {
                println!("← erreur : {message}");
                return ExitCode::from(1);
            }
            None => {
                println!("← erreur : la connexion s'est fermée sans statut");
                return ExitCode::from(1);
            }
        }
    }
}
