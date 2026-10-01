use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use xc_core::assert::{evaluate, ResponseView};
use xc_core::collection::{COLLECTION_FILE, FOLDER_FILE};
use xc_core::request::BLANK_BEFORE;
use xc_core::{normalize, open_collection, prepare, read_request, TreeItem};

#[derive(Parser)]
#[command(
    name = "xc",
    version,
    about = "Client API offline-first, compatible avec les collections Bruno (OpenCollection YAML)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Exécute une requête, un dossier ou toute la collection
    Run {
        /// Dossier de la collection (contient opencollection.yml)
        collection: PathBuf,
        /// Chemin relatif d'une requête ou d'un dossier ; toute la collection si absent
        target: Option<String>,
        /// Environnement à utiliser (nom du fichier dans environments/)
        #[arg(long)]
        env: Option<String>,
        /// Variable runtime, répétable : --env-var nom=valeur
        #[arg(long = "env-var", value_parser = parse_pair)]
        env_vars: Vec<(String, String)>,
        /// S'arrête au premier échec
        #[arg(long)]
        bail: bool,
    },
    /// Relit et réécrit chaque fichier en mémoire pour vérifier l'aller-retour sans diff
    Check { collection: PathBuf },
}

fn parse_pair(s: &str) -> Result<(String, String), String> {
    s.split_once('=').map(|(k, v)| (k.to_owned(), v.to_owned())).ok_or_else(|| format!("attendu nom=valeur, reçu {s}"))
}

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Run { collection, target, env, env_vars, bail } => {
            let runtime = tokio::runtime::Runtime::new().expect("runtime tokio");
            runtime.block_on(run(&collection, target.as_deref(), env.as_deref(), env_vars.into_iter().collect(), bail))
        }
        Command::Check { collection } => check(&collection),
    }
}

fn collect_requests(items: &[TreeItem], prefix: Option<&str>, out: &mut Vec<(String, String)>) {
    for item in items {
        match item {
            TreeItem::Folder { path, children, .. } => {
                let inside = prefix.is_none_or(|p| path == p || path.starts_with(&format!("{p}/")));
                collect_requests(children, if inside { None } else { prefix }, out);
            }
            TreeItem::Request { path, name, request_type, .. } => {
                if request_type == "http" && prefix.is_none_or(|p| path == p) {
                    out.push((path.clone(), name.clone()));
                }
            }
        }
    }
}

async fn run(
    root: &Path,
    target: Option<&str>,
    env: Option<&str>,
    runtime: HashMap<String, String>,
    bail: bool,
) -> ExitCode {
    let collection = match open_collection(root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("erreur : {e}");
            return ExitCode::from(2);
        }
    };
    let env = env.map(str::to_owned).or(collection.default_environment.clone());
    let mut requests = Vec::new();
    collect_requests(&collection.items, target, &mut requests);
    if requests.is_empty() {
        eprintln!("erreur : aucune requête HTTP pour {}", target.unwrap_or("la collection"));
        return ExitCode::from(2);
    }

    let (mut passed, mut failed) = (0, 0);
    for (path, name) in &requests {
        let ok = run_one(root, path, name, env.as_deref(), &runtime).await;
        if ok {
            passed += 1
        } else {
            failed += 1
        }
        if !ok && bail {
            break;
        }
    }
    println!("\n{passed} réussie(s), {failed} en échec, {} au total", requests.len());
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

async fn run_one(root: &Path, path: &str, name: &str, env: Option<&str>, runtime: &HashMap<String, String>) -> bool {
    let doc = match read_request(root, path) {
        Ok(d) => d,
        Err(e) => {
            println!("✗ {name}  {e}");
            return false;
        }
    };
    let prepared = match prepare(root, path, &doc, env, runtime) {
        Ok(p) => p,
        Err(e) => {
            println!("✗ {name}  {e}");
            return false;
        }
    };
    if !prepared.unresolved.is_empty() {
        println!("  ! variables non résolues : {}", prepared.unresolved.join(", "));
    }
    match xc_engine::send(&prepared.request).await {
        Err(e) => {
            println!("✗ {} {name}  {e}", prepared.request.method);
            false
        }
        Ok(res) => {
            let results =
                evaluate(&doc.assertions, &ResponseView { status: res.status, headers: &res.headers, body: &res.body });
            let ok = results.iter().all(|r| r.passed);
            println!(
                "{} {} {name}  {} {}  {:.0} ms",
                if ok { "✓" } else { "✗" },
                prepared.request.method,
                res.status,
                res.reason,
                res.timings.total_ms
            );
            for r in &results {
                let mark = if r.passed { "✓" } else { "✗" };
                let expected = r.expected.as_deref().unwrap_or("");
                let detail = r.error.clone().unwrap_or_else(|| {
                    if r.passed {
                        String::new()
                    } else {
                        format!("  (reçu {})", r.actual)
                    }
                });
                println!("    {mark} {} {} {expected}{detail}", r.expression, r.operator);
            }
            ok
        }
    }
}

fn check(root: &Path) -> ExitCode {
    let mut files = Vec::new();
    walk(root, &mut files);
    let (mut same, mut differ, mut broken) = (0, 0, 0);
    for file in &files {
        let rel = file.strip_prefix(root).unwrap_or(file).display();
        let Ok(text) = fs::read_to_string(file) else { continue };
        let is_env = file.parent().and_then(Path::file_name).is_some_and(|d| d == "environments");
        let blank: &[&str] = if is_env { &[] } else { BLANK_BEFORE };
        match normalize(&text, blank) {
            Ok(out) if out == text => same += 1,
            Ok(_) => {
                differ += 1;
                println!("≠ {rel}");
            }
            Err(e) => {
                broken += 1;
                println!("✗ {rel}  {e}");
            }
        }
    }
    println!("\n{same} identique(s), {differ} renormalisé(s), {broken} illisible(s) sur {} fichier(s)", files.len());
    if differ + broken == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            walk(&path, out);
        } else if name.ends_with(".yml") || name == COLLECTION_FILE || name == FOLDER_FILE {
            out.push(path);
        }
    }
}
