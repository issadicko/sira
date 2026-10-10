use std::fs;
use std::path::Path;
use std::process::Command;

use xc_git::{GitError, Repo};

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git").arg("-C").arg(dir).args(args).env("LC_ALL", "C").output().unwrap();
    assert!(out.status.success(), "git {args:?} : {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn identity(dir: &Path) {
    git(dir, &["config", "user.name", "Ada"]);
    git(dir, &["config", "user.email", "ada@example.test"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn repo_with_commit() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "-b", "main"]);
    identity(dir.path());
    write(dir.path(), "a.yml", "name: a\n");
    write(dir.path(), "dossier/b.yml", "name: b\n");
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "départ"]);
    dir
}

fn names(status: &xc_git::Status) -> Vec<(String, char, bool)> {
    status.files.iter().map(|f| (f.path.clone(), f.state, f.untracked)).collect()
}

#[tokio::test]
async fn ef_git_01_a_folder_outside_any_repository_has_no_repo_and_init_creates_one() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Repo::open(dir.path()).unwrap().is_none());

    let repo = Repo::init(dir.path()).await.unwrap();

    let status = repo.status().await.unwrap();
    assert!(status.unborn);
    assert!(status.files.is_empty());
    assert!(Repo::open(dir.path()).unwrap().is_some());
    assert!(Repo::init(dir.path()).await.is_ok());
}

#[tokio::test]
async fn ef_git_01_the_status_lists_modified_new_and_deleted_files_with_the_branch() {
    let dir = repo_with_commit();
    write(dir.path(), "a.yml", "name: changé\n");
    write(dir.path(), "nouveau.yml", "name: n\n");
    fs::remove_file(dir.path().join("dossier/b.yml")).unwrap();
    let repo = Repo::open(dir.path()).unwrap().unwrap();

    let status = repo.status().await.unwrap();

    assert_eq!(status.branch.as_deref(), Some("main"));
    assert!(!status.unborn);
    assert_eq!(
        names(&status),
        [
            ("a.yml".to_owned(), 'M', false),
            ("dossier/b.yml".to_owned(), 'D', false),
            ("nouveau.yml".to_owned(), 'A', true)
        ]
    );
    assert_eq!((status.ahead, status.behind, status.upstream), (0, 0, None));
}

#[tokio::test]
async fn ef_git_01_a_staged_new_file_a_rename_and_a_conflict_are_told_apart() {
    let dir = repo_with_commit();
    write(dir.path(), "ajout.yml", "name: x\n");
    git(dir.path(), &["add", "ajout.yml"]);
    git(dir.path(), &["mv", "a.yml", "renomme.yml"]);
    let repo = Repo::open(dir.path()).unwrap().unwrap();

    let status = repo.status().await.unwrap();

    let ajout = status.files.iter().find(|f| f.path == "ajout.yml").unwrap();
    assert_eq!((ajout.state, ajout.staged, ajout.untracked), ('A', true, false));
    let renamed = status.files.iter().find(|f| f.path == "renomme.yml").unwrap();
    assert_eq!((renamed.state, renamed.from.as_deref()), ('R', Some("a.yml")));

    git(dir.path(), &["commit", "-q", "-m", "x"]);
    git(dir.path(), &["checkout", "-q", "-b", "autre"]);
    write(dir.path(), "dossier/b.yml", "name: autre\n");
    git(dir.path(), &["commit", "-q", "-am", "autre"]);
    git(dir.path(), &["checkout", "-q", "main"]);
    write(dir.path(), "dossier/b.yml", "name: main\n");
    git(dir.path(), &["commit", "-q", "-am", "main"]);
    let merge = Command::new("git").arg("-C").arg(dir.path()).args(["merge", "autre"]).output().unwrap();
    assert!(!merge.status.success());
    let conflicted = repo.status().await.unwrap();
    let file = conflicted.files.iter().find(|f| f.path == "dossier/b.yml").unwrap();
    assert_eq!(file.state, 'U');
}

#[tokio::test]
async fn ef_git_01_a_collection_inside_a_repo_sees_only_its_own_files_with_relative_paths() {
    let dir = repo_with_commit();
    write(dir.path(), "api/req.yml", "name: r\n");
    write(dir.path(), "autre/x.yml", "name: x\n");
    git(dir.path(), &["add", "-A"]);
    git(dir.path(), &["commit", "-q", "-m", "api"]);
    write(dir.path(), "api/req.yml", "name: modifié\n");
    write(dir.path(), "autre/x.yml", "name: modifié aussi\n");
    let repo = Repo::open(&dir.path().join("api")).unwrap().unwrap();

    let status = repo.status().await.unwrap();

    assert_eq!(names(&status), [("req.yml".to_owned(), 'M', false)]);
    let diff = repo.diff("req.yml").unwrap();
    assert_eq!((diff.head.as_deref(), diff.work.as_deref()), (Some("name: r\n"), Some("name: modifié\n")));
}

#[tokio::test]
async fn ef_git_01_the_diff_gives_both_sides_and_marks_new_deleted_and_binary_files() {
    let dir = repo_with_commit();
    write(dir.path(), "a.yml", "name: changé\n");
    write(dir.path(), "nouveau.yml", "name: n\n");
    fs::remove_file(dir.path().join("dossier/b.yml")).unwrap();
    fs::write(dir.path().join("image.bin"), [0u8, 1, 2, 0]).unwrap();
    let repo = Repo::open(dir.path()).unwrap().unwrap();

    let modified = repo.diff("a.yml").unwrap();
    assert_eq!((modified.head.as_deref(), modified.work.as_deref()), (Some("name: a\n"), Some("name: changé\n")));
    let added = repo.diff("nouveau.yml").unwrap();
    assert_eq!((added.head, added.work.as_deref()), (None, Some("name: n\n")));
    let deleted = repo.diff("dossier/b.yml").unwrap();
    assert_eq!((deleted.head.as_deref(), deleted.work), (Some("name: b\n"), None));
    let binary = repo.diff("image.bin").unwrap();
    assert_eq!(binary.unreadable.as_deref(), Some("fichier binaire"));
    assert!(binary.head.is_none() && binary.work.is_none());
}

#[tokio::test]
async fn ef_git_01_a_commit_takes_every_change_of_the_collection_and_the_tree_is_clean_after() {
    let dir = repo_with_commit();
    write(dir.path(), "a.yml", "name: changé\n");
    write(dir.path(), "nouveau.yml", "name: n\n");
    fs::remove_file(dir.path().join("dossier/b.yml")).unwrap();
    let repo = Repo::open(dir.path()).unwrap().unwrap();

    let id = repo.commit("Mise à jour", &[]).await.unwrap();

    assert!(!id.is_empty());
    assert!(repo.status().await.unwrap().files.is_empty());
    assert_eq!(git(dir.path(), &["log", "-1", "--format=%s"]).trim(), "Mise à jour");
    assert_eq!(git(dir.path(), &["show", "--stat", "--format=", "HEAD"]).lines().count(), 4);
}

#[tokio::test]
async fn ef_git_01_a_commit_with_paths_takes_only_those_files() {
    let dir = repo_with_commit();
    write(dir.path(), "a.yml", "name: changé\n");
    write(dir.path(), "nouveau.yml", "name: n\n");
    let repo = Repo::open(dir.path()).unwrap().unwrap();

    repo.commit("Un seul", &["a.yml".to_owned()]).await.unwrap();

    let status = repo.status().await.unwrap();
    assert_eq!(names(&status), [("nouveau.yml".to_owned(), 'A', true)]);
}

#[tokio::test]
async fn ef_git_01_an_empty_message_and_a_clean_tree_are_refused_with_a_reason() {
    let dir = repo_with_commit();
    let repo = Repo::open(dir.path()).unwrap().unwrap();
    let error = repo.commit("  ", &[]).await.unwrap_err();
    assert_eq!(error.to_string(), "écris un message avant de valider");

    let error = repo.commit("rien", &[]).await.unwrap_err();
    assert!(matches!(error, GitError::Failed(_)), "{error:?}");
    assert!(error.to_string().contains("nothing to commit") || error.to_string().contains("no changes"), "{error}");
}

fn with_origin() -> (tempfile::TempDir, tempfile::TempDir, tempfile::TempDir) {
    let origin = tempfile::tempdir().unwrap();
    git(origin.path(), &["init", "-q", "--bare", "-b", "main"]);
    let work = tempfile::tempdir().unwrap();
    git(work.path(), &["init", "-q", "-b", "main"]);
    identity(work.path());
    write(work.path(), "a.yml", "name: a\n");
    git(work.path(), &["add", "-A"]);
    git(work.path(), &["commit", "-q", "-m", "départ"]);
    git(work.path(), &["remote", "add", "origin", &origin.path().display().to_string()]);
    let other = tempfile::tempdir().unwrap();
    (origin, work, other)
}

fn clone_of(origin: &Path, into: &Path) {
    git(into, &["clone", "-q", &origin.display().to_string(), "."]);
    identity(into);
}

#[tokio::test]
async fn ef_git_01_the_first_push_creates_the_upstream_and_a_later_one_counts_the_commits_ahead() {
    let (origin, work, _) = with_origin();
    let repo = Repo::open(work.path()).unwrap().unwrap();
    assert_eq!(repo.status().await.unwrap().upstream, None);

    repo.push().await.unwrap();

    let status = repo.status().await.unwrap();
    assert_eq!((status.upstream.as_deref(), status.ahead, status.behind), (Some("origin/main"), 0, 0));
    assert_eq!(git(origin.path(), &["log", "--format=%s", "main"]).trim(), "départ");

    write(work.path(), "a.yml", "name: deux\n");
    repo.commit("deux", &[]).await.unwrap();
    assert_eq!(repo.status().await.unwrap().ahead, 1);
    repo.push().await.unwrap();
    assert_eq!(repo.status().await.unwrap().ahead, 0);
}

#[tokio::test]
async fn ef_git_01_a_pull_brings_in_the_remote_commits_and_a_diverged_branch_is_never_merged() {
    let (origin, work, other) = with_origin();
    let repo = Repo::open(work.path()).unwrap().unwrap();
    repo.push().await.unwrap();
    clone_of(origin.path(), other.path());
    write(other.path(), "b.yml", "name: b\n");
    git(other.path(), &["add", "-A"]);
    git(other.path(), &["commit", "-q", "-m", "distant"]);
    git(other.path(), &["push", "-q"]);
    git(work.path(), &["fetch", "-q"]);
    assert_eq!(repo.status().await.unwrap().behind, 1);

    repo.pull().await.unwrap();

    assert!(work.path().join("b.yml").exists());
    assert_eq!(repo.status().await.unwrap().behind, 0);

    write(other.path(), "c.yml", "name: c\n");
    git(other.path(), &["add", "-A"]);
    git(other.path(), &["commit", "-q", "-m", "distant 2"]);
    git(other.path(), &["push", "-q"]);
    write(work.path(), "d.yml", "name: d\n");
    repo.commit("local", &[]).await.unwrap();
    let error = repo.pull().await.unwrap_err().to_string();
    assert!(error.contains("ont divergé"), "{error}");
    assert!(!work.path().join("c.yml").exists());
    let rejected = repo.push().await.unwrap_err().to_string();
    assert!(rejected.contains("pull"), "{rejected}");
}

#[tokio::test]
async fn ef_git_01_a_push_without_remote_says_so() {
    let dir = repo_with_commit();
    let repo = Repo::open(dir.path()).unwrap().unwrap();
    let error = repo.push().await.unwrap_err().to_string();
    assert!(error.contains("dépôt distant"), "{error}");
}
