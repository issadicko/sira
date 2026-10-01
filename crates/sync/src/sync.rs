//! Synchro OpenAPI à 3 voies (`docs/docs/synchro-openapi.md` § 3, 4 et 6) : `plan` compare la nouvelle spec à la
//! base (copie brute de la spec de la dernière synchro) et aux fichiers de l'équipe sans rien écrire ; `Plan::apply`
//! écrit selon les décisions, `source.yml` en dernier ; `Plan::op_view` prépare l'éditeur de fusion.

mod apply;
mod create;
mod pairing;
mod specs;
mod view;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use xc_core::collection::COLLECTION_FILE;
use xc_core::CoreError;

use crate::import::{fetch_spec, is_url, source_value, ImportError};
use crate::merge::{merge, Change, Choice, Choices, Decision, Kind, MergeError, Merged};
use crate::openapi::{load_spec, summary, GroupBy, OpenApiError};
use crate::store::{self, Entry, Store};
use pairing::Relinker;
use specs::{load_ours, Op, Ours};

pub use apply::Report;
pub(crate) use create::write_parallel;
pub use view::{Hunk, OpView};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error(transparent)]
    Import(#[from] ImportError),
    #[error(transparent)]
    Spec(#[from] OpenApiError),
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error(transparent)]
    Merge(#[from] MergeError),
    #[error("la collection n'est pas connectée à une spec OpenAPI : indiquer la source")]
    NotConnected,
    #[error("la spec ne contient aucune opération alors que {0} opération(s) sont suivies : spec vide ou tronquée, rien n'est comparé")]
    EmptySpec(usize),
    #[error("fichier de l'équipe illisible : {file} : {message}")]
    Unreadable { file: String, message: String },
    #[error("{0} n'est pas une requête HTTP (info.type : http) : la synchro ne l'écrit pas")]
    NotARequest(String),
    #[error("rapprochement impossible : {0}")]
    Pairing(String),
    #[error("{count} conflit(s) sans choix : {ids}")]
    Unresolved { count: usize, ids: String },
    #[error("{0} a changé depuis la comparaison : relancer la comparaison")]
    Stale(String),
    #[error("opération inconnue : {0}")]
    Unknown(String),
}

impl SyncError {
    /// L'entrée (spec illisible, collection non connectée ou illisible) est en cause, pas la synchro elle-même.
    pub fn is_input(&self) -> bool {
        match self {
            Self::Import(e) => e.is_input(),
            Self::Spec(OpenApiError::Syntax(_) | OpenApiError::Empty)
            | Self::NotConnected
            | Self::EmptySpec(_)
            | Self::Unreadable { .. } => true,
            Self::Core(CoreError::NotACollection(_) | CoreError::Yaml { .. }) => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpStatus {
    Unchanged,
    Updated,
    Kept,
    Merged,
    Conflict,
    New,
    Removed,
    Restored,
    Missing,
}

/// État de la connexion d'une collection à une spec.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub connected: bool,
    pub source: Option<String>,
    pub group_by: Option<GroupBy>,
    pub operation_count: usize,
    pub removed_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpecRef {
    pub title: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub key: String,
    pub name: String,
    pub method: String,
    pub path: String,
    pub file: Option<String>,
    pub status: OpStatus,
    pub changes: Vec<Change>,
}

/// Rapprochement proposé entre une opération retirée et une nouvelle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pairing {
    pub removed: String,
    pub added: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub unchanged: usize,
    pub updated: usize,
    pub kept: usize,
    pub merged: usize,
    pub conflicts: usize,
    pub conflict_fields: usize,
    pub created: usize,
    pub removed: usize,
    pub restored: usize,
    pub missing: usize,
}

/// Arbitrage : un choix par conflit, les nouvelles opérations à ne pas créer (`skip`) et les manquantes à recréer
/// (`recreate`) ; une opération manquante est sinon oubliée.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Decisions {
    #[serde(default)]
    pub choices: Choices,
    #[serde(default)]
    pub skip: Vec<String>,
    #[serde(default)]
    pub recreate: Vec<String>,
}

impl Decisions {
    /// Le même choix pour tous les conflits du plan.
    pub fn uniform(plan: &Plan, choice: Choice) -> Self {
        let conflicts = plan.operations.iter().flat_map(|op| &op.changes).filter(|c| c.kind == Kind::Conflict);
        let choices = conflicts.map(|c| (c.id.clone(), Decision { choice, value: None })).collect();
        Self { choices, ..Self::default() }
    }
}

enum Fate {
    Listed(OpStatus),
    Carry(Entry),
}

struct Base {
    text: String,
    doc: xc_core::RequestDoc,
}

/// Les opérations de la base, par clé.
type Bases = HashMap<String, Base>;

struct Item {
    key: String,
    entry: Option<Entry>,
    spec: Option<Op>,
    base: Option<Base>,
    ours: Option<Ours>,
    changes: Vec<Change>,
    fate: Fate,
}

impl Item {
    fn bare(key: &str, entry: Option<&Entry>, fate: Fate) -> Self {
        Self {
            key: key.to_owned(),
            entry: entry.cloned(),
            spec: None,
            base: None,
            ours: None,
            changes: Vec::new(),
            fate,
        }
    }

    fn merge(&self, choices: &Choices) -> Result<Option<Merged>, MergeError> {
        let (Some(ours), Some(spec)) = (&self.ours, &self.spec) else { return Ok(None) };
        merge(&spec.key, self.base.as_ref().map(|b| &b.doc), &ours.doc, &spec.doc, choices).map(Some)
    }

    fn operation(&self) -> Option<Operation> {
        let Fate::Listed(status) = self.fate else { return None };
        let doc = self.spec.as_ref().map(|s| &s.doc).or(self.ours.as_ref().map(|o| &o.doc))?;
        let name = self.ours.as_ref().map_or(&doc.name, |o| &o.doc.name);
        let file = self.ours.as_ref().map(|o| o.file.clone()).or(self.entry.as_ref().and_then(|e| e.file.clone()));
        Some(Operation {
            key: self.key.clone(),
            name: name.clone(),
            method: doc.method.clone(),
            path: pairing::display(&doc.url),
            file,
            status,
            changes: self.changes.clone(),
        })
    }
}

struct State {
    root: PathBuf,
    spec_text: String,
    items: Vec<Item>,
}

impl State {
    /// Fichiers (en minuscules) que suit une entrée qui reste, et ceux que ne suit plus qu'une opération retirée : une
    /// nouvelle opération ne réutilise pas les premiers et peut reprendre les seconds.
    fn claimed_files(&self) -> (HashSet<String>, HashSet<String>) {
        let (mut held, mut adoptable) = (HashSet::new(), HashSet::new());
        for item in &self.items {
            let (file, removed) = match (&item.fate, &item.ours) {
                (Fate::Carry(entry), _) if !entry.ignored => (entry.file.as_deref(), entry.removed),
                (Fate::Listed(OpStatus::Removed), Some(ours)) => (Some(ours.file.as_str()), true),
                (Fate::Listed(_), Some(ours)) => (Some(ours.file.as_str()), false),
                _ => continue,
            };
            let set = if removed { &mut adoptable } else { &mut held };
            set.extend(file.map(str::to_lowercase));
        }
        (held, adoptable)
    }
}

impl std::fmt::Debug for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("State").field("root", &self.root).field("items", &self.items.len()).finish()
    }
}

/// Résultat de la comparaison. Rien n'est écrit tant que [`Plan::apply`] n'est pas appelé.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub source: String,
    pub group_by: GroupBy,
    pub has_base: bool,
    pub from: Option<SpecRef>,
    pub to: SpecRef,
    pub summary: Summary,
    pub operations: Vec<Operation>,
    pub suggestions: Vec<Pairing>,
    #[serde(skip)]
    state: State,
}

impl Plan {
    /// Une opération au moins a divergé de la base, ou son fichier a disparu (`--check`).
    pub fn diverged(&self) -> bool {
        use OpStatus::{Conflict, Merged, Missing, New, Removed, Restored, Updated};
        self.operations
            .iter()
            .any(|op| matches!(op.status, Updated | Merged | Conflict | New | Removed | Restored | Missing))
    }
}

/// Lit `source.yml` : l'état de la connexion de la collection.
pub fn status(root: &Path) -> Result<SyncStatus, SyncError> {
    let Some(store) = store::read(root)? else {
        return Ok(SyncStatus { connected: false, source: None, group_by: None, operation_count: 0, removed_count: 0 });
    };
    Ok(SyncStatus {
        connected: true,
        operation_count: store.operations.iter().filter(|e| !e.ignored).count(),
        removed_count: store.operations.iter().filter(|e| e.removed).count(),
        group_by: Some(store.group_by),
        source: Some(store.source),
    })
}

/// Contenu de la spec et valeur à enregistrer dans `source.yml`. `source` absent : la source enregistrée, une URL ou
/// un chemin relatif à la collection ; sinon un fichier (relatif au dossier courant) ou une URL.
pub async fn read_source(root: &Path, source: Option<&str>) -> Result<(String, String), SyncError> {
    match (source, store::read(root)?) {
        (Some(source), _) => Ok((fetch_spec(source).await?, source_value(source, root))),
        (None, Some(store)) => {
            let location = match is_url(&store.source) {
                true => store.source.clone(),
                false => root.join(&store.source).to_string_lossy().into_owned(),
            };
            Ok((fetch_spec(&location).await?, store.source))
        }
        (None, None) => Err(SyncError::NotConnected),
    }
}

fn spec_ref(spec: &Value) -> SpecRef {
    let summary = summary(spec);
    SpecRef { title: summary.title, version: summary.version.unwrap_or_default() }
}

/// La base : la version de la spec et chaque opération telle qu'elle était. `None` quand la copie brute de la spec a
/// disparu : la synchro se fait alors sans base, et conflit au moindre doute.
fn load_base(root: &Path, store: &Store) -> Result<Option<(SpecRef, Bases)>, SyncError> {
    let Some(text) = store::read_spec(root, store)? else { return Ok(None) };
    let spec = load_spec(&text)?;
    let bases = specs::operations(&spec, store.group_by)?;
    let bases = bases.into_iter().map(|op| (op.key, Base { text: op.text, doc: op.doc })).collect();
    Ok(Some((spec_ref(&spec), bases)))
}

/// Compare `spec_text` à la base et à la collection, sans rien écrire. `source` est la valeur à enregistrer dans
/// `source.yml` ; `pairings` sont les rapprochements manuels (clé retirée, clé ajoutée). Sans `source.yml`, il n'y a
/// pas de base : chaque opération est rapprochée d'une requête de même méthode et de même chemin normalisé. Une spec
/// sans aucune opération est refusée quand des opérations sont suivies : elle retirerait toutes les requêtes.
pub fn plan(root: &Path, spec_text: &str, source: String, pairings: &[(String, String)]) -> Result<Plan, SyncError> {
    if !root.join(COLLECTION_FILE).is_file() {
        return Err(CoreError::NotACollection(root.display().to_string()).into());
    }
    let spec = load_spec(spec_text)?;
    let store = store::read(root)?;
    let group_by = store.as_ref().map_or(GroupBy::Tags, |s| s.group_by);
    let specs = specs::operations(&spec, group_by)?;
    let tracked = store.iter().flat_map(|s| &s.operations).filter(|e| !e.ignored && !e.removed).count();
    if specs.is_empty() && tracked > 0 {
        return Err(SyncError::EmptySpec(tracked));
    }
    let loaded = match &store {
        Some(store) => load_base(root, store)?,
        None => None,
    };
    let (from, bases) = match loaded {
        Some((from, bases)) => (Some(from), bases),
        None => (None, HashMap::new()),
    };
    let entries = match &store {
        Some(store) => store.operations.clone(),
        None => pairing::connect(root, &specs)?,
    };
    let items = classify(root, specs, &entries, bases, pairings)?;
    let operations: Vec<Operation> = items.iter().filter_map(Item::operation).collect();
    Ok(Plan {
        source,
        group_by,
        has_base: from.is_some(),
        from,
        to: spec_ref(&spec),
        summary: summarize(&operations),
        suggestions: pairing::suggest(&items),
        operations,
        state: State { root: root.to_path_buf(), spec_text: spec_text.to_owned(), items },
    })
}

fn summarize(operations: &[Operation]) -> Summary {
    let mut summary = Summary::default();
    for op in operations {
        let conflicts = op.changes.iter().filter(|c| c.kind == Kind::Conflict).count();
        summary.conflict_fields += conflicts;
        match op.status {
            OpStatus::Unchanged => summary.unchanged += 1,
            OpStatus::Updated => summary.updated += 1,
            OpStatus::Kept => summary.kept += 1,
            OpStatus::Merged => summary.merged += 1,
            OpStatus::Conflict => summary.conflicts += 1,
            OpStatus::New => summary.created += 1,
            OpStatus::Removed => summary.removed += 1,
            OpStatus::Restored => summary.restored += 1,
            OpStatus::Missing => summary.missing += 1,
        }
    }
    summary
}

fn status_of(changes: &[Change], restored: bool) -> OpStatus {
    let any = |wanted: &[Kind]| changes.iter().any(|c| wanted.contains(&c.kind));
    if any(&[Kind::Conflict]) {
        return OpStatus::Conflict;
    }
    if restored {
        return OpStatus::Restored;
    }
    match (any(&[Kind::Applied, Kind::Merged]), any(&[Kind::Kept, Kind::Merged])) {
        (true, true) => OpStatus::Merged,
        (true, false) => OpStatus::Updated,
        (false, true) => OpStatus::Kept,
        (false, false) => OpStatus::Unchanged,
    }
}

/// Opérations de la spec dans son ordre, puis celles qui n'y sont plus.
fn classify(
    root: &Path,
    specs: Vec<Op>,
    entries: &[Entry],
    mut bases: Bases,
    pairings: &[(String, String)],
) -> Result<Vec<Item>, SyncError> {
    let spec_keys: HashSet<String> = specs.iter().map(|s| s.key.clone()).collect();
    let mut by_key: HashMap<&str, usize> = HashMap::new();
    for (i, entry) in entries.iter().enumerate() {
        by_key.entry(&entry.key).or_insert(i);
    }
    let paired = validate_pairings(pairings, entries, &by_key, &spec_keys)?;
    let mut relinker = Relinker::new(root, entries, &specs);
    let mut used = vec![false; entries.len()];
    let mut items = Vec::new();
    for spec in specs {
        let index = by_key.get(spec.key.as_str()).or_else(|| paired.get(&spec.key)).copied();
        if let Some(i) = index {
            used[i] = true;
        }
        items.push(match index {
            None => with_spec(spec, None, Fate::Listed(OpStatus::New)),
            Some(i) => matched(root, &entries[i], spec, &mut bases, &mut relinker)?,
        });
    }
    for (i, entry) in entries.iter().enumerate() {
        if !used[i] && !spec_keys.contains(&entry.key) {
            items.extend(absent(root, entry, &mut bases)?);
        }
    }
    Ok(items)
}

fn with_spec(spec: Op, entry: Option<&Entry>, fate: Fate) -> Item {
    let key = spec.key.clone();
    Item { spec: Some(spec), ..Item::bare(&key, entry, fate) }
}

fn validate_pairings(
    pairings: &[(String, String)],
    entries: &[Entry],
    by_key: &HashMap<&str, usize>,
    spec_keys: &HashSet<String>,
) -> Result<HashMap<String, usize>, SyncError> {
    let mut paired = HashMap::new();
    let mut taken = HashSet::new();
    for (removed, added) in pairings {
        let index =
            by_key.get(removed.as_str()).copied().filter(|&i| !entries[i].ignored && !spec_keys.contains(removed));
        let Some(index) = index else {
            return Err(SyncError::Pairing(format!("« {removed} » n'est pas une opération retirée de la spec")));
        };
        if !spec_keys.contains(added) || by_key.contains_key(added.as_str()) {
            return Err(SyncError::Pairing(format!("« {added} » n'est pas une nouvelle opération")));
        }
        if !taken.insert(index) || paired.insert(added.clone(), index).is_some() {
            return Err(SyncError::Pairing(format!("« {removed} » ou « {added} » est rapprochée deux fois")));
        }
    }
    Ok(paired)
}

/// Dernière version de la requête d'une opération que la spec avait retirée : la base d'une opération restaurée.
fn removed_base(root: &Path, entry: &Entry) -> Option<Base> {
    let text = store::read_removed(root, &entry.key).filter(|_| entry.removed)?;
    let doc = xc_core::RequestDoc::from_tree(&specs::tree(&text, &entry.key).ok()?);
    Some(Base { text, doc })
}

fn matched(
    root: &Path,
    entry: &Entry,
    spec: Op,
    bases: &mut Bases,
    relinker: &mut Relinker,
) -> Result<Item, SyncError> {
    if entry.ignored {
        return Ok(Item::bare(&spec.key, Some(entry), Fate::Carry(entry.clone())));
    }
    let base = bases.remove(&entry.key).or_else(|| removed_base(root, entry));
    let mut entry = entry.clone();
    let mut ours = load_ours(root, entry.file.as_deref().unwrap_or_default())?;
    if ours.is_none() {
        if let Some(file) = relinker.find(&spec, base.as_ref())? {
            ours = load_ours(root, &file)?;
            entry.file = Some(file);
        }
    }
    let Some(ours) = ours else { return Ok(with_spec(spec, Some(&entry), Fate::Listed(OpStatus::Missing))) };
    let merged = merge(&spec.key, base.as_ref().map(|b| &b.doc), &ours.doc, &spec.doc, &Choices::new())?;
    let status = status_of(&merged.changes, entry.removed);
    Ok(Item {
        key: spec.key.clone(),
        entry: Some(entry),
        spec: Some(spec),
        base,
        ours: Some(ours),
        changes: merged.changes,
        fate: Fate::Listed(status),
    })
}

fn absent(root: &Path, entry: &Entry, bases: &mut Bases) -> Result<Option<Item>, SyncError> {
    let file = entry.file.as_deref().unwrap_or_default();
    if entry.ignored {
        return Ok(None);
    }
    if entry.removed {
        let kept = specs::exists(root, file)?;
        return Ok(kept.then(|| Item::bare(&entry.key, Some(entry), Fate::Carry(entry.clone()))));
    }
    let Some(ours) = load_ours(root, file)? else { return Ok(None) };
    Ok(Some(Item {
        base: bases.remove(&entry.key),
        ours: Some(ours),
        ..Item::bare(&entry.key, Some(entry), Fate::Listed(OpStatus::Removed))
    }))
}
