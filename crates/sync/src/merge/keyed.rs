//! Collections à clé (paramètres, en-têtes, champs de formulaire) : existence et description à la spec, valeur et
//! activation à l'équipe (`docs/docs/synchro-openapi.md` § 5).

use std::collections::HashMap;

use xc_core::{KeyValue, MultipartField, MultipartValue, Param, ParamKind};

use super::{verdict, Choice, Cx, Field, Kind, MergeError, Verdict};

/// Clé de rapprochement d'un élément, identifiant et libellé de son changement.
pub(super) struct Ident {
    pub key: String,
    pub id: String,
    pub label: String,
}

pub(super) struct Group<'a, E> {
    pub field: Field,
    pub ident: fn(&E) -> Ident,
    /// Élément dont la spec n'a retiré l'existence qu'en retirant son segment d'adresse : jamais un conflit.
    pub implied_removal: &'a dyn Fn(&E) -> bool,
}

impl<E> Group<'static, E> {
    pub fn plain(field: Field, ident: fn(&E) -> Ident) -> Self {
        Self { field, ident, implied_removal: &|_| false }
    }
}

pub(super) trait Entry: Clone + PartialEq {
    fn show(&self) -> String;
    /// La valeur seule, que l'interface préremplit quand le conflit propose de la saisir (`edit`).
    fn raw(&self) -> String;
    fn description(&self) -> Option<&str>;
    fn same_value(&self, other: &Self) -> bool;
    fn take_value(&mut self, other: &Self);
    fn set_description(&mut self, description: Option<String>);
    fn edited(&self, value: &str) -> Self;
}

fn display(value: &str, enabled: bool, description: Option<&str>) -> String {
    let mut text = value.to_owned();
    if !enabled {
        text.push_str(" (désactivé)");
    }
    if let Some(description) = description.filter(|d| !d.is_empty()) {
        text.push_str(" — ");
        text.push_str(description);
    }
    text
}

impl Entry for Param {
    fn show(&self) -> String {
        display(&self.value, self.enabled, self.description.as_deref())
    }

    fn raw(&self) -> String {
        self.value.clone()
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn same_value(&self, other: &Self) -> bool {
        self.value == other.value && self.enabled == other.enabled
    }

    fn take_value(&mut self, other: &Self) {
        self.value.clone_from(&other.value);
        self.enabled = other.enabled;
    }

    fn set_description(&mut self, description: Option<String>) {
        self.description = description;
    }

    fn edited(&self, value: &str) -> Self {
        Self { value: value.to_owned(), ..self.clone() }
    }
}

impl Entry for KeyValue {
    fn show(&self) -> String {
        display(&self.value, self.enabled, self.description.as_deref())
    }

    fn raw(&self) -> String {
        self.value.clone()
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn same_value(&self, other: &Self) -> bool {
        self.value == other.value && self.enabled == other.enabled
    }

    fn take_value(&mut self, other: &Self) {
        self.value.clone_from(&other.value);
        self.enabled = other.enabled;
    }

    fn set_description(&mut self, description: Option<String>) {
        self.description = description;
    }

    fn edited(&self, value: &str) -> Self {
        Self { value: value.to_owned(), ..self.clone() }
    }
}

impl Entry for MultipartField {
    fn show(&self) -> String {
        let value = match &self.value {
            MultipartValue::Text(_) => self.raw(),
            MultipartValue::File(_) => format!("fichier : {}", self.raw()),
        };
        display(&value, self.enabled, self.description.as_deref())
    }

    fn raw(&self) -> String {
        match &self.value {
            MultipartValue::Text(text) => text.clone(),
            MultipartValue::File(paths) => paths.join(", "),
        }
    }

    fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }

    fn same_value(&self, other: &Self) -> bool {
        self.value == other.value && self.content_type == other.content_type && self.enabled == other.enabled
    }

    fn take_value(&mut self, other: &Self) {
        self.value.clone_from(&other.value);
        self.content_type.clone_from(&other.content_type);
        self.enabled = other.enabled;
    }

    fn set_description(&mut self, description: Option<String>) {
        self.description = description;
    }

    fn edited(&self, value: &str) -> Self {
        Self { value: MultipartValue::Text(value.to_owned()), ..self.clone() }
    }
}

pub(super) fn param_ident(param: &Param) -> Ident {
    let (kind, label) = match param.kind {
        ParamKind::Query => ("query", "Paramètre query"),
        ParamKind::Path => ("path", "Paramètre de chemin"),
    };
    Ident {
        key: format!("{kind}/{}", param.name),
        id: format!("param/{kind}/{}", param.name),
        label: format!("{label} « {} »", param.name),
    }
}

pub(super) fn header_ident(header: &KeyValue) -> Ident {
    let name = header.name.to_lowercase();
    Ident { id: format!("header/{name}"), label: format!("En-tête « {} »", header.name), key: name }
}

pub(super) fn form_key_value_ident(field: &KeyValue) -> Ident {
    form_ident(&field.name)
}

pub(super) fn form_multipart_ident(field: &MultipartField) -> Ident {
    form_ident(&field.name)
}

fn form_ident(name: &str) -> Ident {
    Ident { key: name.to_owned(), id: format!("body/form/{name}"), label: format!("Champ de formulaire « {name} »") }
}

struct Index<'a, E> {
    list: &'a [E],
    first: HashMap<String, usize>,
}

impl<'a, E> Index<'a, E> {
    fn new(list: &'a [E], ident: fn(&E) -> Ident) -> Self {
        let mut first = HashMap::new();
        for (i, entry) in list.iter().enumerate() {
            first.entry(ident(entry).key).or_insert(i);
        }
        Self { list, first }
    }

    fn get(&self, key: &str) -> Option<&'a E> {
        self.first.get(key).map(|&i| &self.list[i])
    }

    fn is_first(&self, key: &str, position: usize) -> bool {
        self.first.get(key) == Some(&position)
    }
}

/// Fusionne les trois listes ; l'ordre est celui de l'équipe, puis les ajouts de la spec dans son ordre. `base` est
/// `None` quand il n'y a pas de base : les éléments de l'équipe sont gardés et ceux de la spec ajoutés.
pub(super) fn merge<E: Entry>(
    cx: &mut Cx,
    group: &Group<E>,
    base: Option<&[E]>,
    ours: &[E],
    theirs: &[E],
) -> Result<Vec<E>, MergeError> {
    let (b, o, t) = (
        base.map(|list| Index::new(list, group.ident)),
        Index::new(ours, group.ident),
        Index::new(theirs, group.ident),
    );
    let mut result = Vec::with_capacity(ours.len());
    let mut added: Vec<(usize, E)> = Vec::new();

    for (i, own) in ours.iter().enumerate() {
        let ident = (group.ident)(own);
        if !o.is_first(&ident.key, i) {
            result.push(own.clone());
            continue;
        }
        let before = b.as_ref().and_then(|b| b.get(&ident.key));
        let kept = match (t.get(&ident.key), before) {
            (Some(spec), before) => Some(present(cx, group, &ident, before, own, spec, b.is_some())?),
            (None, Some(before)) => retired_by_spec(cx, group, &ident, before, own)?,
            (None, None) => Some(own.clone()),
        };
        result.extend(kept);
    }

    if let Some(b) = &b {
        for (j, before) in b.list.iter().enumerate() {
            let ident = (group.ident)(before);
            if !b.is_first(&ident.key, j) || o.first.contains_key(&ident.key) {
                continue;
            }
            let spec = t.get(&ident.key);
            let readded = retired_by_team(cx, group, &ident, before, spec)?;
            if let (Some(element), Some(&position)) = (readded, t.first.get(&ident.key)) {
                added.push((position, element));
            }
        }
    }

    for (i, spec) in theirs.iter().enumerate() {
        let ident = (group.ident)(spec);
        let known = o.first.contains_key(&ident.key) || b.as_ref().is_some_and(|b| b.first.contains_key(&ident.key));
        if !t.is_first(&ident.key, i) || known {
            continue;
        }
        let mut change = cx.change(&ident.id, group.field, &ident.label, Kind::Applied, "Ajouté par la spec");
        change.theirs = Some(spec.show());
        change.result = Some(spec.show());
        cx.changes.push(change);
        added.push((i, spec.clone()));
    }

    added.sort_by_key(|(position, _)| *position);
    result.extend(added.into_iter().map(|(_, element)| element));
    Ok(result)
}

const KEPT_BY_TEAM: &str = "Modifié par l'équipe seulement : valeur de l'équipe conservée";

fn present<E: Entry>(
    cx: &mut Cx,
    group: &Group<E>,
    ident: &Ident,
    before: Option<&E>,
    own: &E,
    spec: &E,
    has_base: bool,
) -> Result<E, MergeError> {
    let Some(before) = before else {
        let (kind, reason) = match (own == spec, has_base) {
            (true, true) => (Kind::Same, "Ajouté des deux côtés, à l'identique"),
            (true, false) => return Ok(own.clone()),
            (false, true) => (Kind::Kept, "Ajouté des deux côtés : élément de l'équipe conservé"),
            (false, false) => (Kind::Kept, "Sans base : élément de l'équipe conservé"),
        };
        let mut change = cx.change(&ident.id, group.field, &ident.label, kind, reason);
        change.ours = Some(own.show());
        change.theirs = Some(spec.show());
        change.result = Some(own.show());
        cx.changes.push(change);
        return Ok(own.clone());
    };

    let value = if own.same_value(before) {
        if spec.same_value(before) {
            Verdict::Nothing
        } else {
            Verdict::Applied
        }
    } else if spec.same_value(before) {
        Verdict::Kept
    } else if own.same_value(spec) {
        Verdict::Same
    } else {
        Verdict::Kept
    };
    let description = verdict(Some(&before.description()), &own.description(), &spec.description());
    let kinds = [value, description];
    let has = |wanted: Verdict| kinds.contains(&wanted);
    let (kind, reason) = match (has(Verdict::Conflict), has(Verdict::Applied), has(Verdict::Kept), has(Verdict::Same)) {
        (true, ..) => (Kind::Conflict, "Description modifiée à la fois par l'équipe et par la spec"),
        (_, true, true, _) => (Kind::Merged, "Modifié des deux côtés : fusion automatique"),
        (_, true, ..) => (Kind::Applied, "Modifié par la spec seulement : valeur de la spec appliquée"),
        (_, _, true, _) => (Kind::Kept, KEPT_BY_TEAM),
        (_, _, _, true) => (Kind::Same, "Même modification dans l'équipe et dans la spec"),
        _ => return Ok(own.clone()),
    };
    let mut change = cx.change(&ident.id, group.field, &ident.label, kind, reason);
    (change.base, change.ours, change.theirs) = (Some(before.show()), Some(own.show()), Some(spec.show()));

    let mut result = own.clone();
    if value == Verdict::Applied {
        result.take_value(spec);
    }
    match description {
        Verdict::Applied => result.set_description(spec.description().map(str::to_owned)),
        Verdict::Conflict => {
            change.choices = vec![Choice::Team, Choice::Spec];
            if let Some(decision) = cx.decide(&mut change)? {
                if decision.choice == Choice::Spec {
                    result.set_description(spec.description().map(str::to_owned));
                }
            }
        }
        _ => {}
    }
    change.result = (kind != Kind::Conflict || change.decided).then(|| result.show());
    cx.changes.push(change);
    Ok(result)
}

fn retired_by_spec<E: Entry>(
    cx: &mut Cx,
    group: &Group<E>,
    ident: &Ident,
    before: &E,
    own: &E,
) -> Result<Option<E>, MergeError> {
    if (group.implied_removal)(own) {
        return Ok(Some(own.clone()));
    }
    let untouched = own == before;
    let (kind, reason) = if untouched {
        (Kind::Applied, "Retiré par la spec, non modifié par l'équipe")
    } else {
        (Kind::Conflict, "Retiré par la spec, mais modifié par l'équipe")
    };
    let mut change = cx.change(&ident.id, group.field, &ident.label, kind, reason);
    if untouched {
        (change.base, change.ours) = (Some(before.show()), Some(own.show()));
        cx.changes.push(change);
        return Ok(None);
    }
    (change.base, change.ours) = (Some(before.raw()), Some(own.raw()));
    change.choices = vec![Choice::Team, Choice::Spec, Choice::Edit];
    let kept = match cx.decide(&mut change)? {
        None => Some(own.clone()),
        Some(decision) => match (decision.choice, decision.value) {
            (Choice::Spec, _) => None,
            (Choice::Edit, Some(value)) => Some(own.edited(&value)),
            _ => Some(own.clone()),
        },
    };
    change.result = kept.as_ref().filter(|_| change.decided).map(Entry::show);
    cx.changes.push(change);
    Ok(kept)
}

/// Élément que l'équipe a retiré : renvoie celui de la spec à rajouter si le conflit est tranché ainsi.
fn retired_by_team<E: Entry>(
    cx: &mut Cx,
    group: &Group<E>,
    ident: &Ident,
    before: &E,
    spec: Option<&E>,
) -> Result<Option<E>, MergeError> {
    let (kind, reason) = match spec {
        None => (Kind::Same, "Retiré des deux côtés"),
        Some(spec) if spec == before => (Kind::Kept, "Retiré par l'équipe, inchangé dans la spec : reste retiré"),
        Some(_) => (Kind::Conflict, "Retiré par l'équipe, mais modifié par la spec"),
    };
    let mut change = cx.change(&ident.id, group.field, &ident.label, kind, reason);
    let Some(spec) = spec.filter(|_| kind == Kind::Conflict) else {
        (change.base, change.theirs) = (Some(before.show()), spec.map(Entry::show));
        cx.changes.push(change);
        return Ok(None);
    };
    (change.base, change.theirs) = (Some(before.raw()), Some(spec.raw()));
    change.choices = vec![Choice::Team, Choice::Spec, Choice::Edit];
    let readded = match cx.decide(&mut change)? {
        None => None,
        Some(decision) => match (decision.choice, decision.value) {
            (Choice::Spec, _) => Some(spec.clone()),
            (Choice::Edit, Some(value)) => Some(spec.edited(&value)),
            _ => None,
        },
    };
    change.result = readded.as_ref().map(Entry::show);
    cx.changes.push(change);
    Ok(readded)
}
