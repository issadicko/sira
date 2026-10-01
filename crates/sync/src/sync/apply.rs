//! Application d'un plan (§ 6) : fichiers modifiés, nouveaux fichiers, copie brute de la spec, puis `source.yml`.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde::Serialize;
use xc_core::collection::{resolve_visible_path, write_atomic};
use xc_core::request::{BLANK_BEFORE, HTTP_ORDER, TOP_ORDER};
use xc_core::yaml::{emit, Map, Value};
use xc_core::{Auth, Body, CoreError, RequestDoc};

use super::create::{write_parallel, Creator};
use super::specs::{tree, Op, Ours};
use super::{Decisions, Fate, OpStatus, Plan, SyncError};
use crate::store::{self, Entry};

/// Ce que l'application a fait : chemins relatifs des fichiers réécrits, créés et marqués retirés de la spec,
/// clés des opérations désormais ignorées.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub written: Vec<String>,
    pub created: Vec<String>,
    pub removed: Vec<String>,
    pub ignored: Vec<String>,
}

impl Plan {
    /// Écrit le plan selon les décisions, dans cet ordre : fichiers modifiés, nouveaux fichiers, dernière version des
    /// opérations retirées, copie brute de la spec, `source.yml`. Refuse, sans rien écrire, s'il reste un conflit sans
    /// choix, si un fichier à réécrire a changé depuis le plan ou n'est pas une requête HTTP.
    pub fn apply(&self, decisions: &Decisions) -> Result<Report, SyncError> {
        let root = &self.state.root;
        let writes = self.rewrites(decisions)?;
        let jobs: Vec<_> = writes
            .iter()
            .map(|(ours, text)| Ok((resolve_visible_path(root, &ours.file)?, text)))
            .collect::<Result<_, CoreError>>()?;
        write_parallel(&jobs, |(path, text)| write_atomic(path, text))?;

        let mut run = Run::new(root, self.state.claimed_files());
        run.report.written.extend(writes.iter().map(|(ours, _)| ours.file.clone()));
        let mut entries = self.entries(decisions, &mut run)?;
        run.creator.flush()?;

        let (mut report, adopted) = (run.report, run.adopted);
        let is_adopted = |file: &str| adopted.contains(&file.to_lowercase());
        entries.retain(|entry| !(entry.removed && entry.file.as_deref().is_some_and(is_adopted)));
        report.removed.retain(|file| !is_adopted(file));
        for item in &self.state.items {
            let kept = item.entry.as_ref().and_then(|entry| entry.file.as_deref()).is_some_and(|f| !is_adopted(f));
            if let (Fate::Listed(OpStatus::Removed), Some(base), true) = (&item.fate, &item.base, kept) {
                store::write_removed(root, &item.key, &base.text)?;
            }
        }
        store::write(root, &self.source, self.group_by, &entries, &self.state.spec_text)?;
        Ok(report)
    }

    /// Les fichiers de l'équipe à réécrire avec leur nouveau texte, après vérification des décisions et des fichiers.
    fn rewrites(&self, decisions: &Decisions) -> Result<Vec<(&Ours, String)>, SyncError> {
        let mut writes = Vec::new();
        let mut unresolved = Vec::new();
        for item in &self.state.items {
            let Some(merged) = item.merge(&decisions.choices)? else { continue };
            unresolved.extend(merged.unresolved().map(|change| change.id.clone()));
            if let (Some(ours), Some(spec)) = (&item.ours, &item.spec) {
                let text = render(ours, &merged.doc, spec)?;
                if text != ours.text {
                    writes.push((ours, text));
                }
            }
        }
        if !unresolved.is_empty() {
            return Err(SyncError::Unresolved { count: unresolved.len(), ids: unresolved.join(", ") });
        }
        for (ours, _) in &writes {
            if !ours.is_request {
                return Err(SyncError::NotARequest(ours.file.clone()));
            }
            let path = resolve_visible_path(&self.state.root, &ours.file)?;
            if fs::read_to_string(&path).ok().as_deref() != Some(ours.text.as_str()) {
                return Err(SyncError::Stale(ours.file.clone()));
            }
        }
        Ok(writes)
    }

    /// Les entrées de `source.yml` après la synchro, dans l'ordre du plan ; les nouvelles requêtes sont choisies
    /// (écrites par `flush`).
    fn entries(&self, decisions: &Decisions, run: &mut Run) -> Result<Vec<Entry>, SyncError> {
        let skip: HashSet<&str> = decisions.skip.iter().map(String::as_str).collect();
        let recreate: HashSet<&str> = decisions.recreate.iter().map(String::as_str).collect();
        let mut entries = Vec::new();
        for item in &self.state.items {
            let entry = match (&item.fate, &item.spec, &item.entry) {
                (Fate::Carry(entry), ..) => entry.clone(),
                (Fate::Listed(OpStatus::New), Some(spec), _) => run.entry(spec, !skip.contains(spec.key.as_str()))?,
                (Fate::Listed(OpStatus::Missing), Some(spec), _) => {
                    run.entry(spec, recreate.contains(spec.key.as_str()))?
                }
                (Fate::Listed(OpStatus::Removed), None, Some(entry)) => {
                    run.report.removed.extend(entry.file.clone());
                    Entry { removed: true, ..entry.clone() }
                }
                (Fate::Listed(_), Some(spec), _) => match &item.ours {
                    Some(ours) => Entry::tracked(&spec.key, &ours.file),
                    None => continue,
                },
                _ => continue,
            };
            entries.push(entry);
        }
        Ok(entries)
    }
}

/// Création des nouvelles requêtes d'une application : fichiers déjà suivis (`held`), fichiers d'opérations retirées
/// qu'une nouvelle opération peut reprendre (`adoptable`) et ceux qu'elle a repris (`adopted`).
struct Run<'a> {
    creator: Creator<'a>,
    report: Report,
    held: HashSet<String>,
    adoptable: HashSet<String>,
    adopted: HashSet<String>,
}

impl<'a> Run<'a> {
    fn new(root: &'a Path, (held, adoptable): (HashSet<String>, HashSet<String>)) -> Self {
        Self { creator: Creator::new(root), report: Report::default(), held, adoptable, adopted: HashSet::new() }
    }

    /// L'entrée d'une nouvelle opération (ou d'une manquante à recréer) : son fichier créé, ou `ignored` si `create`
    /// est faux. Un fichier existant, identique à ce que la synchro écrirait et suivi seulement par une opération
    /// retirée, est repris : c'est un rapprochement, l'ancienne entrée disparaît et rien n'est créé.
    fn entry(&mut self, spec: &Op, create: bool) -> Result<Entry, SyncError> {
        if !create {
            self.report.ignored.push(spec.key.clone());
            return Ok(Entry::ignored(&spec.key));
        }
        let claimed = self.creator.create(spec, &self.held)?;
        let id = claimed.file.to_lowercase();
        if claimed.reused && self.adoptable.contains(&id) {
            self.adopted.insert(id);
        } else {
            self.report.created.push(claimed.file.clone());
        }
        Ok(Entry::tracked(&spec.key, &claimed.file))
    }
}

/// Le fichier de l'équipe avec le document fusionné : seules les sections modifiées sont réécrites, le reste
/// (champs inconnus, scripts, mise en forme) est conservé.
pub(super) fn render(ours: &Ours, merged: &RequestDoc, spec: &Op) -> Result<String, CoreError> {
    if *merged == ours.doc {
        return Ok(ours.text.clone());
    }
    let mut tree = tree(&ours.text, &ours.file)?;
    merged.apply(&mut tree, &ours.doc);
    adopt_untyped(&mut tree, merged, &ours.doc, spec)?;
    Ok(emit(&Value::Map(tree), BLANK_BEFORE))
}

/// `RequestDoc::apply` n'écrit pas les auth et corps que le modèle ne type pas : ceux de la spec sont repris tels quels.
fn adopt_untyped(tree: &mut Map, merged: &RequestDoc, ours: &RequestDoc, spec: &Op) -> Result<(), CoreError> {
    let auth = matches!(merged.auth, Auth::Other { .. }) && merged.auth != ours.auth;
    let body = matches!(merged.body, Body::Other { .. }) && merged.body != ours.body;
    if !auth && !body {
        return Ok(());
    }
    let spec_tree = self::tree(&spec.text, &spec.key)?;
    let Some(spec_http) = spec_tree.map("http") else { return Ok(()) };
    let http = tree.map_mut_or_insert("http", TOP_ORDER);
    for key in [("auth", auth), ("body", body)].into_iter().filter(|(_, wanted)| *wanted).map(|(key, _)| key) {
        if let Some(value) = spec_http.get(key) {
            http.set(key, value.clone(), HTTP_ORDER);
        }
    }
    Ok(())
}
