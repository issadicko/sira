//! Git pour une collection : état de l'arbre de travail, diff d'un fichier, commit, pull et push. La lecture du dépôt
//! (branche courante, contenu d'un fichier au dernier commit) passe par `gix` ; tout ce qui écrit dans l'index, l'historique
//! ou le réseau passe par l'exécutable `git` du système, qui connaît déjà les identifiants, les clés SSH et la configuration
//! de l'utilisateur (`gix` ne sait pas pousser).

mod run;
mod status;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

pub use status::{parse_status, FileChange, Status};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("ce dossier n'est pas dans un dépôt Git")]
    NotARepository,
    #[error("l'exécutable git est introuvable : installe Git pour utiliser le contrôle de version")]
    GitMissing,
    #[error("{0}")]
    Failed(String),
    #[error("{0}")]
    Io(String),
}

pub type Result<T> = std::result::Result<T, GitError>;

/// Délai d'une opération réseau (pull, push) avant abandon.
const NETWORK_TIMEOUT: Duration = Duration::from_secs(120);
const LOCAL_TIMEOUT: Duration = Duration::from_secs(30);

/// Au-delà, un fichier n'est pas comparé ligne à ligne.
pub const MAX_DIFF_BYTES: usize = 2 * 1024 * 1024;

/// Un fichier côté dépôt et côté disque, pour l'afficher en deux colonnes.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileDiff {
    pub path: String,
    /// Le contenu au dernier commit ; `None` pour un fichier nouveau.
    pub head: Option<String>,
    /// Le contenu sur le disque ; `None` pour un fichier supprimé.
    pub work: Option<String>,
    /// Binaire ou trop gros : aucun contenu n'est renvoyé.
    pub unreadable: Option<String>,
}

/// Le dépôt qui contient une collection, vue depuis le dossier de la collection.
#[derive(Debug, Clone)]
pub struct Repo {
    /// Le dossier de la collection, d'où partent toutes les commandes.
    dir: PathBuf,
    /// Le dossier de travail du dépôt.
    workdir: PathBuf,
    /// Le chemin de la collection depuis la racine du dépôt, avec `/` final (vide quand c'est la racine).
    prefix: String,
}

fn io(error: impl std::fmt::Display) -> GitError {
    GitError::Io(error.to_string())
}

impl Repo {
    /// Le dépôt qui contient `dir`, ou `None` quand `dir` n'est dans aucun dépôt.
    pub fn open(dir: &Path) -> Result<Option<Self>> {
        let Ok(repository) = gix::discover(dir) else { return Ok(None) };
        let Some(workdir) = repository.workdir().map(Path::to_path_buf) else { return Ok(None) };
        let dir = plain(dir.canonicalize().map_err(io)?);
        let workdir = plain(workdir.canonicalize().map_err(io)?);
        let relative = dir.strip_prefix(&workdir).map_err(io)?;
        let mut prefix = relative.to_string_lossy().replace('\\', "/");
        if !prefix.is_empty() {
            prefix.push('/');
        }
        Ok(Some(Self { dir, workdir, prefix }))
    }

    /// Initialise un dépôt dans `dir` (`git init`) ; sans effet sur un dossier qui en contient déjà un.
    pub async fn init(dir: &Path) -> Result<Self> {
        if let Some(repo) = Self::open(dir)? {
            return Ok(repo);
        }
        run::git(dir, &["init"], LOCAL_TIMEOUT).await?;
        Self::open(dir)?.ok_or(GitError::NotARepository)
    }

    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// L'état de l'arbre de travail pour les fichiers de la collection.
    pub async fn status(&self) -> Result<Status> {
        let out = run::git(
            &self.dir,
            &["status", "--porcelain=v2", "-z", "--branch", "--untracked-files=all", "--", "."],
            LOCAL_TIMEOUT,
        )
        .await?;
        Ok(parse_status(&out, &self.prefix))
    }

    /// Les deux versions de `path` (relatif à la collection) : au dernier commit et sur le disque.
    pub fn diff(&self, path: &str) -> Result<FileDiff> {
        let full = format!("{}{}", self.prefix, path);
        let head = self.head_blob(&full)?;
        let work = match std::fs::read(self.workdir.join(&full)) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(io(error)),
        };
        let reason = [head.as_deref(), work.as_deref()].into_iter().flatten().find_map(unreadable);
        let text = |bytes: Option<Vec<u8>>| bytes.map(|b| String::from_utf8_lossy(&b).into_owned());
        Ok(match reason {
            Some(reason) => FileDiff { path: path.to_owned(), head: None, work: None, unreadable: Some(reason) },
            None => FileDiff { path: path.to_owned(), head: text(head), work: text(work), unreadable: None },
        })
    }

    fn head_blob(&self, full: &str) -> Result<Option<Vec<u8>>> {
        let repository = gix::open(&self.workdir).map_err(io)?;
        let Ok(commit) = repository.head_commit() else { return Ok(None) };
        let tree = commit.tree().map_err(io)?;
        let Some(entry) = tree.lookup_entry_by_path(full).map_err(io)? else { return Ok(None) };
        let object = entry.object().map_err(io)?;
        Ok(Some(object.data.clone()))
    }

    /// Ajoute puis valide `paths` (relatifs à la collection ; tout ce qui a changé quand la liste est vide). Rend le
    /// début de l'identifiant du commit.
    pub async fn commit(&self, message: &str, paths: &[String]) -> Result<String> {
        if message.trim().is_empty() {
            return Err(GitError::Failed("écris un message avant de valider".into()));
        }
        let scope: Vec<String> = if paths.is_empty() { vec![".".to_owned()] } else { paths.to_vec() };
        let mut add = vec!["add", "-A", "--"];
        add.extend(scope.iter().map(String::as_str));
        run::git(&self.dir, &add, LOCAL_TIMEOUT).await?;
        let mut commit = vec!["commit", "-m", message, "--"];
        commit.extend(scope.iter().map(String::as_str));
        run::git(&self.dir, &commit, LOCAL_TIMEOUT).await?;
        let id = run::git(&self.dir, &["rev-parse", "--short", "HEAD"], LOCAL_TIMEOUT).await?;
        Ok(id.trim().to_owned())
    }

    /// Récupère et avance la branche quand c'est un simple avancement (`--ff-only`) : une branche qui a divergé n'est
    /// jamais fusionnée en silence.
    pub async fn pull(&self) -> Result<String> {
        run::git(&self.dir, &["pull", "--ff-only"], NETWORK_TIMEOUT).await
    }

    /// Pousse la branche courante ; sans branche amont, la crée sur `origin`.
    pub async fn push(&self) -> Result<String> {
        let status = self.status().await?;
        if status.upstream.is_some() {
            return run::git(&self.dir, &["push"], NETWORK_TIMEOUT).await;
        }
        let branch =
            status.branch.ok_or_else(|| GitError::Failed("aucune branche courante : impossible de pousser".into()))?;
        run::git(&self.dir, &["push", "--set-upstream", "origin", &branch], NETWORK_TIMEOUT).await
    }
}

/// Sous Windows, `canonicalize` rend un chemin `\\?\C:\…` que `git -C` ne comprend pas toujours : on le ramène à `C:\…`.
fn plain(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    match text.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => path,
    }
}

fn unreadable(bytes: &[u8]) -> Option<String> {
    if bytes.len() > MAX_DIFF_BYTES {
        return Some(format!("fichier trop gros pour être comparé ({} Mo)", bytes.len() / (1024 * 1024)));
    }
    bytes.contains(&0).then(|| "fichier binaire".to_owned())
}
