//! Gate 1 (ENF-COMP-02) : des collections Bruno publiques, relues puis réécrites sans rien perdre ni déplacer.
//!
//! Le corpus n'est pas dans le dépôt : `corpus/manifest.json` épingle chaque collection à un commit, que git récupère
//! dans `target/corpus` (ou `XC_CORPUS_DIR`) au premier lancement. Hors CI, le test s'ignore quand le réseau manque.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::Deserialize;
use xc_core::collection::{COLLECTION_FILE, ENV_DIR, FOLDER_FILE, REQUEST_EXT, REQUEST_KINDS};
use xc_core::request::BLANK_BEFORE;
use xc_core::yaml::{self, Value};
use xc_core::{
    open_collection, read_environment, read_request, restyle, save_environment, save_request, KeyValue, RequestDoc,
    TreeItem,
};

const FETCH_THREADS: usize = 6;
const MIN_COLLECTIONS: usize = 20;

#[derive(Deserialize)]
struct Entry {
    id: String,
    repo: String,
    commit: String,
    path: String,
    restyled: usize,
}

struct File {
    rel: String,
    text: String,
}

impl File {
    fn is_environment(&self) -> bool {
        self.rel.starts_with(&format!("{ENV_DIR}/"))
    }

    fn blank_before(&self) -> &'static [&'static str] {
        if self.is_environment() {
            &[]
        } else {
            BLANK_BEFORE
        }
    }

    fn request_type(&self) -> Option<String> {
        if self.is_environment() || self.rel == COLLECTION_FILE || self.rel.ends_with(FOLDER_FILE) {
            return None;
        }
        let Ok(Value::Map(tree)) = yaml::parse(&self.text) else { return None };
        let kind = tree.map("info")?.str("type")?;
        REQUEST_KINDS.contains(&kind).then(|| kind.to_owned())
    }
}

struct Collection {
    entry: Entry,
    root: PathBuf,
    files: Vec<File>,
}

fn corpus_dir() -> PathBuf {
    std::env::var_os("XC_CORPUS_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/corpus"))
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .output()
        .map_err(|e| format!("git : {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("git {} : {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Dossier `entry.path` du dépôt au commit épinglé, sans conversion de fin de ligne : les octets sont ceux du dépôt,
/// sur toutes les plates-formes.
fn fetch(entry: &Entry, dest: &Path) -> Result<(), String> {
    let part = dest.with_extension("part");
    fs::remove_dir_all(&part).ok();
    fs::create_dir_all(&part).map_err(|e| format!("{} : {e}", part.display()))?;
    let url = format!("https://github.com/{}.git", entry.repo);
    for args in [
        &["init", "-q"][..],
        &["remote", "add", "origin", &url],
        &["config", "core.sparseCheckout", "true"],
        &["config", "core.autocrlf", "false"],
        &["config", "core.eol", "lf"],
        &["config", "core.longpaths", "true"],
    ] {
        git(&part, args)?;
    }
    let pattern = if entry.path.is_empty() { "/*\n".to_owned() } else { format!("/{}/\n", entry.path) };
    fs::write(part.join(".git/info/sparse-checkout"), pattern).map_err(|e| e.to_string())?;
    git(&part, &["fetch", "-q", "--depth", "1", "--filter=blob:none", "origin", &entry.commit])?;
    git(&part, &["checkout", "-q", "FETCH_HEAD"])?;
    fs::rename(&part, dest).map_err(|e| format!("{} : {e}", dest.display()))
}

fn walk(dir: &Path, root: &Path, out: &mut Vec<File>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == "node_modules" {
            continue;
        }
        if path.is_dir() {
            walk(&path, root, out);
        } else if name.ends_with(REQUEST_EXT) {
            let Ok(text) = fs::read_to_string(&path) else { continue };
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            out.push(File { rel, text });
        }
    }
}

fn load() -> Result<Vec<Collection>, String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/manifest.json");
    let entries: Vec<Entry> = serde_json::from_str(&fs::read_to_string(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| format!("manifest.json : {e}"))?;
    let base = corpus_dir();
    fs::create_dir_all(&base).map_err(|e| format!("{} : {e}", base.display()))?;

    let next = AtomicUsize::new(0);
    let failures = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..FETCH_THREADS {
            scope.spawn(|| {
                while let Some(entry) = entries.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let dest = base.join(&entry.id);
                    if !dest.join(".git/HEAD").exists() {
                        if let Err(e) = fetch(entry, &dest) {
                            failures.lock().unwrap().push(format!("{} : {e}", entry.id));
                        }
                    }
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    if !failures.is_empty() {
        return Err(failures.join("\n"));
    }

    Ok(entries
        .into_iter()
        .map(|entry| {
            let root = base.join(&entry.id).join(&entry.path);
            let mut files = Vec::new();
            walk(&root, &root, &mut files);
            files.sort_by(|a, b| a.rel.cmp(&b.rel));
            Collection { entry, root, files }
        })
        .collect())
}

fn corpus() -> Option<&'static [Collection]> {
    static CORPUS: OnceLock<Option<Vec<Collection>>> = OnceLock::new();
    CORPUS
        .get_or_init(|| match load() {
            Ok(collections) => Some(collections),
            Err(e) if std::env::var_os("CI").is_some() => panic!("corpus indisponible :\n{e}"),
            Err(e) => {
                eprintln!("corpus ignoré (réseau absent ?) :\n{e}");
                None
            }
        })
        .as_deref()
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if source.is_dir() {
            copy_tree(&source, &target);
        } else {
            fs::copy(&source, &target).unwrap();
        }
    }
}

fn report(problems: Vec<String>) {
    assert!(problems.is_empty(), "{} écart(s) :\n{}", problems.len(), problems.join("\n"));
}

/// La première ligne où `after` s'écarte de `before`, avec deux lignes de contexte de chaque côté.
fn first_difference(before: &[&str], after: &[&str]) -> String {
    let at = before.iter().zip(after).position(|(x, y)| x != y).unwrap_or(before.len().min(after.len()));
    let window = |lines: &[&str], mark: char| {
        (at.saturating_sub(2)..(at + 3).min(lines.len()))
            .map(|i| format!("    {}{:>4} {:?}", if i == at { mark } else { ' ' }, i + 1, lines[i]))
            .collect::<Vec<_>>()
            .join("\n")
    };
    format!("{}\n{}", window(before, '-'), window(after, '+'))
}

fn count_requests(items: &[TreeItem]) -> usize {
    items
        .iter()
        .map(|item| match item {
            TreeItem::Folder { children, .. } => count_requests(children),
            TreeItem::Request { error, .. } => usize::from(error.is_none()),
        })
        .sum()
}

#[test]
fn enf_comp_02_corpus_has_enough_collections_and_they_all_open() {
    let Some(corpus) = corpus() else { return };
    assert!(corpus.len() >= MIN_COLLECTIONS, "{} collection(s), il en faut {MIN_COLLECTIONS}", corpus.len());
    let mut problems = Vec::new();
    for c in corpus {
        let requests = c.files.iter().filter(|f| f.request_type().is_some()).count();
        match open_collection(&c.root) {
            Ok(info) if info.request_count == count_requests(&info.items) && info.request_count == requests => {}
            Ok(info) => problems.push(format!(
                "{} : {} requête(s) comptée(s), {} dans l'arbre, {requests} fichier(s) de requête",
                c.entry.id,
                info.request_count,
                count_requests(&info.items)
            )),
            Err(e) => problems.push(format!("{} : {e}", c.entry.id)),
        }
    }
    report(problems);
}

#[test]
fn enf_comp_02_rewriting_keeps_the_meaning_of_every_file_and_is_stable() {
    let Some(corpus) = corpus() else { return };
    let mut problems = Vec::new();
    for c in corpus {
        for f in &c.files {
            let at = format!("{}/{}", c.entry.id, f.rel);
            let Ok(before) = yaml::parse(&f.text) else {
                problems.push(format!("{at} : illisible"));
                continue;
            };
            let Ok(once) = restyle(&f.text, f.blank_before()) else {
                problems.push(format!("{at} : réécriture impossible"));
                continue;
            };
            match yaml::parse(once.trim_start_matches('\u{feff}')) {
                Ok(after) if after == before => {}
                _ => problems.push(format!("{at} : le sens du fichier change à la réécriture")),
            }
            if restyle(&once, f.blank_before()).ok().as_deref() != Some(once.as_str()) {
                problems.push(format!("{at} : la réécriture n'est pas stable"));
            }
        }
    }
    report(problems);
}

#[test]
fn enf_comp_02_files_nobody_edited_are_never_rewritten() {
    let Some(corpus) = corpus() else { return };
    let mut problems = Vec::new();
    for c in corpus {
        let work = tempfile::tempdir().unwrap();
        copy_tree(&c.root, work.path());
        let root = work.path();
        for f in &c.files {
            let at = format!("{}/{}", c.entry.id, f.rel);
            let saved = if f.is_environment() {
                let name = f.rel.trim_start_matches("environments/").trim_end_matches(REQUEST_EXT);
                read_environment(root, name).and_then(|vars| save_environment(root, name, &vars, false))
            } else if f.request_type().is_some() {
                read_request(root, &f.rel).and_then(|doc| save_request(root, &f.rel, &doc))
            } else {
                continue;
            };
            match saved {
                Ok(false) => {}
                Ok(true) => problems.push(format!("{at} : réécrit sans modification")),
                Err(e) => problems.push(format!("{at} : {e}")),
            }
            if fs::read_to_string(root.join(&f.rel)).ok().as_deref() != Some(f.text.as_str()) {
                problems.push(format!("{at} : octets modifiés"));
            }
        }
    }
    report(problems);
}

/// Une modification, si elle s'applique à la requête, et ce qu'elle doit laisser du fichier.
struct Edit {
    name: &'static str,
    apply: fn(&mut RequestDoc) -> bool,
    outcome: fn(&[&str], &[&str]) -> bool,
}

fn one_line_changed(before: &[&str], after: &[&str]) -> bool {
    before.len() == after.len() && before.iter().zip(after).filter(|(x, y)| x != y).count() == 1
}

fn only_inserted(before: &[&str], after: &[&str]) -> bool {
    let mut rest = after.iter();
    before.iter().all(|line| rest.any(|l| l == line))
}

const EDITS: [Edit; 4] = [
    Edit {
        name: "nom",
        apply: |doc| {
            doc.name.push('x');
            true
        },
        outcome: one_line_changed,
    },
    Edit {
        name: "URL",
        apply: |doc| {
            doc.url.push('x');
            true
        },
        outcome: one_line_changed,
    },
    Edit {
        name: "méthode",
        apply: |doc| {
            let has_verb = matches!(doc.request_type.as_str(), "http" | "graphql");
            doc.method = if doc.method == "PATCH" { "PUT" } else { "PATCH" }.into();
            has_verb
        },
        outcome: one_line_changed,
    },
    Edit {
        name: "en-tête ajouté",
        apply: |doc| {
            doc.headers.push(KeyValue { name: "X-Corpus".into(), value: "1".into(), enabled: true, description: None });
            true
        },
        outcome: only_inserted,
    },
];

#[test]
fn enf_comp_02_editing_a_canonical_request_touches_only_what_changed() {
    let Some(corpus) = corpus() else { return };
    let (mut problems, mut checked) = (Vec::new(), 0);
    for c in corpus {
        let work = tempfile::tempdir().unwrap();
        copy_tree(&c.root, work.path());
        let root = work.path();
        for f in c.files.iter().filter(|f| f.request_type().is_some()) {
            if restyle(&f.text, BLANK_BEFORE).ok().as_deref() != Some(f.text.as_str()) {
                continue;
            }
            for edit in &EDITS {
                let at = format!("{}/{} : {}", c.entry.id, f.rel, edit.name);
                let mut doc = read_request(root, &f.rel).unwrap();
                if !(edit.apply)(&mut doc) {
                    continue;
                }
                match save_request(root, &f.rel, &doc) {
                    Ok(true) => {}
                    other => {
                        problems.push(format!("{at} : rien d'écrit ({other:?})"));
                        continue;
                    }
                }
                let after = fs::read_to_string(root.join(&f.rel)).unwrap();
                let (a, b): (Vec<_>, Vec<_>) = (f.text.split('\n').collect(), after.split('\n').collect());
                if !(edit.outcome)(&a, &b) {
                    problems.push(format!("{at} : le fichier change plus que prévu\n{}", first_difference(&a, &b)));
                }
                fs::write(root.join(&f.rel), &f.text).unwrap();
                checked += 1;
            }
        }
    }
    report(problems);
    assert!(checked > 0, "aucune requête canonique éditée");
}

#[test]
fn enf_comp_02_restyled_file_counts_match_the_manifest() {
    let Some(corpus) = corpus() else { return };
    let mut problems = Vec::new();
    for c in corpus {
        let restyled: Vec<&str> = c
            .files
            .iter()
            .filter(|f| restyle(&f.text, f.blank_before()).ok().as_deref() != Some(f.text.as_str()))
            .map(|f| f.rel.as_str())
            .collect();
        if restyled.len() != c.entry.restyled {
            problems.push(format!(
                "{} : {} fichier(s) restylé(s) au lieu de {} : {}",
                c.entry.id,
                restyled.len(),
                c.entry.restyled,
                restyled.iter().take(5).copied().collect::<Vec<_>>().join(", ")
            ));
        }
    }
    report(problems);
}
