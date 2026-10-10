//! Lancer l'exécutable `git` sans jamais rester bloqué : pas de question sur le terminal, délai borné.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::{GitError, Result};

/// Les messages de `git` en anglais, pour reconnaître les cas que l'on explique en français.
const ENV: [(&str, &str); 3] = [("LC_ALL", "C"), ("GIT_TERMINAL_PROMPT", "0"), ("GIT_EDITOR", "true")];

fn explain(args: &[&str], stderr: &str) -> String {
    let text = stderr.trim();
    let lower = text.to_lowercase();
    let hint = if lower.contains("please tell me who you are") || lower.contains("unable to auto-detect email") {
        Some("Git ne connaît pas ton identité : configure `git config --global user.name` et `user.email`.")
    } else if lower.contains("could not read username")
        || lower.contains("authentication failed")
        || lower.contains("permission denied (publickey)")
    {
        Some("Authentification refusée : configure un assistant d'identifiants Git ou une clé SSH pour ce dépôt.")
    } else if lower.contains("not possible to fast-forward") || lower.contains("diverging branches") {
        Some("La branche locale et la branche distante ont divergé : rien n'a été fusionné. Résous-le avec Git (rebase ou fusion), puis recharge.")
    } else if lower.contains("no tracking information") || lower.contains("no upstream") {
        Some("La branche n'a pas de branche amont : pousse-la d'abord.")
    } else if lower.contains("would be overwritten") || lower.contains("local changes") {
        Some("Des modifications locales gêneraient la mise à jour : valide-les ou mets-les de côté d'abord.")
    } else if lower.contains("non-fast-forward") || lower.contains("fetch first") || lower.contains("rejected") {
        Some("Le serveur a refusé le push : récupère d'abord les changements distants (pull).")
    } else if lower.contains("no remote") || lower.contains("does not appear to be a git repository") {
        Some("Aucun dépôt distant n'est configuré (`git remote add origin <url>`).")
    } else {
        None
    };
    match hint {
        Some(hint) => format!("{hint}\n\n{text}"),
        None => {
            if text.is_empty() {
                format!("git {} a échoué", args.first().copied().unwrap_or_default())
            } else {
                text.to_owned()
            }
        }
    }
}

pub async fn git(dir: &Path, args: &[&str], timeout: Duration) -> Result<String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        .args(args)
        .envs(ENV)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            GitError::GitMissing
        } else {
            GitError::Io(error.to_string())
        }
    })?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| {
            GitError::Failed(format!(
                "git {} n'a pas répondu en {} s",
                args.first().copied().unwrap_or_default(),
                timeout.as_secs()
            ))
        })?
        .map_err(|error| GitError::Io(error.to_string()))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() { String::from_utf8_lossy(&output.stdout) } else { stderr };
        Err(GitError::Failed(explain(args, &detail)))
    }
}

#[cfg(test)]
mod tests {
    use super::explain;

    #[test]
    fn ef_git_01_the_usual_git_failures_are_explained_in_french_with_the_original_message_kept() {
        let identity = explain(&["commit"], "Author identity unknown\n\n*** Please tell me who you are.");
        assert!(identity.starts_with("Git ne connaît pas ton identité"), "{identity}");
        assert!(identity.contains("Please tell me who you are"), "{identity}");
        let auth = explain(&["push"], "fatal: could not read Username for 'https://x.test': terminal prompts disabled");
        assert!(auth.starts_with("Authentification refusée"), "{auth}");
        let unknown = explain(&["pull"], "fatal: boom");
        assert_eq!(unknown, "fatal: boom");
        assert_eq!(explain(&["status"], "  "), "git status a échoué");
    }
}
