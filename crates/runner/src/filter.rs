//! Choisir les requêtes d'un run comme `bru run --tests-only`, `--tags` et `--exclude-tags`.

use std::path::Path;

use xc_core::{read_request, RequestDoc};

use crate::run::Item;
use crate::scripts::{AFTER_RESPONSE, BEFORE_REQUEST, TESTS};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Seules les requêtes qui ont un test ou une assertion active.
    pub tests_only: bool,
    /// Une requête n'est gardée que si elle porte l'un de ces tags ; vide : toutes.
    pub tags: Vec<String>,
    /// Une requête qui porte l'un de ces tags est écartée.
    pub exclude_tags: Vec<String>,
}

impl Filter {
    pub fn is_empty(&self) -> bool {
        !self.tests_only && self.tags.is_empty() && self.exclude_tags.is_empty()
    }

    /// `--tags=a,b` : les noms séparés par des virgules, sans autre nettoyage, comme Bruno.
    pub fn split(list: Option<&str>) -> Vec<String> {
        list.filter(|l| !l.is_empty()).map(|l| l.split(',').map(str::to_owned).collect()).unwrap_or_default()
    }

    pub fn keeps(&self, doc: &RequestDoc) -> bool {
        if self.tests_only && !has_tests(doc) {
            return false;
        }
        let included = self.tags.is_empty() || doc.tags.iter().any(|t| self.tags.contains(t));
        let excluded = !self.exclude_tags.is_empty() && doc.tags.iter().any(|t| self.exclude_tags.contains(t));
        included && !excluded
    }
}

/// Une assertion active ou un `test(` exécutable dans l'un des trois scripts de la requête.
fn has_tests(doc: &RequestDoc) -> bool {
    doc.assertions.iter().any(|a| a.enabled)
        || doc
            .scripts
            .iter()
            .filter(|s| matches!(s.kind.as_str(), BEFORE_REQUEST | AFTER_RESPONSE | TESTS))
            .any(|s| has_executable_test(&s.code))
}

fn line_end(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

fn strip_line_comments(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' && chars.get(i + 1) == Some(&'/') {
            while i < chars.len() && !line_end(chars[i]) {
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn strip_block_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("/*") {
        match rest[start + 2..].find("*/") {
            Some(end) => {
                out.push_str(&rest[..start]);
                rest = &rest[start + 2 + end + 2..];
            }
            None => break,
        }
    }
    out.push_str(rest);
    out
}

/// Remplace chaque texte entre `quote` (échappements admis, retour à la ligne interdit dans un échappement) par deux
/// guillemets vides, comme les expressions régulières de Bruno ; un guillemet sans fermeture reste tel quel.
fn blank_strings(text: &str, quote: char) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != quote {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut closed = false;
        while j < chars.len() {
            match chars[j] {
                c if c == quote => {
                    closed = true;
                    break;
                }
                '\\' if j + 1 < chars.len() && !line_end(chars[j + 1]) => j += 2,
                '\\' => break,
                _ => j += 1,
            }
        }
        if closed {
            out.push(quote);
            out.push(quote);
            i = j + 1;
        } else {
            out.push(quote);
            i += 1;
        }
    }
    out
}

/// Un appel `test(` qui n'est ni en commentaire, ni dans un texte, ni une méthode (`obj.test(`) : la règle de Bruno.
pub fn has_executable_test(script: &str) -> bool {
    let clean = strip_block_comments(&strip_line_comments(script));
    let clean = blank_strings(&blank_strings(&blank_strings(&clean, '"'), '\''), '`');
    clean.match_indices("test").any(|(at, word)| {
        let after = clean[at + word.len()..].trim_start();
        after.starts_with('(') && !clean[..at].ends_with('.')
    })
}

/// Les requêtes de `items` que `filter` garde. Un fichier illisible reste : son erreur doit figurer au rapport.
pub fn filter_items(root: &Path, items: Vec<Item>, filter: &Filter) -> Vec<Item> {
    if filter.is_empty() {
        return items;
    }
    items
        .into_iter()
        .filter(|item| {
            item.unreadable.is_some() || read_request(root, &item.path).map_or(true, |doc| filter.keeps(&doc))
        })
        .collect()
}
