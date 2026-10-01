//! Appariement des opérations (EF-SYN-01) : chemins normalisés pour connecter une collection sans base,
//! et rapprochements proposés quand un chemin change sans `operationId`.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use xc_core::{open_collection, RequestDoc, TreeItem};

use super::specs::Op;
use super::{Fate, Item, OpStatus, Pairing, SyncError};
use crate::store::Entry;

/// Le chemin d'une URL : sans `{{variables}}` en tête, sans schéma ni hôte, sans query.
fn path_of(url: &str) -> &str {
    let mut rest = url.trim();
    while let Some(inner) = rest.strip_prefix("{{") {
        match inner.find("}}") {
            Some(end) => rest = &inner[end + 2..],
            None => break,
        }
    }
    if let Some(at) =
        rest.find("://").filter(|at| rest[..*at].chars().all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)))
    {
        let after = &rest[at + 3..];
        rest = after.find('/').map_or("", |slash| &after[slash..]);
    }
    rest.split('?').next().unwrap_or_default()
}

fn is_param(segment: &str) -> bool {
    let named = |inner: &str| !inner.is_empty();
    segment.strip_prefix(':').is_some_and(named)
        || segment
            .strip_prefix('{')
            .and_then(|s| s.strip_suffix('}'))
            .is_some_and(|inner| named(inner) && !inner.starts_with('{'))
}

/// Chemin normalisé : segments `:nom` ou `{nom}` remplacés par `{}`, sans `/` final.
pub(super) fn normalize(url: &str) -> String {
    let segments: Vec<&str> = path_of(url).split('/').map(|s| if is_param(s) { "{}" } else { s }).collect();
    segments.join("/").trim_end_matches('/').to_owned()
}

/// Chemin tel que la spec l'écrit (`/pets/{id}`), pour l'affichage.
pub(super) fn display(url: &str) -> String {
    let segments: Vec<String> = path_of(url)
        .split('/')
        .map(|s| match s.strip_prefix(':').filter(|name| !name.is_empty()) {
            Some(name) => format!("{{{name}}}"),
            None => s.to_owned(),
        })
        .collect();
    let path = segments.join("/");
    if path.is_empty() {
        "/".to_owned()
    } else {
        path
    }
}

/// Sans `source.yml` : chaque opération de la spec est rapprochée de la requête de la collection qui a la même
/// méthode et le même chemin normalisé ; un rapprochement ambigu n'est pas fait.
pub(super) fn connect(root: &Path, specs: &[Op]) -> Result<Vec<Entry>, SyncError> {
    let mut files: HashMap<(String, String), Vec<String>> = HashMap::new();
    let mut stack = open_collection(root)?.items;
    while let Some(item) = stack.pop() {
        match item {
            TreeItem::Folder { children, .. } => stack.extend(children),
            TreeItem::Request { path, method, url, request_type, error: None, .. } if request_type == "http" => {
                files.entry((method, normalize(&url))).or_default().push(path);
            }
            TreeItem::Request { .. } => {}
        }
    }
    let key_of = |doc: &RequestDoc| (doc.method.clone(), normalize(&doc.url));
    let mut wanted: HashMap<(String, String), usize> = HashMap::new();
    for op in specs {
        *wanted.entry(key_of(&op.doc)).or_default() += 1;
    }
    let matched = specs.iter().filter_map(|op| {
        let key = key_of(&op.doc);
        match (wanted[&key], files.get(&key).map(Vec::as_slice)) {
            (1, Some([file])) => Some(Entry::tracked(&op.key, file)),
            _ => None,
        }
    });
    Ok(matched.collect())
}

struct Profile<'a> {
    method: &'a str,
    names: Vec<&'a str>,
    params: BTreeSet<&'a str>,
    segments: Vec<String>,
}

impl<'a> Profile<'a> {
    fn new(docs: &[&'a RequestDoc]) -> Self {
        let doc = docs[0];
        Self {
            method: &doc.method,
            names: docs.iter().map(|d| d.name.as_str()).collect(),
            params: doc.params.iter().map(|p| p.name.as_str()).collect(),
            segments: normalize(&doc.url).split('/').filter(|s| !s.is_empty()).map(str::to_owned).collect(),
        }
    }

    fn resembles(&self, added: &Profile) -> Option<&'static str> {
        if self.method != added.method {
            return None;
        }
        if self.names.iter().any(|name| added.names.contains(name) && !name.is_empty()) {
            return Some("même méthode et même nom de requête");
        }
        if !self.params.is_empty() && self.params == added.params {
            return Some("même méthode et mêmes paramètres");
        }
        let equal = self.segments.iter().zip(&added.segments).filter(|(a, b)| a == b).count();
        let (count, same_count) = (self.segments.len(), self.segments.len() == added.segments.len());
        (same_count && count > 0 && equal * 2 >= count).then_some("même méthode et chemins proches")
    }
}

/// Opérations retirées et nouvelles qui se ressemblent : candidates à un rapprochement manuel.
pub(super) fn suggest(items: &[Item]) -> Vec<Pairing> {
    let with_status = |wanted: OpStatus| items.iter().filter(move |i| matches!(i.fate, Fate::Listed(s) if s == wanted));
    let mut out = Vec::new();
    for removed in with_status(OpStatus::Removed) {
        let docs: Vec<&RequestDoc> =
            removed.base.iter().map(|b| &b.doc).chain(removed.ours.iter().map(|o| &o.doc)).collect();
        let profile = Profile::new(&docs);
        for added in with_status(OpStatus::New) {
            let Some(spec) = &added.spec else { continue };
            if let Some(reason) = profile.resembles(&Profile::new(&[&spec.doc])) {
                out.push(Pairing { removed: removed.key.clone(), added: added.key.clone(), reason: reason.to_owned() });
            }
        }
    }
    out
}
