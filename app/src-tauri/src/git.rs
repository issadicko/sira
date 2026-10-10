//! Git pour la collection ouverte : état de l'arbre de travail, diff, validation, pull et push. Tout est fait par
//! `xc-git` ; les erreurs arrivent à l'interface en français.

use std::path::Path;

use serde::{Deserialize, Serialize};
use xc_git::{FileDiff, Repo, Status};

use crate::{err, Reply};

/// L'état de la collection vue de Git : `repo` est faux quand le dossier n'est dans aucun dépôt.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitState {
    pub repo: bool,
    #[serde(flatten)]
    pub status: Status,
}

async fn repo_of(root: &str) -> Reply<Repo> {
    Repo::open(Path::new(root)).map_err(err)?.ok_or_else(|| xc_git::GitError::NotARepository.to_string())
}

#[tauri::command]
pub async fn git_status(root: String) -> Reply<GitState> {
    match Repo::open(Path::new(&root)).map_err(err)? {
        None => Ok(GitState { repo: false, status: Status::default() }),
        Some(repo) => Ok(GitState { repo: true, status: repo.status().await.map_err(err)? }),
    }
}

/// Crée un dépôt Git dans le dossier de la collection.
#[tauri::command]
pub async fn git_init(root: String) -> Reply<GitState> {
    let repo = Repo::init(Path::new(&root)).await.map_err(err)?;
    Ok(GitState { repo: true, status: repo.status().await.map_err(err)? })
}

#[tauri::command]
pub async fn git_diff(root: String, path: String) -> Reply<FileDiff> {
    repo_of(&root).await?.diff(&path).map_err(err)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitArgs {
    pub root: String,
    pub message: String,
    /// Les fichiers à valider (relatifs à la collection) ; vide : tout ce qui a changé.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Pousse aussitôt après la validation.
    #[serde(default)]
    pub push: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Committed {
    pub id: String,
    pub pushed: bool,
    /// Pourquoi le push a échoué alors que la validation a réussi.
    pub push_error: Option<String>,
}

#[tauri::command]
pub async fn git_commit(args: CommitArgs) -> Reply<Committed> {
    let repo = repo_of(&args.root).await?;
    let id = repo.commit(&args.message, &args.paths).await.map_err(err)?;
    if !args.push {
        return Ok(Committed { id, pushed: false, push_error: None });
    }
    Ok(match repo.push().await {
        Ok(_) => Committed { id, pushed: true, push_error: None },
        Err(error) => Committed { id, pushed: false, push_error: Some(error.to_string()) },
    })
}

#[tauri::command]
pub async fn git_pull(root: String) -> Reply<String> {
    repo_of(&root).await?.pull().await.map(|out| out.trim().to_owned()).map_err(err)
}

#[tauri::command]
pub async fn git_push(root: String) -> Reply<String> {
    repo_of(&root).await?.push().await.map(|out| out.trim().to_owned()).map_err(err)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git").arg("-C").arg(dir).args(args).output().unwrap();
        assert!(out.status.success(), "git {args:?} : {}", String::from_utf8_lossy(&out.stderr));
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        git(dir.path(), &["config", "user.name", "Ada"]);
        git(dir.path(), &["config", "user.email", "ada@example.test"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        fs::write(dir.path().join("a.yml"), "name: a\n").unwrap();
        git(dir.path(), &["add", "-A"]);
        git(dir.path(), &["commit", "-q", "-m", "départ"]);
        dir
    }

    #[tokio::test]
    async fn ef_git_01_the_commands_report_status_diff_and_commit_for_the_open_collection() {
        let dir = repo();
        let root = dir.path().display().to_string();
        fs::write(dir.path().join("a.yml"), "name: changé\n").unwrap();

        let state = git_status(root.clone()).await.unwrap();
        assert!(state.repo);
        assert_eq!(state.status.branch.as_deref(), Some("main"));
        assert_eq!(state.status.files.len(), 1);

        let diff = git_diff(root.clone(), "a.yml".into()).await.unwrap();
        assert_eq!((diff.head.as_deref(), diff.work.as_deref()), (Some("name: a\n"), Some("name: changé\n")));

        let done =
            git_commit(CommitArgs { root: root.clone(), message: "Mise à jour".into(), paths: vec![], push: false })
                .await
                .unwrap();
        assert!(!done.id.is_empty() && !done.pushed && done.push_error.is_none());
        assert!(git_status(root).await.unwrap().status.files.is_empty());
    }

    #[tokio::test]
    async fn ef_git_01_a_commit_that_cannot_be_pushed_still_succeeds_and_says_why() {
        let dir = repo();
        let root = dir.path().display().to_string();
        fs::write(dir.path().join("a.yml"), "name: deux\n").unwrap();

        let done = git_commit(CommitArgs { root, message: "deux".into(), paths: vec![], push: true }).await.unwrap();

        assert!(!done.id.is_empty());
        assert!(!done.pushed);
        assert!(done.push_error.is_some_and(|e| e.contains("dépôt distant")));
    }

    #[tokio::test]
    async fn ef_git_01_a_folder_without_repository_is_reported_and_can_be_initialised() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().display().to_string();

        let state = git_status(root.clone()).await.unwrap();
        assert!(!state.repo);
        assert!(git_diff(root.clone(), "a.yml".into()).await.unwrap_err().contains("pas dans un dépôt"));

        let state = git_init(root).await.unwrap();
        assert!(state.repo);
        assert!(state.status.unborn);
    }
}
