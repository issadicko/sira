//! `xc ws` : ouvre une requête WebSocket de la collection, envoie ses messages cochés (et ceux de `--send`), affiche ce
//! qui arrive, puis ferme après un temps sans message. Sert à éprouver une requête sans l'application.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::Args;
use xc_core::{open_collection, read_request};
use xc_engine::{WsEvent, WsOutgoing};
use xc_runner::{describe_opened, open_websocket, Session};
use xc_secrets::{Keychain, SecretStore};

use crate::run::{input_error, parse_pair, secrets_of, NetArgs};

#[derive(Args)]
pub struct WsArgs {
    /// Dossier de la collection (contient opencollection.yml)
    collection: PathBuf,
    /// Chemin de la requête WebSocket dans la collection, avec ou sans `.yml`
    request: String,
    /// Environnement à utiliser (nom du fichier dans environments/)
    #[arg(long)]
    env: Option<String>,
    /// Variable runtime, répétable : --env-var nom=valeur
    #[arg(long = "env-var", value_parser = parse_pair)]
    env_vars: Vec<(String, String)>,
    /// Texte à envoyer après la connexion, répétable, après les messages cochés du fichier
    #[arg(long = "send", value_name = "TEXTE")]
    send: Vec<String>,
    /// N'envoie pas les messages cochés du fichier
    #[arg(long)]
    no_send: bool,
    /// Ferme la connexion après ce nombre de secondes sans rien recevoir
    #[arg(long, value_name = "SECONDES", default_value_t = 2.0)]
    idle: f64,
    /// Ferme la connexion au bout de ce nombre de secondes, quoi qu'il arrive
    #[arg(long, value_name = "SECONDES")]
    max_time: Option<f64>,
    #[command(flatten)]
    net: NetArgs,
}

const SHOWN_BYTES: usize = 32;

fn hex(data: &[u8]) -> String {
    let mut out: Vec<String> = data.iter().take(SHOWN_BYTES).map(|b| format!("{b:02x}")).collect();
    if data.len() > SHOWN_BYTES {
        out.push("…".into());
    }
    out.join(" ")
}

fn indent(text: &str) -> String {
    text.lines().collect::<Vec<_>>().join("\n    ")
}

fn show(event: &WsEvent) {
    match event {
        WsEvent::Text { data } => println!("← texte ({} o) : {}", data.len(), indent(data)),
        WsEvent::Binary { data } => println!("← binaire ({} o) : {}", data.len(), hex(data)),
        WsEvent::Ping { data } => println!("← ping ({} o)", data.len()),
        WsEvent::Pong { data } => println!("← pong ({} o)", data.len()),
        WsEvent::Close { code, reason } => {
            let code = code.map_or_else(|| "sans code".to_owned(), |c| c.to_string());
            println!("← fermeture ({code}){}", if reason.is_empty() { String::new() } else { format!(" : {reason}") });
        }
        WsEvent::Error { message } => println!("← erreur : {message}"),
    }
}

fn seconds(value: f64) -> Option<Duration> {
    (value.is_finite() && value >= 0.0).then(|| Duration::from_secs_f64(value))
}

pub async fn ws(args: WsArgs) -> ExitCode {
    ws_with(args, &Keychain).await
}

pub async fn ws_with(args: WsArgs, store: &dyn SecretStore) -> ExitCode {
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
    let (Some(idle), max_time) = (seconds(args.idle), args.max_time.and_then(seconds)) else {
        return input_error("--idle doit être un nombre de secondes positif");
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
    let mut start = match open_websocket(root, &path, &doc, env.as_deref(), &session).await {
        Ok(start) => start,
        Err(e) => {
            eprintln!("✗ connexion impossible : {e}");
            return ExitCode::from(1);
        }
    };
    println!(
        "✓ connecté à {} ({}) en {:.0} ms",
        start.connection.opened.url,
        describe_opened(&start.connection.opened),
        start.connection.opened.timings.total_ms
    );
    if !start.unresolved.is_empty() {
        eprintln!("  ! variables non résolues : {}", start.unresolved.join(", "));
    }

    let mut outgoing: Vec<String> = Vec::new();
    if !args.no_send {
        outgoing.extend(start.selected().map(|m| m.data.clone()));
    }
    outgoing.extend(args.send.iter().cloned());
    for text in outgoing {
        println!("→ texte ({} o) : {}", text.len(), indent(&text));
        if let Err(e) = start.connection.sender.send(WsOutgoing::Text(text)) {
            eprintln!("✗ {e}");
            return ExitCode::from(1);
        }
    }

    let ends = max_time.map(|limit| tokio::time::Instant::now() + limit);
    let mut closing = false;
    let mut failed = false;
    loop {
        let wait = match ends {
            Some(end) => idle.min(end.saturating_duration_since(tokio::time::Instant::now())),
            None => idle,
        };
        match tokio::time::timeout(wait, start.connection.next()).await {
            Ok(Some(event)) => {
                show(&event);
                failed |= matches!(event, WsEvent::Error { .. });
                if matches!(event, WsEvent::Close { .. } | WsEvent::Error { .. }) {
                    break;
                }
            }
            Ok(None) => break,
            Err(_) if closing => break,
            Err(_) => {
                println!("→ fermeture (1000)");
                closing = true;
                if start.connection.sender.close().is_err() {
                    break;
                }
            }
        }
    }
    ExitCode::from(u8::from(failed))
}
