//! `xc run` : exécute une collection, un dossier ou des requêtes, affiche le détail au fil de l'eau et écrit les
//! rapports JSON, JUnit et HTML demandés.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use clap::Args;
use xc_core::{open_collection, ClientCertificate, NetworkPrefs};
use xc_runner::{
    filter_items, html_page, json, junit, load_env_file, now_iso, read_rows, run_collection, select, Event, Filter,
    Halt, Job, Meta, PhaseReport, Redact, RequestResult, Row, RunReport, Session, Skip, MAX_JUMPS,
};

#[derive(Args)]
pub struct RunArgs {
    /// Dossier de la collection (contient opencollection.yml)
    collection: PathBuf,
    /// Chemins relatifs de requêtes ou de dossiers, exécutés l'un après l'autre ; toute la collection si absents
    targets: Vec<String>,
    /// Environnement à utiliser (nom du fichier dans environments/)
    #[arg(long)]
    env: Option<String>,
    /// Fichier d'environnement (.json ou .yml), absolu ou relatif à la collection, à la place de --env
    #[arg(long = "env-file", value_name = "FICHIER", conflicts_with = "env")]
    env_file: Option<PathBuf>,
    /// Variable runtime, répétable : --env-var nom=valeur
    #[arg(long = "env-var", value_parser = parse_pair)]
    env_vars: Vec<(String, String)>,
    /// Ne lance que les requêtes qui ont un test ou une assertion active
    #[arg(long)]
    tests_only: bool,
    /// Ne lance que les requêtes qui portent l'un de ces tags, séparés par des virgules
    #[arg(long, value_name = "TAGS")]
    tags: Option<String>,
    /// Écarte les requêtes qui portent l'un de ces tags, séparés par des virgules
    #[arg(long, value_name = "TAGS")]
    exclude_tags: Option<String>,
    /// S'arrête au premier échec d'une requête, d'un test ou d'une assertion ; le reste est ignoré
    #[arg(long)]
    bail: bool,
    /// Attente, en millisecondes, entre deux requêtes
    #[arg(long, value_name = "MS", default_value_t = 0)]
    delay: u64,
    /// Fichier CSV (en-têtes en première ligne) ou JSON (tableau d'objets) : une itération par ligne, dont les champs
    /// deviennent des variables runtime
    #[arg(long, value_name = "FICHIER")]
    data: Option<PathBuf>,
    /// Fichier de résultats, au format de --format
    #[arg(short, long, value_name = "FICHIER")]
    output: Option<PathBuf>,
    /// Format de --output : json, junit ou html
    #[arg(short, long, default_value = "json")]
    format: Format,
    /// Écrit les résultats JSON dans ce fichier
    #[arg(long, value_name = "FICHIER")]
    reporter_json: Option<PathBuf>,
    /// Écrit les résultats JUnit dans ce fichier
    #[arg(long, value_name = "FICHIER")]
    reporter_junit: Option<PathBuf>,
    /// Écrit le rapport HTML dans ce fichier
    #[arg(long, value_name = "FICHIER")]
    reporter_html: Option<PathBuf>,
    /// Retire tous les en-têtes des rapports
    #[arg(long)]
    reporter_skip_all_headers: bool,
    /// Retire ces en-têtes des rapports (sans tenir compte de la casse)
    #[arg(long, value_name = "NOM", num_args = 1..)]
    reporter_skip_headers: Vec<String>,
    /// Retire le corps des requêtes des rapports
    #[arg(long)]
    reporter_skip_request_body: bool,
    /// Retire le corps des réponses des rapports
    #[arg(long)]
    reporter_skip_response_body: bool,
    /// Retire les corps des requêtes et des réponses des rapports
    #[arg(long)]
    reporter_skip_body: bool,
    /// Ne vérifie ni la chaîne ni le nom d'hôte des certificats des serveurs
    #[arg(long)]
    insecure: bool,
    /// Fichier PEM d'autorités de certification à ajouter à celles du système
    #[arg(long, value_name = "FICHIER")]
    cacert: Option<PathBuf>,
    /// Avec --cacert : seules les autorités de ce fichier font confiance
    #[arg(long)]
    ignore_truststore: bool,
    /// N'utilise aucun proxy, ni celui de la collection ni celui de l'environnement
    #[arg(long)]
    noproxy: bool,
    /// N'envoie ni ne garde aucun cookie : le pot de cookies reste vide
    #[arg(long)]
    disable_cookies: bool,
    /// Fichier JSON {"enabled": true, "certs": [...]} de certificats client, après ceux de la collection
    #[arg(long, value_name = "FICHIER")]
    client_cert_config: Option<PathBuf>,
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum Format {
    Json,
    Junit,
    Html,
}

pub(crate) fn parse_pair(s: &str) -> Result<(String, String), String> {
    s.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())).ok_or_else(|| format!("attendu nom=valeur, reçu {s}"))
}

pub(crate) fn input_error(message: impl std::fmt::Display) -> ExitCode {
    eprintln!("erreur : {message}");
    ExitCode::from(2)
}

/// Les réglages réseau de la ligne de commande : pas de préférences, seulement les options et l'environnement.
fn network_prefs(args: &RunArgs) -> Result<NetworkPrefs, String> {
    let mut prefs = NetworkPrefs {
        verify_tls: !args.insecure,
        keep_default_roots: !args.ignore_truststore,
        no_proxy: args.noproxy,
        send_cookies: !args.disable_cookies,
        store_cookies: !args.disable_cookies,
        ..NetworkPrefs::default()
    };
    if let Some(cacert) = &args.cacert {
        if args.insecure {
            eprintln!("  ! --cacert est ignoré : --insecure désactive la vérification des certificats");
        } else if !cacert.is_file() {
            return Err(format!("le fichier --cacert {} n'existe pas", cacert.display()));
        }
        prefs.ca_file = Some(cacert.display().to_string());
    }
    if let Some(file) = &args.client_cert_config {
        let text = fs::read_to_string(file).map_err(|e| format!("lecture de {} impossible : {e}", file.display()))?;
        let config: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{} n'est pas du JSON valide : {e}", file.display()))?;
        match (config.get("enabled").and_then(serde_json::Value::as_bool), config.get("certs")) {
            (Some(true), Some(certs @ serde_json::Value::Array(_))) => {
                prefs.client_certificates = serde_json::from_value::<Vec<ClientCertificate>>(certs.clone())
                    .map_err(|e| format!("{} : certificat client invalide : {e}", file.display()))?;
            }
            _ => eprintln!(
                "  ! {} : \"enabled\" n'est pas vrai ou \"certs\" n'est pas une liste, aucun certificat client ajouté",
                file.display()
            ),
        }
    }
    Ok(prefs)
}

/// Les rapports demandés, chacun avec son fichier : `--output` selon `--format`, puis les `--reporter-*` qui le
/// remplacent pour leur format.
fn reporters(args: &RunArgs) -> Vec<(Format, PathBuf)> {
    let mut out: Vec<(Format, PathBuf)> = Vec::new();
    let mut set = |format: Format, path: &Option<PathBuf>| {
        if let Some(path) = path {
            out.retain(|(f, _)| *f != format);
            out.push((format, path.clone()));
        }
    };
    set(args.format, &args.output);
    set(Format::Html, &args.reporter_html);
    set(Format::Json, &args.reporter_json);
    set(Format::Junit, &args.reporter_junit);
    out
}

fn redact(args: &RunArgs) -> Redact {
    Redact {
        all_headers: args.reporter_skip_all_headers,
        headers: args.reporter_skip_headers.clone(),
        request_body: args.reporter_skip_request_body || args.reporter_skip_body,
        response_body: args.reporter_skip_response_body || args.reporter_skip_body,
    }
}

pub async fn run(args: RunArgs) -> ExitCode {
    let reporters = reporters(&args);
    for (_, path) in &reporters {
        let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        if !dir.is_dir() {
            return input_error(format!("le dossier {} n'existe pas", dir.display()));
        }
    }
    let rows: Vec<Row> = match args.data.as_deref().map(read_rows).transpose() {
        Ok(rows) => rows.unwrap_or_default(),
        Err(e) => return input_error(e),
    };
    if args.data.is_some() && rows.is_empty() {
        return input_error("le fichier de données ne contient aucune ligne");
    }
    let root = args.collection.as_path();
    let collection = match open_collection(root) {
        Ok(c) => c,
        Err(e) => return input_error(e),
    };
    if let Some(env) = &args.env {
        if !collection.environments.contains(env) {
            let known = if collection.environments.is_empty() {
                "aucun".to_owned()
            } else {
                collection.environments.join(", ")
            };
            return input_error(format!("environnement « {env} » introuvable (disponibles : {known})"));
        }
    }
    let items = match select(&collection.items, &args.targets) {
        Ok(items) => items,
        Err(e) => return input_error(e),
    };
    let filter = Filter {
        tests_only: args.tests_only,
        tags: Filter::split(args.tags.as_deref()),
        exclude_tags: Filter::split(args.exclude_tags.as_deref()),
    };
    let items = filter_items(root, items, &filter);
    if items.is_empty() {
        eprintln!("  ! aucune requête ne correspond à --tests-only, --tags ou --exclude-tags");
    }
    let (env, env_writes) = match &args.env_file {
        Some(file) => match load_env_file(&root.join(file)) {
            Ok(writes) => (None, Some(writes)),
            Err(e) => return input_error(e),
        },
        None => (args.env.clone().or(collection.default_environment.clone()), None),
    };

    let network = match network_prefs(&args) {
        Ok(network) => network,
        Err(e) => return input_error(e),
    };
    let mut session = Session { network, env: env_writes, ..Session::default() };
    let runtime: HashMap<String, String> = args.env_vars.iter().cloned().collect();
    session.runtime.extend(runtime.into_iter().map(|(k, v)| (k, serde_json::Value::String(v))));
    let job = Job {
        root,
        collection_name: &collection.name,
        env: env.as_deref(),
        items: &items,
        rows: &rows,
        bail: args.bail,
        delay: Duration::from_millis(args.delay),
        max_jumps: MAX_JUMPS,
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let report = run_collection(job, &mut session, &mut |event| show(&event)).await;
    summarize(&report);

    let redact = redact(&args);
    let meta = Meta { collection: collection.name.clone(), completed_at: now_iso() };
    for (format, path) in &reporters {
        let text = match format {
            Format::Json => json(&report, &redact),
            Format::Junit => junit(&report, &redact),
            Format::Html => html_page(&report, &redact, &meta),
        };
        if let Err(e) = fs::write(path, text) {
            return input_error(format!("écriture de {} impossible : {e}", path.display()));
        }
        println!("Résultats écrits dans {}", path.display());
    }
    if report.failed() {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn show(event: &Event<'_>) {
    match event {
        Event::Iteration { index, total, row } if *total > 1 => {
            let data = row.map(|r| format!("  {}", serde_json::Value::Object((*r).clone()))).unwrap_or_default();
            println!("\n── itération {}/{total}{data}", index + 1);
        }
        Event::Iteration { .. } | Event::Started { .. } => {}
        Event::Finished(result) => print_result(result),
        Event::Waiting(delay) => println!("  … attente de {} ms avant la requête suivante", delay.as_millis()),
        Event::Warning(warning) => eprintln!("  ! {warning}"),
    }
}

fn print_phase(label: &str, phase: &PhaseReport) {
    for line in &phase.logs {
        let args: Vec<String> = line.args.as_array().map_or_else(Vec::new, |a| {
            a.iter().map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned)).collect()
        });
        println!("    [{}] {}", line.level, args.join(" "));
    }
    for result in &phase.results {
        if result.status == "pass" {
            println!("    ✓ {}", result.description);
        } else {
            println!("    ✗ {}  {}", result.description, result.error.as_deref().unwrap_or(""));
        }
    }
    if let Some(error) = &phase.error {
        println!("    ✗ {label} : {error}");
    }
}

fn print_result(result: &RequestResult) {
    let outcome = &result.outcome;
    let (method, name) = (&outcome.method, &result.name);
    match &result.skip {
        Some(Skip::Script) => {
            print_phase("script pré-requête", &outcome.pre);
            println!("- {method} {name}  ignorée par un script");
            return;
        }
        Some(Skip::Prompts(names)) => {
            println!(
                "- {method} {name}  ignorée : variables à saisir ({}), que la ligne de commande ne sait pas demander",
                names.join(", ")
            );
            return;
        }
        Some(Skip::Bail | Skip::StopExecution) => return,
        Some(Skip::Unreadable(why)) => {
            println!("- {name}  ignorée, fichier illisible : {why}");
            return;
        }
        None => {}
    }
    if !outcome.unresolved.is_empty() {
        println!("  ! variables non résolues : {}", outcome.unresolved.join(", "));
    }
    match (&outcome.error, &outcome.response) {
        (Some(error), _) => {
            print_phase("script pré-requête", &outcome.pre);
            println!("✗ {method} {name}  {}", error.message);
        }
        (None, Some(res)) => {
            let mark = if outcome.passed() { "✓" } else { "✗" };
            println!("{mark} {method} {name}  {} {}  {:.0} ms", res.status, res.reason, res.timings.total_ms);
            print_phase("script pré-requête", &outcome.pre);
            print_phase("script post-réponse", &outcome.post);
            for r in &outcome.assertions {
                let mark = if r.passed { "✓" } else { "✗" };
                let expected = r.expected.as_deref().unwrap_or("");
                let detail = r.error.as_ref().map(|e| format!("  {e}")).unwrap_or_default();
                println!("    {mark} {} {} {expected}{detail}", r.expression, r.operator);
            }
            print_phase("tests", &outcome.tests);
        }
        (None, None) => {}
    }
}

fn summarize(report: &RunReport) {
    let s = report.summary();
    let skipped = if s.skipped_requests > 0 { format!(", {} ignorée(s)", s.skipped_requests) } else { String::new() };
    println!(
        "\n{} réussie(s), {} en échec{skipped}, {} au total",
        s.passed_requests,
        s.failed_requests + s.error_requests,
        s.total_requests
    );
    let tests = s.passed_tests + s.passed_pre_request_tests + s.passed_post_response_tests;
    let total = s.total_tests + s.total_pre_request_tests + s.total_post_response_tests;
    println!(
        "Tests : {tests}/{total} · Assertions : {}/{} · Durée : {:.2} s",
        s.passed_assertions,
        s.total_assertions,
        report.elapsed.as_secs_f64()
    );
    match &report.halt {
        Some(Halt::Bail { request, reason, remaining }) => {
            println!("Arrêt au premier échec : {reason} dans « {request} », {remaining} requête(s) ignorée(s).");
        }
        Some(Halt::StopExecution { request, remaining }) => {
            println!("Run arrêté par un script dans « {request} », {remaining} requête(s) ignorée(s).");
        }
        Some(Halt::Loop) => println!("Run arrêté : trop de sauts, probablement une boucle sans fin."),
        Some(Halt::Cancelled) => println!("Run annulé."),
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: RunArgs,
    }

    fn prefs(options: &[&str]) -> Result<NetworkPrefs, String> {
        let mut argv = vec!["xc", "collection"];
        argv.extend_from_slice(options);
        network_prefs(&Wrapper::try_parse_from(argv).unwrap().args)
    }

    #[test]
    fn ef_req_04_without_options_the_command_line_verifies_and_keeps_the_defaults() {
        assert_eq!(prefs(&[]).unwrap(), NetworkPrefs::default());
    }

    #[test]
    fn ef_req_04_insecure_and_noproxy_and_ignore_truststore_set_their_preference() {
        let p = prefs(&["--insecure", "--noproxy", "--ignore-truststore"]).unwrap();
        assert!(!p.verify_tls && p.no_proxy && !p.keep_default_roots);
    }

    #[test]
    fn ef_ux_02_disable_cookies_turns_off_sending_and_storing() {
        let off = prefs(&["--disable-cookies"]).unwrap();
        assert!(!off.send_cookies && !off.store_cookies);
        let on = prefs(&[]).unwrap();
        assert!(on.send_cookies && on.store_cookies);
    }

    #[test]
    fn ef_req_04_cacert_must_exist_unless_insecure_makes_it_moot() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("absent.pem").display().to_string();
        assert!(prefs(&["--cacert", &missing]).unwrap_err().contains("n'existe pas"));
        assert_eq!(prefs(&["--insecure", "--cacert", &missing]).unwrap().ca_file.as_deref(), Some(missing.as_str()));
        let present = dir.path().join("ca.pem");
        fs::write(&present, "PEM").unwrap();
        let present = present.display().to_string();
        assert_eq!(prefs(&["--cacert", &present]).unwrap().ca_file.as_deref(), Some(present.as_str()));
    }

    #[test]
    fn ef_req_04_client_cert_config_is_added_only_when_enabled_with_a_list() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| {
            let path = dir.path().join(name);
            fs::write(&path, text).unwrap();
            path.display().to_string()
        };
        let enabled = write(
            "on.json",
            r#"{"enabled": true, "certs": [{"domain": "api.test", "type": "cert", "certFilePath": "c.pem", "keyFilePath": "k.pem"}]}"#,
        );
        let certs = prefs(&["--client-cert-config", &enabled]).unwrap().client_certificates;
        assert_eq!((certs.len(), certs[0].domain.as_str(), certs[0].key_file_path.as_str()), (1, "api.test", "k.pem"));

        let off = write("off.json", r#"{"enabled": false, "certs": [{"domain": "api.test"}]}"#);
        assert!(prefs(&["--client-cert-config", &off]).unwrap().client_certificates.is_empty());
        let not_a_list = write("list.json", r#"{"enabled": true, "certs": "x"}"#);
        assert!(prefs(&["--client-cert-config", &not_a_list]).unwrap().client_certificates.is_empty());
        let broken = write("broken.json", "{");
        assert!(prefs(&["--client-cert-config", &broken]).unwrap_err().contains("JSON"));
    }
}
