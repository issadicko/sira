//! Lire la sortie de `git status --porcelain=v2 -z --branch`.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    /// Relatif à la collection.
    pub path: String,
    /// `M` modifié, `A` ajouté ou nouveau, `D` supprimé, `R` renommé, `U` en conflit.
    pub state: char,
    /// Déjà ajouté à l'index.
    pub staged: bool,
    /// Pas encore suivi par Git.
    pub untracked: bool,
    /// L'ancien chemin d'un fichier renommé.
    pub from: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// `None` : tête détachée ou dépôt sans commit.
    pub branch: Option<String>,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// Vrai tant que le dépôt n'a aucun commit.
    pub unborn: bool,
    pub files: Vec<FileChange>,
}

fn relative(path: &str, prefix: &str) -> String {
    path.strip_prefix(prefix).unwrap_or(path).to_owned()
}

fn summary(x: char, y: char) -> char {
    if x == 'U' || y == 'U' || (x == 'A' && y == 'A') || (x == 'D' && y == 'D') {
        'U'
    } else if x == 'D' || y == 'D' {
        'D'
    } else if x == 'R' || y == 'R' {
        'R'
    } else if x == 'A' {
        'A'
    } else {
        'M'
    }
}

/// `prefix` est le chemin de la collection depuis la racine du dépôt (avec `/` final), que `git status` met devant chaque
/// chemin et que l'on retire.
pub fn parse_status(output: &str, prefix: &str) -> Status {
    let mut status = Status::default();
    let mut fields = output.split('\0').filter(|f| !f.is_empty());
    while let Some(entry) = fields.next() {
        if let Some(header) = entry.strip_prefix("# ") {
            let (key, value) = header.split_once(' ').unwrap_or((header, ""));
            match key {
                "branch.oid" => status.unborn = value == "(initial)",
                "branch.head" => status.branch = (value != "(detached)").then(|| value.to_owned()),
                "branch.upstream" => status.upstream = Some(value.to_owned()),
                "branch.ab" => {
                    let mut counts =
                        value.split(' ').filter_map(|n| n.trim_start_matches(['+', '-']).parse::<u32>().ok());
                    status.ahead = counts.next().unwrap_or(0);
                    status.behind = counts.next().unwrap_or(0);
                }
                _ => {}
            }
            continue;
        }
        let kind = entry.chars().next().unwrap_or(' ');
        let parts = |count: usize| entry.splitn(count, ' ').collect::<Vec<_>>();
        match kind {
            '1' => {
                let p = parts(9);
                let mut xy = p[1].chars();
                let (x, y) = (xy.next().unwrap_or('.'), xy.next().unwrap_or('.'));
                status.files.push(FileChange {
                    path: relative(p[8], prefix),
                    state: summary(x, y),
                    staged: x != '.',
                    untracked: false,
                    from: None,
                });
            }
            '2' => {
                let p = parts(10);
                let mut xy = p[1].chars();
                let (x, y) = (xy.next().unwrap_or('.'), xy.next().unwrap_or('.'));
                let from = fields.next().map(|f| relative(f, prefix));
                status.files.push(FileChange {
                    path: relative(p[9], prefix),
                    state: summary(x, y),
                    staged: x != '.',
                    untracked: false,
                    from,
                });
            }
            'u' => {
                let p = parts(11);
                status.files.push(FileChange {
                    path: relative(p[10], prefix),
                    state: 'U',
                    staged: false,
                    untracked: false,
                    from: None,
                });
            }
            '?' => {
                status.files.push(FileChange {
                    path: relative(&entry[2..], prefix),
                    state: 'A',
                    staged: false,
                    untracked: true,
                    from: None,
                });
            }
            _ => {}
        }
    }
    status.files.sort_by(|a, b| a.path.cmp(&b.path));
    status
}
