//! Récupération du corpus de Gate 1 et Gate 3 : `manifest.json` épingle chaque collection à un commit, que git
//! récupère dans `target/corpus` (ou `XC_CORPUS_DIR`) au premier lancement.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::Deserialize;

const FETCH_THREADS: usize = 6;

#[derive(Deserialize)]
pub struct Entry {
    pub id: String,
    pub repo: String,
    pub commit: String,
    pub path: String,
    pub restyled: usize,
}

pub fn corpus_dir() -> PathBuf {
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
pub fn fetch(entry: &Entry, dest: &Path) -> Result<(), String> {
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

/// Le manifeste du corpus, `manifest.json` du crate `xc-core`.
pub fn manifest() -> Result<Vec<Entry>, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/tests/corpus/manifest.json");
    serde_json::from_str(&fs::read_to_string(&path).map_err(|e| format!("{} : {e}", path.display()))?)
        .map_err(|e| format!("manifest.json : {e}"))
}

/// Dossier de la collection `entry` dans le corpus récupéré.
pub fn root_of(entry: &Entry) -> PathBuf {
    corpus_dir().join(&entry.id).join(&entry.path)
}

/// Récupère les collections absentes de `target/corpus`.
pub fn fetch_all(entries: &[Entry]) -> Result<(), String> {
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
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    }
}
