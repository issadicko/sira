//! Vue d'une opération pour l'éditeur de fusion : les quatre YAML complets et, pour chaque changement, les plages de
//! lignes (1-based, inclusives) de son champ dans chacun.

use std::ops::Range;

use serde::Serialize;
use xc_core::yaml;

use super::apply::render;
use super::{Decisions, Fate, Plan, SyncError};
use crate::merge::{Change, Kind};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hunk {
    pub change_id: String,
    pub ours: Option<[usize; 2]>,
    pub theirs: Option<[usize; 2]>,
    pub base: Option<[usize; 2]>,
    pub result: Option<[usize; 2]>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpView {
    pub ours: String,
    pub theirs: String,
    pub base: Option<String>,
    pub result: String,
    pub hunks: Vec<Hunk>,
}

impl Plan {
    /// Les quatre fichiers de l'opération `key` (équipe, spec, base, résultat selon `decisions`) et les plages à
    /// surligner pour chaque changement qui n'est pas identique des deux côtés.
    pub fn op_view(&self, key: &str, decisions: &Decisions) -> Result<OpView, SyncError> {
        let item = self
            .state
            .items
            .iter()
            .find(|item| item.key == key && matches!(item.fate, Fate::Listed(_)))
            .ok_or_else(|| SyncError::Unknown(key.to_owned()))?;
        let theirs = item.spec.as_ref().map(|spec| spec.text.clone()).unwrap_or_default();
        let base = item.base.as_ref().map(|base| base.text.clone());
        let ours = item.ours.as_ref().map(|ours| ours.text.clone()).unwrap_or_default();
        let merged = item.merge(&decisions.choices)?;
        let result = match (&merged, &item.ours, &item.spec) {
            (Some(merged), Some(ours), Some(spec)) => render(ours, &merged.doc, spec)?,
            (_, Some(_), _) => ours.clone(),
            _ => theirs.clone(),
        };
        let changes = merged.map(|merged| merged.changes).unwrap_or_default();
        let hunks = changes
            .iter()
            .filter(|change| change.kind != Kind::Same)
            .map(|change| hunk(key, change, [&ours, &theirs, base.as_deref().unwrap_or_default(), &result]))
            .collect();
        Ok(OpView { ours, theirs, base, result, hunks })
    }
}

fn hunk(key: &str, change: &Change, [ours, theirs, base, result]: [&str; 4]) -> Hunk {
    let suffix = change.id.strip_prefix(&format!("{key}::")).unwrap_or(&change.id);
    let find = |text: &str| locate(text, suffix).map(|lines| [lines.start + 1, lines.end]);
    Hunk {
        change_id: change.id.clone(),
        ours: find(ours),
        theirs: find(theirs),
        base: find(base),
        result: find(result),
    }
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn is_dash(line: &str) -> bool {
    line.trim_start().starts_with("- ")
}

/// Indentation des enfants du bloc `parent` : celle de sa première ligne qui n'est ni blanche ni un commentaire. Les
/// fichiers écrits à la main n'ont pas tous l'indentation de l'émetteur.
fn depth_of(lines: &[&str], parent: &Range<usize>) -> Option<usize> {
    let line = (parent.start + 1..parent.end)
        .map(|i| lines[i])
        .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'));
    line.map(indent)
}

/// Lignes du bloc de la clé `key`, indentée de `depth`, dans `within` : de sa ligne jusqu'à la suivante qui n'est pas
/// plus profonde, sans les lignes blanches de queue. Les éléments d'une liste peuvent être indentés comme leur clé.
fn block(lines: &[&str], within: Range<usize>, depth: usize, key: &str) -> Option<Range<usize>> {
    let prefix = format!("{}{key}:", " ".repeat(depth));
    let start = within.clone().find(|&i| lines[i].starts_with(&prefix))?;
    let leaves = |line: &str| indent(line) < depth || (indent(line) == depth && !is_dash(line));
    let mut end =
        (start + 1..within.end).find(|&i| !lines[i].trim().is_empty() && leaves(lines[i])).unwrap_or(within.end);
    while end > start + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    Some(start..end)
}

fn child(lines: &[&str], parent: &Range<usize>, key: &str) -> Option<Range<usize>> {
    block(lines, parent.start + 1..parent.end, depth_of(lines, parent)?, key)
}

/// Éléments de la liste du bloc `owner` : un par ligne `- `, toutes indentées pareil. Renvoie aussi l'indentation des
/// champs d'un élément.
fn elements(lines: &[&str], owner: &Range<usize>) -> Option<(Vec<Range<usize>>, usize)> {
    let depth = (owner.start + 1..owner.end).map(|i| lines[i]).find(|l| is_dash(l)).map(indent)?;
    let dash = format!("{}- ", " ".repeat(depth));
    let starts: Vec<usize> = (owner.start + 1..owner.end).filter(|&i| lines[i].starts_with(&dash)).collect();
    let ends = starts.iter().skip(1).copied().chain([owner.end]);
    Some((starts.iter().zip(ends).map(|(&start, end)| start..end).collect(), depth + 2))
}

fn scalar(text: &str) -> Option<String> {
    yaml::parse(text.trim()).ok()?.scalar()
}

/// Valeur de la clé `key` d'un élément de liste dont les champs sont indentés de `depth`.
fn field(lines: &[&str], element: &Range<usize>, depth: usize, key: &str) -> Option<String> {
    let prefix = format!("{key}:");
    element.clone().find_map(|i| {
        let line = lines[i];
        let body = if i == element.start {
            line.trim_start().strip_prefix("- ")?
        } else if indent(line) == depth {
            line.trim_start()
        } else {
            return None;
        };
        scalar(body.strip_prefix(&prefix)?)
    })
}

fn element_named(lines: &[&str], list: &Range<usize>, matches: impl Fn(&str, &str) -> bool) -> Option<Range<usize>> {
    let (elements, depth) = elements(lines, list)?;
    elements.into_iter().find(|element| {
        let name = field(lines, element, depth, "name").unwrap_or_default();
        let kind = field(lines, element, depth, "type").unwrap_or_else(|| "query".into());
        matches(&name, &kind)
    })
}

/// Plage de lignes (0-based, fin exclue) de la section d'un changement dans un fichier de requête.
fn locate(text: &str, suffix: &str) -> Option<Range<usize>> {
    let lines: Vec<&str> = text.lines().collect();
    let http = block(&lines, 0..lines.len(), 0, "http")?;
    let section = |key: &str| child(&lines, &http, key);
    if matches!(suffix, "method" | "url" | "auth" | "body") {
        return section(suffix);
    }
    if let Some(rest) = suffix.strip_prefix("param/") {
        let (kind, name) = rest.split_once('/')?;
        return element_named(&lines, &section("params")?, |n, k| n == name && k == kind);
    }
    if let Some(name) = suffix.strip_prefix("header/") {
        return element_named(&lines, &section("headers")?, |n, _| n.to_lowercase() == name);
    }
    let name = suffix.strip_prefix("body/form/")?;
    let data = child(&lines, &section("body")?, "data")?;
    element_named(&lines, &data, |n, _| n == name)
}
