use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{ArgGroup, Parser, Subcommand};
use xc_core::assert::{evaluate, ResponseView};
use xc_core::collection::{COLLECTION_FILE, ENV_DIR, FOLDER_FILE, REQUEST_EXT};
use xc_core::request::BLANK_BEFORE;
use xc_core::{open_collection, prepare, read_request, restyle, TreeItem};
use xc_sync::import::{fetch_spec, import_spec};
use xc_sync::merge::{Choice, Kind};
use xc_sync::openapi::GroupBy;
use xc_sync::sync::{self, Decisions, OpStatus, Operation, Plan, Report, SyncError};

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
    Check {
        collection: PathBuf,
        /// Montre la première ligne qui diffère de chaque fichier renormalisé
        #[arg(long)]
        diff: bool,
    },
    /// Importe une spec OpenAPI 3.x ou Swagger 2.0 dans une nouvelle collection et affiche son chemin
    Import {
        /// Chemin d'un fichier ou URL http(s) de la spec
        source: String,
        /// Dossier parent où créer le dossier de la collection (il doit exister)
        location: PathBuf,
        /// Regroupement des requêtes : tags ou path
        #[arg(long, default_value = "tags")]
        group_by: GroupBy,
    },
    /// Compare la collection à la spec OpenAPI (fusion à 3 voies) : `--check` pour la CI, `--apply` pour écrire
    #[command(group(ArgGroup::new("mode").required(true).args(["check", "apply"])))]
    Sync {
        /// Dossier de la collection (contient opencollection.yml)
        collection: PathBuf,
        /// Sort avec le code 1 si la spec a divergé de la base ; n'écrit rien
        #[arg(long)]
        check: bool,
        /// Applique la synchro ; refuse s'il reste un conflit sans --keep-team ni --take-spec, ou une opération dont le fichier a disparu sans --forget-missing ni --recreate-missing
        #[arg(long)]
        apply: bool,
        /// Fichier ou URL de la spec ; par défaut, la source enregistrée dans .oc-sync
        #[arg(long)]
        source: Option<String>,
        /// Avec --apply : garde la version de l'équipe pour tous les conflits
        #[arg(long, conflicts_with = "take_spec")]
        keep_team: bool,
        /// Avec --apply : prend la version de la spec pour tous les conflits
        #[arg(long)]
        take_spec: bool,
        /// Avec --apply : oublie les opérations dont le fichier a disparu (elles ne sont plus suivies)
        #[arg(long, conflicts_with = "recreate_missing")]
        forget_missing: bool,
        /// Avec --apply : recrée le fichier des opérations dont il a disparu
        #[arg(long)]
        recreate_missing: bool,
    },
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
        Command::Check { collection, diff } => check(&collection, diff),
        Command::Import { source, location, group_by } => {
            let runtime = tokio::runtime::Runtime::new().expect("runtime tokio");
            runtime.block_on(import(&source, &location, group_by))
        }
        Command::Sync { collection, apply, source, keep_team, take_spec, forget_missing, recreate_missing, .. } => {
            if !apply && (keep_team || take_spec || forget_missing || recreate_missing) {
                eprintln!(
                    "erreur : --keep-team, --take-spec, --forget-missing et --recreate-missing s'utilisent avec --apply"
                );
                return ExitCode::from(2);
            }
            let choices = Arbitration {
                conflicts: if keep_team { Some(Choice::Team) } else { take_spec.then_some(Choice::Spec) },
                missing: if recreate_missing {
                    Some(Missing::Recreate)
                } else {
                    forget_missing.then_some(Missing::Forget)
                },
            };
            let runtime = tokio::runtime::Runtime::new().expect("runtime tokio");
            runtime.block_on(sync(&collection, apply, source.as_deref(), choices))
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Missing {
    Forget,
    Recreate,
}

/// Les options d'arbitrage de `--apply` : le choix pour tous les conflits, et le sort des opérations manquantes.
struct Arbitration {
    conflicts: Option<Choice>,
    missing: Option<Missing>,
}

async fn sync(root: &Path, apply: bool, source: Option<&str>, choices: Arbitration) -> ExitCode {
    match run_sync(root, apply, source, choices).await {
        Ok(code) => code,
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::from(if e.is_input() { 2 } else { 1 })
        }
    }
}

async fn run_sync(root: &Path, apply: bool, source: Option<&str>, choices: Arbitration) -> Result<ExitCode, SyncError> {
    let (text, recorded) = sync::read_source(root, source).await?;
    let plan = sync::plan(root, &text, recorded, &[])?;
    println!("{}", describe(&plan));
    if !apply {
        return Ok(if plan.diverged() { ExitCode::FAILURE } else { ExitCode::SUCCESS });
    }
    let missing: Vec<&Operation> = plan.operations.iter().filter(|op| op.status == OpStatus::Missing).collect();
    let undecided_conflicts = choices.conflicts.is_none() && plan.summary.conflicts > 0;
    let undecided_missing = choices.missing.is_none() && !missing.is_empty();
    if undecided_conflicts {
        eprintln!(
            "refus : {} conflit(s) à arbitrer, utiliser --keep-team ou --take-spec",
            plan.summary.conflict_fields
        );
        for change in conflicts(&plan) {
            eprintln!("  {}  {}", change.id, change.label);
        }
    }
    if undecided_missing {
        eprintln!(
            "refus : {} opération(s) dont le fichier a disparu, utiliser --forget-missing ou --recreate-missing",
            missing.len()
        );
        for op in &missing {
            eprintln!("  {}  {}", op.key, op.file.as_deref().unwrap_or_default());
        }
    }
    if undecided_conflicts || undecided_missing {
        return Ok(ExitCode::FAILURE);
    }
    let mut decisions = choices.conflicts.map(|choice| Decisions::uniform(&plan, choice)).unwrap_or_default();
    if choices.missing == Some(Missing::Recreate) {
        decisions.recreate = missing.iter().map(|op| op.key.clone()).collect();
    }
    println!("{}", applied(&plan.apply(&decisions)?));
    Ok(ExitCode::SUCCESS)
}

fn conflicts(plan: &Plan) -> impl Iterator<Item = &xc_sync::merge::Change> {
    plan.operations.iter().flat_map(|op| &op.changes).filter(|change| change.kind == Kind::Conflict)
}

fn status_label(status: OpStatus) -> &'static str {
    match status {
        OpStatus::Unchanged => "inchangée",
        OpStatus::Updated => "mise à jour",
        OpStatus::Kept => "conservée",
        OpStatus::Merged => "fusionnée",
        OpStatus::Conflict => "en conflit",
        OpStatus::New => "nouvelle",
        OpStatus::Removed => "retirée",
        OpStatus::Restored => "restaurée",
        OpStatus::Missing => "manquante",
    }
}

fn describe(plan: &Plan) -> String {
    let (title, version) = (&plan.to.title, &plan.to.version);
    let mut out = match &plan.from {
        Some(from) => format!("Spec : {title} {} → {version}\n", from.version),
        None => format!("Spec : {title} {version} (collection non connectée, aucune base)\n"),
    };
    let s = &plan.summary;
    out.push_str(&format!(
        "{} inchangée(s), {} mise(s) à jour, {} conservée(s), {} fusionnée(s), {} en conflit ({} champ(s)), \
         {} nouvelle(s), {} retirée(s), {} restaurée(s), {} manquante(s)",
        s.unchanged,
        s.updated,
        s.kept,
        s.merged,
        s.conflicts,
        s.conflict_fields,
        s.created,
        s.removed,
        s.restored,
        s.missing
    ));
    for op in plan.operations.iter().filter(|op| !matches!(op.status, OpStatus::Unchanged | OpStatus::Kept)) {
        let file = op.file.as_deref().map(|f| format!(" ({f})")).unwrap_or_default();
        out.push_str(&format!("\n  {:<11} {} {}{file}", status_label(op.status), op.method, op.path));
        for change in op.changes.iter().filter(|c| c.kind == Kind::Conflict) {
            out.push_str(&format!("\n      conflit : {} — {}", change.label, change.reason));
        }
    }
    out
}

fn applied(report: &Report) -> String {
    let mut out = format!(
        "{} fichier(s) modifié(s), {} créé(s), {} marqué(s) retiré(s) de la spec, {} ignorée(s)",
        report.written.len(),
        report.created.len(),
        report.removed.len(),
        report.ignored.len()
    );
    for (label, files) in [("modifié", &report.written), ("créé", &report.created), ("retiré", &report.removed)] {
        for file in files {
            out.push_str(&format!("\n  {label:<8} {file}"));
        }
    }
    out
}

async fn import(source: &str, location: &Path, group_by: GroupBy) -> ExitCode {
    let result = match fetch_spec(source).await {
        Ok(text) => import_spec(&text, source, location, group_by),
        Err(e) => Err(e),
    };
    match result {
        Ok(root) => {
            println!("{}", root.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::from(if e.is_input() { 2 } else { 1 })
        }
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
    let method = prepared.request.method.clone();
    match xc_engine::send(prepared.request).await {
        Err(e) => {
            println!("✗ {method} {name}  {e}");
            false
        }
        Ok(res) => {
            let results =
                evaluate(&doc.assertions, &ResponseView { status: res.status, headers: &res.headers, body: &res.body });
            let ok = results.iter().all(|r| r.passed);
            println!(
                "{} {} {name}  {} {}  {:.0} ms",
                if ok { "✓" } else { "✗" },
                method,
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

fn check(root: &Path, diff: bool) -> ExitCode {
    let mut files = Vec::new();
    walk(root, &mut files);
    let (mut same, mut differ, mut broken) = (0, 0, 0);
    for file in &files {
        let rel = file.strip_prefix(root).unwrap_or(file).display();
        let Ok(text) = fs::read_to_string(file) else { continue };
        let is_env = file.parent().and_then(Path::file_name).is_some_and(|d| d == ENV_DIR);
        let blank: &[&str] = if is_env { &[] } else { BLANK_BEFORE };
        match restyle(&text, blank) {
            Ok(out) if out == text => same += 1,
            Ok(out) => {
                differ += 1;
                println!("≠ {rel}");
                if diff {
                    println!("{}", first_difference(&text, &out));
                }
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

/// La première ligne où `after` s'écarte de `before`, avec deux lignes avant et après de chaque côté.
fn first_difference(before: &str, after: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (before.split('\n').collect(), after.split('\n').collect());
    let at = a.iter().zip(&b).position(|(x, y)| x != y).unwrap_or(a.len().min(b.len()));
    let window = |lines: &[&str], mark: char| {
        let from = at.saturating_sub(2);
        (from..(at + 3).min(lines.len()))
            .map(|i| format!("    {}{:>4} {:?}", if i == at { mark } else { ' ' }, i + 1, lines[i]))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!("{}\n{}", window(&a, '-'), window(&b, '+'))
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
        } else if name.ends_with(REQUEST_EXT) || name == COLLECTION_FILE || name == FOLDER_FILE {
            out.push(path);
        }
    }
}
