use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{ArgGroup, Parser, Subcommand};
use xc_core::collection::{COLLECTION_FILE, ENV_DIR, FOLDER_FILE, REQUEST_EXT};
use xc_core::request::BLANK_BEFORE;
use xc_core::restyle;
use xc_sync::import::{
    fetch_spec, import_bru as bru_collection, import_insomnia as insomnia_collection,
    import_postman as postman_collection, import_postman_environment, import_spec, ImportError,
};
use xc_sync::merge::{Choice, Kind};
use xc_sync::openapi::GroupBy;
use xc_sync::postman::Issue;
use xc_sync::sync::{self, Decisions, OpStatus, Operation, Plan, Report, SyncError};

mod run;

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
    /// Exécute des requêtes, un dossier ou toute la collection, et écrit les rapports JSON, JUnit et HTML
    Run(Box<run::RunArgs>),
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
    /// Importe une collection Postman (v2.0 ou v2.1) dans une nouvelle collection et affiche son chemin ; ce qui n'a pas
    /// pu être converti est listé sur la sortie d'erreur
    ImportPostman {
        /// Fichier JSON exporté par Postman
        file: PathBuf,
        /// Dossier parent où créer le dossier de la collection (il doit exister) ; avec --environment, la collection où
        /// ajouter l'environnement
        location: PathBuf,
        /// Le fichier est un environnement Postman, ajouté à la collection `location`
        #[arg(long)]
        environment: bool,
    },
    /// Importe un export Insomnia (v4 JSON ou v5 YAML) dans une nouvelle collection, environnements compris, et affiche
    /// son chemin ; ce qui n'a pas pu être converti est listé sur la sortie d'erreur
    ImportInsomnia {
        /// Fichier exporté par Insomnia (JSON ou YAML)
        file: PathBuf,
        /// Dossier parent où créer le dossier de la collection (il doit exister)
        location: PathBuf,
    },
    /// Convertit une collection Bruno au format `.bru` en OpenCollection YAML, dans une nouvelle collection (la source
    /// n'est pas modifiée) ; affiche son chemin, et liste sur la sortie d'erreur ce qui n'a pas pu être converti
    ImportBru {
        /// Dossier de la collection `.bru` (contient bruno.json)
        source: PathBuf,
        /// Dossier parent où créer le dossier de la collection (il doit exister)
        location: PathBuf,
    },
    /// Écrit le code d'une requête dans un langage (cURL, JavaScript, Python, Go, Java, Kotlin, Dart, PHP, C#), variables
    /// résolues ; les variables sans valeur sont signalées sur la sortie d'erreur
    Code {
        /// Dossier de la collection (contient opencollection.yml)
        collection: PathBuf,
        /// Chemin de la requête dans la collection (`Dossier/Requête.yml`)
        request: String,
        /// Langage : curl, javascript, python, go, java, kotlin, dart, php ou csharp
        #[arg(long, default_value = "curl")]
        lang: String,
        /// Environnement à appliquer
        #[arg(long)]
        env: Option<String>,
        /// Variable runtime, répétable : --env-var nom=valeur
        #[arg(long = "env-var", value_parser = run::parse_pair)]
        env_vars: Vec<(String, String)>,
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

fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Run(args) => {
            let runtime = tokio::runtime::Runtime::new().expect("runtime tokio");
            runtime.block_on(run::run(*args))
        }
        Command::Check { collection, diff } => check(&collection, diff),
        Command::Import { source, location, group_by } => {
            let runtime = tokio::runtime::Runtime::new().expect("runtime tokio");
            runtime.block_on(import(&source, &location, group_by))
        }
        Command::Code { collection, request, lang, env, env_vars } => {
            code(&collection, &request, &lang, env.as_deref(), env_vars)
        }
        Command::ImportPostman { file, location, environment } => import_postman(&file, &location, environment),
        Command::ImportInsomnia { file, location } => import_insomnia(&file, &location),
        Command::ImportBru { source, location } => {
            report_import(bru_collection(&source, &location).map(|(root, issues)| (root.display().to_string(), issues)))
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

fn import_postman(file: &Path, location: &Path, environment: bool) -> ExitCode {
    let text = match read_export(file) {
        Ok(text) => text,
        Err(code) => return code,
    };
    report_import(if environment {
        import_postman_environment(&text, location).map(|name| (name, Vec::new()))
    } else {
        postman_collection(&text, location).map(|(root, issues)| (root.display().to_string(), issues))
    })
}

fn import_insomnia(file: &Path, location: &Path) -> ExitCode {
    let text = match read_export(file) {
        Ok(text) => text,
        Err(code) => return code,
    };
    report_import(insomnia_collection(&text, location).map(|(root, issues)| (root.display().to_string(), issues)))
}

fn code(collection: &Path, request: &str, lang: &str, env: Option<&str>, env_vars: Vec<(String, String)>) -> ExitCode {
    use xc_codegen::Language;
    let Some(language) = Language::from_id(lang) else {
        let known: Vec<_> = Language::ALL.iter().map(|l| l.id()).collect();
        return run::input_error(format!("langage inconnu : {lang} ({} attendu)", known.join(", ")));
    };
    let doc = match xc_core::read_request(collection, request) {
        Ok(doc) => doc,
        Err(e) => return run::input_error(e),
    };
    let runtime = env_vars.into_iter().collect();
    match xc_core::prepare::snippet(collection, request, &doc, env, &runtime, xc_core::prepare::Overrides::default()) {
        Ok((snippet, unresolved)) => {
            for name in unresolved {
                eprintln!("avertissement : la variable {name} n'a pas de valeur");
            }
            print!("{}", xc_codegen::generate(&snippet, language));
            ExitCode::SUCCESS
        }
        Err(e) => run::input_error(e),
    }
}

fn read_export(file: &Path) -> Result<String, ExitCode> {
    fs::read_to_string(file).map_err(|e| {
        eprintln!("erreur : {} illisible : {e}", file.display());
        ExitCode::from(2)
    })
}

/// Affiche ce qui a été créé sur la sortie standard et ce qui n'a pas pu être converti sur la sortie d'erreur.
fn report_import(result: Result<(String, Vec<Issue>), ImportError>) -> ExitCode {
    match result {
        Ok((created, issues)) => {
            for issue in &issues {
                eprintln!(
                    "{} : {} ({})",
                    issue.path,
                    issue.message,
                    if issue.severity == "error" { "ignoré" } else { "avertissement" }
                );
            }
            println!("{created}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("erreur : {e}");
            ExitCode::from(if e.is_input() { 2 } else { 1 })
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
