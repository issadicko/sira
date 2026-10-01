//! Appariement des opérations (EF-SYN-01) : chemins normalisés pour connecter une collection sans base,
//! et rapprochements proposés quand un chemin change sans `operationId`.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;

use xc_core::{open_collection, RequestDoc, TreeItem};

use super::specs::Op;
use super::{Base, Fate, Item, OpStatus, Pairing, SyncError};
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

type Key = (String, String);

fn key_of(doc: &RequestDoc) -> Key {
    (doc.method.clone(), normalize(&doc.url))
}

/// Chemins relatifs des requêtes HTTP lisibles de la collection, par méthode et chemin normalisé.
fn requests(root: &Path) -> Result<HashMap<Key, Vec<String>>, SyncError> {
    let mut files: HashMap<Key, Vec<String>> = HashMap::new();
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
    Ok(files)
}

/// Nombre d'opérations de la spec pour chaque méthode et chemin normalisé.
fn wanted(specs: &[Op]) -> HashMap<Key, usize> {
    let mut wanted = HashMap::new();
    for op in specs {
        *wanted.entry(key_of(&op.doc)).or_default() += 1;
    }
    wanted
}

/// Sans `source.yml` : chaque opération de la spec est rapprochée de la requête de la collection qui a la même
/// méthode et le même chemin normalisé ; un rapprochement ambigu n'est pas fait.
pub(super) fn connect(root: &Path, specs: &[Op]) -> Result<Vec<Entry>, SyncError> {
    let files = requests(root)?;
    let wanted = wanted(specs);
    let matched = specs.iter().filter_map(|op| {
        let key = key_of(&op.doc);
        match (wanted[&key], files.get(&key).map(Vec::as_slice)) {
            (1, Some([file])) => Some(Entry::tracked(&op.key, file)),
            _ => None,
        }
    });
    Ok(matched.collect())
}

/// Retrouve le fichier d'une opération dont le fichier suivi a disparu (déplacé ou renommé par l'équipe) : la
/// requête non suivie qui a la même méthode et le même chemin normalisé, selon la spec puis selon la base, si elle
/// est la seule. Même logique que la connexion sans base ; en cas d'ambiguïté, l'opération reste manquante.
pub(super) struct Relinker<'a> {
    root: &'a Path,
    tracked: HashSet<String>,
    wanted: HashMap<Key, usize>,
    files: Option<HashMap<Key, Vec<String>>>,
}

impl<'a> Relinker<'a> {
    pub fn new(root: &'a Path, entries: &[Entry], specs: &[Op]) -> Self {
        let tracked = entries.iter().filter(|e| !e.ignored).filter_map(|e| e.file.as_deref()).map(str::to_lowercase);
        Self { root, tracked: tracked.collect(), wanted: wanted(specs), files: None }
    }

    pub fn find(&mut self, spec: &Op, base: Option<&Base>) -> Result<Option<String>, SyncError> {
        let own = key_of(&spec.doc);
        let keys = [Some(own.clone()), base.map(|b| key_of(&b.doc))];
        for key in keys.into_iter().flatten() {
            let rivals = self.wanted.get(&key).copied().unwrap_or(0) - usize::from(key == own);
            if rivals > 0 {
                continue;
            }
            if self.files.is_none() {
                self.files = Some(requests(self.root)?);
            }
            let free: Vec<&String> = (self.files.iter().flat_map(|files| files.get(&key)).flatten())
                .filter(|file| !self.tracked.contains(&file.to_lowercase()))
                .collect();
            if let [file] = free.as_slice() {
                let file = (*file).clone();
                self.tracked.insert(file.to_lowercase());
                return Ok(Some(file));
            }
        }
        Ok(None)
    }
}

struct Profile<'a> {
    method: &'a str,
    names: Vec<&'a str>,
    params: BTreeSet<&'a str>,
    segments: Vec<String>,
}

/// Plus petit est meilleur : le critère de ressemblance, puis le nombre de segments égaux.
type Score = (u8, std::cmp::Reverse<usize>);

const REASONS: [&str; 3] =
    ["même méthode et même nom de requête", "même méthode et mêmes paramètres", "même méthode et chemins proches"];

/// Nombre de rapprochements proposés au plus : une liste plus longue ne se relit plus.
const MAX_SUGGESTIONS: usize = 50;

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

    fn resembles(&self, added: &Profile) -> Option<Score> {
        if self.method != added.method {
            return None;
        }
        let equal = self.segments.iter().zip(&added.segments).filter(|(a, b)| a == b).count();
        let rank = |reason: u8| Some((reason, std::cmp::Reverse(equal)));
        if self.names.iter().any(|name| added.names.contains(name) && !name.is_empty()) {
            return rank(0);
        }
        if !self.params.is_empty() && self.params == added.params {
            return rank(1);
        }
        let (count, same_count) = (self.segments.len(), self.segments.len() == added.segments.len());
        if same_count && count > 0 && equal * 2 >= count {
            return rank(2);
        }
        None
    }
}

/// Opérations retirées et nouvelles qui se ressemblent : candidates à un rapprochement manuel. Chaque opération
/// retirée n'a qu'une candidate, la plus proche, et une nouvelle opération n'est proposée qu'à une seule ; au plus
/// [`MAX_SUGGESTIONS`] rapprochements, les plus sûrs d'abord.
pub(super) fn suggest(items: &[Item]) -> Vec<Pairing> {
    let with_status = |wanted: OpStatus| items.iter().filter(move |i| matches!(i.fate, Fate::Listed(s) if s == wanted));
    let added: Vec<(&Item, Profile)> = with_status(OpStatus::New)
        .filter_map(|i| i.spec.as_ref().map(|spec| (i, Profile::new(&[&spec.doc]))))
        .collect();
    let mut pairs = Vec::new();
    for (removed_at, removed) in with_status(OpStatus::Removed).enumerate() {
        let docs: Vec<&RequestDoc> =
            removed.base.iter().map(|b| &b.doc).chain(removed.ours.iter().map(|o| &o.doc)).collect();
        let profile = Profile::new(&docs);
        let best = added.iter().enumerate().filter_map(|(at, (_, other))| Some((profile.resembles(other)?, at))).min();
        pairs.extend(best.map(|(score, at)| (score, removed_at, at)));
    }
    pairs.sort();
    let (mut removed_used, mut added_used) = (HashSet::new(), HashSet::new());
    let mut chosen: Vec<_> = pairs
        .into_iter()
        .filter(|(_, removed_at, at)| removed_used.insert(*removed_at) && added_used.insert(*at))
        .take(MAX_SUGGESTIONS)
        .collect();
    chosen.sort_by_key(|(_, removed_at, at)| (*removed_at, *at));
    let removed: Vec<&Item> = with_status(OpStatus::Removed).collect();
    chosen
        .into_iter()
        .map(|((reason, _), removed_at, at)| Pairing {
            removed: removed[removed_at].key.clone(),
            added: added[at].0.key.clone(),
            reason: REASONS[usize::from(reason)].to_owned(),
        })
        .collect()
}
