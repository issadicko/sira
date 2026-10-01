//! Application d'un plan (§ 6) : fichiers modifiés, nouveaux fichiers, copie brute de la spec, puis `source.yml`.

use std::collections::HashSet;
use std::fs;

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
    /// Écrit le plan selon les décisions, dans cet ordre : fichiers modifiés, nouveaux fichiers, copie brute de la
    /// spec, `source.yml`. Refuse, sans rien écrire, s'il reste un conflit sans choix ou si un fichier à réécrire a
    /// changé depuis le plan.
    pub fn apply(&self, decisions: &Decisions) -> Result<Report, SyncError> {
        let root = &self.state.root;
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
            let path = resolve_visible_path(root, &ours.file)?;
            if fs::read_to_string(&path).ok().as_deref() != Some(ours.text.as_str()) {
                return Err(SyncError::Stale(ours.file.clone()));
            }
        }

        let mut report = Report::default();
        let jobs: Vec<_> = writes
            .iter()
            .map(|(ours, text)| Ok((resolve_visible_path(root, &ours.file)?, text)))
            .collect::<Result<_, CoreError>>()?;
        write_parallel(&jobs, |(path, text)| write_atomic(path, text))?;
        report.written.extend(writes.iter().map(|(ours, _)| ours.file.clone()));

        let skip: HashSet<&str> = decisions.skip.iter().map(String::as_str).collect();
        let recreate: HashSet<&str> = decisions.recreate.iter().map(String::as_str).collect();
        let mut creator = Creator::new(root);
        let mut entries = Vec::new();
        for item in &self.state.items {
            let entry = match (&item.fate, &item.spec, &item.entry) {
                (Fate::Carry(entry), ..) => entry.clone(),
                (Fate::Listed(OpStatus::New), Some(spec), _) => {
                    created_or_ignored(&mut creator, &mut report, spec, !skip.contains(spec.key.as_str()))?
                }
                (Fate::Listed(OpStatus::Missing), Some(spec), _) => {
                    created_or_ignored(&mut creator, &mut report, spec, recreate.contains(spec.key.as_str()))?
                }
                (Fate::Listed(OpStatus::Removed), None, Some(entry)) => {
                    report.removed.extend(entry.file.clone());
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
        creator.flush()?;

        store::write(root, &self.source, self.group_by, &entries, &self.state.spec_text)?;
        Ok(report)
    }
}

fn created_or_ignored(creator: &mut Creator, report: &mut Report, spec: &Op, create: bool) -> Result<Entry, SyncError> {
    if !create {
        report.ignored.push(spec.key.clone());
        return Ok(Entry::ignored(&spec.key));
    }
    let file = creator.create(spec)?;
    report.created.push(file.clone());
    Ok(Entry::tracked(&spec.key, &file))
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
