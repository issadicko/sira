//! Fusion à 3 voies d'une requête (base, équipe, spec), champ par champ (`docs/docs/synchro-openapi.md` § 5).
//!
//! Le moteur est pur : il ne touche pas au disque. Il rend le document fusionné et la liste des changements ; un
//! conflit n'est jamais tranché seul, la valeur de l'équipe reste en place tant qu'aucun choix n'est donné.

mod body;
mod json;
mod keyed;
mod url;

use std::borrow::Cow;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use xc_core::{Auth, Param, ParamKind, RequestDoc};

use keyed::{header_ident, param_ident, Entry, Group};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Field {
    Method,
    Url,
    Param,
    Header,
    Body,
    Auth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Applied,
    Kept,
    Same,
    Merged,
    Conflict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Choice {
    Team,
    Spec,
    Both,
    Edit,
}

impl Choice {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Spec => "spec",
            Self::Both => "both",
            Self::Edit => "edit",
        }
    }
}

/// Choix de l'utilisateur pour un conflit ; `value` est la valeur saisie du choix `edit`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Decision {
    pub choice: Choice,
    #[serde(default)]
    pub value: Option<String>,
}

/// Choix par identifiant de changement.
pub type Choices = HashMap<String, Decision>;

/// Un changement entre les trois versions d'un champ. `result` est `None` pour un conflit sans choix et pour un
/// élément absent du résultat ; `choices` n'est rempli que pour un conflit.
#[derive(Debug, Clone, Serialize)]
pub struct Change {
    pub id: String,
    pub field: Field,
    pub label: String,
    pub reason: String,
    pub kind: Kind,
    pub base: Option<String>,
    pub ours: Option<String>,
    pub theirs: Option<String>,
    pub result: Option<String>,
    pub choices: Vec<Choice>,
    #[serde(skip)]
    pub decided: bool,
}

#[derive(Debug, Clone)]
pub struct Merged {
    pub doc: RequestDoc,
    pub changes: Vec<Change>,
}

impl Merged {
    /// Conflits qui attendent encore un choix.
    pub fn unresolved(&self) -> impl Iterator<Item = &Change> {
        self.changes.iter().filter(|c| c.kind == Kind::Conflict && !c.decided)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    #[error("{id} : le choix « {choice} » n'est pas proposé pour ce conflit")]
    Unavailable { id: String, choice: &'static str },
    #[error("{id} : la valeur à saisir est manquante")]
    MissingValue { id: String },
}

struct Cx<'a> {
    key: &'a str,
    choices: &'a Choices,
    changes: Vec<Change>,
}

impl Cx<'_> {
    fn change(&self, suffix: &str, field: Field, label: &str, kind: Kind, reason: &str) -> Change {
        Change {
            id: format!("{}::{suffix}", self.key),
            field,
            label: label.to_owned(),
            reason: reason.to_owned(),
            kind,
            base: None,
            ours: None,
            theirs: None,
            result: None,
            choices: Vec::new(),
            decided: false,
        }
    }

    fn decide(&self, change: &mut Change) -> Result<Option<Decision>, MergeError> {
        let Some(decision) = self.choices.get(&change.id) else { return Ok(None) };
        if !change.choices.contains(&decision.choice) {
            return Err(MergeError::Unavailable { id: change.id.clone(), choice: decision.choice.as_str() });
        }
        if decision.choice == Choice::Edit && decision.value.is_none() {
            return Err(MergeError::MissingValue { id: change.id.clone() });
        }
        change.decided = true;
        Ok(Some(decision.clone()))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Nothing,
    Applied,
    Kept,
    Same,
    Conflict,
}

/// Fusion 3 voies d'une valeur ; sans base, deux valeurs différentes sont en conflit.
fn verdict<T: PartialEq + ?Sized>(base: Option<&T>, ours: &T, theirs: &T) -> Verdict {
    if ours == theirs {
        return if base.is_some_and(|b| b != ours) { Verdict::Same } else { Verdict::Nothing };
    }
    match base {
        Some(b) if ours == b => Verdict::Applied,
        Some(b) if theirs == b => Verdict::Kept,
        _ => Verdict::Conflict,
    }
}

fn describe(verdict: Verdict, has_base: bool) -> (Kind, &'static str) {
    match verdict {
        Verdict::Applied => (Kind::Applied, "Modifié par la spec seulement : valeur de la spec appliquée"),
        Verdict::Kept => (Kind::Kept, "Modifié par l'équipe seulement : valeur de l'équipe conservée"),
        Verdict::Same => (Kind::Same, "Même modification dans l'équipe et dans la spec"),
        _ if has_base => (Kind::Conflict, "Modifié à la fois par l'équipe et par la spec"),
        _ => (Kind::Conflict, "Valeurs différentes et aucune base pour trancher"),
    }
}

struct Slot<'a, T> {
    field: Field,
    suffix: &'a str,
    label: &'a str,
    show: fn(&T) -> Option<String>,
    edit: Option<fn(&T, &str) -> T>,
}

/// Résultats que le moteur du champ a déjà calculés : une fusion propre, et la combinaison où l'équipe gagne.
struct Extra<T> {
    merged: Option<T>,
    both: Option<T>,
}

impl<T> Default for Extra<T> {
    fn default() -> Self {
        Self { merged: None, both: None }
    }
}

fn settle<T: Clone + PartialEq>(
    cx: &mut Cx,
    slot: &Slot<T>,
    base: Option<&T>,
    ours: &T,
    theirs: &T,
    extra: Extra<T>,
) -> Result<T, MergeError> {
    let found = verdict(base, ours, theirs);
    if found == Verdict::Nothing {
        return Ok(ours.clone());
    }
    let clean = extra.merged.filter(|_| found == Verdict::Conflict);
    let (kind, reason) = match clean {
        Some(_) => (Kind::Merged, "Modifié des deux côtés : fusion automatique"),
        None => describe(found, base.is_some()),
    };
    let mut change = cx.change(slot.suffix, slot.field, slot.label, kind, reason);
    change.base = base.and_then(|b| (slot.show)(b));
    change.ours = (slot.show)(ours);
    change.theirs = (slot.show)(theirs);
    let value = match (clean, found) {
        (Some(merged), _) => merged,
        (None, Verdict::Applied) => theirs.clone(),
        (None, Verdict::Conflict) => {
            change.choices = vec![Choice::Team, Choice::Spec];
            change.choices.extend(extra.both.as_ref().map(|_| Choice::Both));
            change.choices.extend(slot.edit.map(|_| Choice::Edit));
            match cx.decide(&mut change)? {
                None => ours.clone(),
                Some(decision) => match (decision.choice, decision.value, slot.edit, extra.both) {
                    (Choice::Spec, ..) => theirs.clone(),
                    (Choice::Both, _, _, Some(both)) => both,
                    (Choice::Edit, Some(value), Some(edit), _) => edit(ours, &value),
                    _ => ours.clone(),
                },
            }
        }
        (None, _) => ours.clone(),
    };
    change.result = (kind != Kind::Conflict || change.decided).then(|| (slot.show)(&value)).flatten();
    cx.changes.push(change);
    Ok(value)
}

/// Un type que le modèle ne détaille pas : son nom, suivi de sa configuration canonique.
fn show_other(label: &str, config: &str) -> String {
    if config.is_empty() {
        label.to_owned()
    } else {
        format!("{label}\n{config}")
    }
}

fn show_auth(auth: &Auth) -> Option<String> {
    Some(match auth {
        Auth::Inherit => "inherit".into(),
        Auth::None => "none".into(),
        Auth::Bearer { token } => format!("bearer : {token}"),
        Auth::Basic { username, .. } => format!("basic : {username}"),
        Auth::Apikey { key, placement, .. } => format!("apikey : {key} ({placement})"),
        Auth::Digest { username, .. } => format!("digest : {username}"),
        Auth::Awsv4 { access_key_id, region, service, .. } => format!("awsv4 : {access_key_id} ({service} {region})"),
        Auth::Oauth2(config) => show_other("oauth2", &xc_core::oauth2::canonical(config)),
        Auth::Other { label, config } => show_other(label, config),
    })
}

/// Fusionne `ours` (fichier de l'équipe) et `theirs` (spec) à partir de `base`, ou sans base (`None`).
/// Seuls la méthode, l'adresse, les paramètres, les en-têtes, le corps et l'auth sont touchés ; `key` préfixe les
/// identifiants des changements.
pub fn merge(
    key: &str,
    base: Option<&RequestDoc>,
    ours: &RequestDoc,
    theirs: &RequestDoc,
    choices: &Choices,
) -> Result<Merged, MergeError> {
    let mut cx = Cx { key, choices, changes: Vec::new() };
    let mut doc = ours.clone();

    let method = Slot {
        field: Field::Method,
        suffix: "method",
        label: "Méthode",
        show: |m: &String| Some(m.clone()),
        edit: None,
    };
    doc.method = settle(&mut cx, &method, base.map(|d| &d.method), &ours.method, &theirs.method, Extra::default())?;

    let own_address = url::address(&ours.url).to_owned();
    let spec_address = url::address(&theirs.url).to_owned();
    let base_address = base.map(|d| url::address(&d.url).to_owned());
    let address_slot = Slot {
        field: Field::Url,
        suffix: "url",
        label: "Adresse",
        show: |a: &String| Some(a.clone()),
        edit: Some(|_: &String, value: &str| url::address(value).to_owned()),
    };
    let address = settle(&mut cx, &address_slot, base_address.as_ref(), &own_address, &spec_address, Extra::default())?;

    let headers = Group::plain(Field::Header, header_ident);
    doc.headers = keyed::merge(&mut cx, &headers, base.map(|d| d.headers.as_slice()), &ours.headers, &theirs.headers)?;

    let paths = PathNames::new(&own_address, &address);
    let own_params = paths.rename(&ours.params);
    let base_params = base.map(|d| paths.rename(&d.params));
    let spec_paths = url::path_names(&spec_address);
    let implied = |p: &Param| p.kind == ParamKind::Path && !spec_paths.contains(&p.name);
    let params = Group { field: Field::Param, ident: param_ident, implied_removal: &implied };
    let merged = keyed::merge(&mut cx, &params, base_params.as_deref(), &own_params, &theirs.params)?;
    let merged = follow_address(&mut cx, merged, &paths, base_params.as_deref(), &own_params, &theirs.params)?;
    doc.url = if url::query_view(&merged) == url::query_view(&ours.params) {
        format!("{address}{}", url::query_suffix(&ours.url))
    } else {
        url::with_query(&address, &ours.url, &ours.params, &merged)
    };
    doc.params = merged;

    doc.body = body::merge(&mut cx, base.map(|d| &d.body), &ours.body, &theirs.body)?;

    let auth = Slot { field: Field::Auth, suffix: "auth", label: "Authentification", show: show_auth, edit: None };
    doc.auth = settle(&mut cx, &auth, base.map(|d| &d.auth), &ours.auth, &theirs.auth, Extra::default())?;

    Ok(Merged { doc, changes: cx.changes })
}

/// Noms des segments `:nom` de l'adresse de l'équipe (`old`) et de l'adresse fusionnée (`new`), et le segment
/// que la spec a renommé s'il est le seul à avoir changé, à la même position.
struct PathNames {
    old: Vec<String>,
    new: Vec<String>,
    renamed: Option<(String, String)>,
}

impl PathNames {
    fn new(own_address: &str, address: &str) -> Self {
        let (old, new) = (url::path_names(own_address), url::path_names(address));
        let differing: Vec<_> = old.iter().zip(&new).filter(|(a, b)| a != b).collect();
        let renamed = match differing.as_slice() {
            [(from, to)] if old.len() == new.len() => Some(((*from).clone(), (*to).clone())),
            _ => None,
        };
        let old = old
            .into_iter()
            .map(|name| renamed.as_ref().filter(|(from, _)| *from == name).map_or(name, |(_, to)| to.clone()))
            .collect();
        Self { old, new, renamed }
    }

    /// Les paramètres de chemin, dont celui du segment renommé suit son nouveau nom : sa valeur n'est pas perdue.
    fn rename<'a>(&self, params: &'a [Param]) -> Cow<'a, [Param]> {
        let Some((from, to)) = &self.renamed else { return Cow::Borrowed(params) };
        let named = |p: &Param, name: &str| p.kind == ParamKind::Path && p.name == name;
        if params.iter().any(|p| named(p, to)) || !params.iter().any(|p| named(p, from)) {
            return Cow::Borrowed(params);
        }
        let rename = |mut p: Param| {
            if named(&p, from) {
                p.name.clone_from(to);
            }
            p
        };
        Cow::Owned(params.iter().cloned().map(rename).collect())
    }
}

/// Les paramètres de chemin suivent les segments `:nom` de l'adresse fusionnée, leurs valeurs reprises par nom :
/// ceux de l'ancienne adresse disparaissent avec leur segment, ceux de la nouvelle sont créés au besoin. Un paramètre
/// de chemin ajouté par l'équipe sans segment correspondant n'est pas touché. Celui dont le segment disparaît alors
/// que l'équipe en a saisi la valeur est un conflit : sa valeur n'est jamais perdue en silence.
fn follow_address(
    cx: &mut Cx,
    params: Vec<Param>,
    paths: &PathNames,
    base: Option<&[Param]>,
    ours: &[Param],
    theirs: &[Param],
) -> Result<Vec<Param>, MergeError> {
    let is_path = |p: &Param| p.kind == ParamKind::Path;
    let team_junk = |p: &Param| ours.iter().any(|o| is_path(o) && o.name == p.name) && !paths.old.contains(&p.name);
    let mut kept = Vec::with_capacity(params.len());
    for param in params {
        if !is_path(&param) || paths.new.contains(&param.name) || team_junk(&param) || retire(cx, &param, base)? {
            kept.push(param);
        }
    }
    if paths.new == paths.old {
        return Ok(kept);
    }
    for name in &paths.new {
        if kept.iter().any(|p| is_path(p) && &p.name == name) {
            continue;
        }
        let known = |list: &[Param]| list.iter().find(|p| is_path(p) && &p.name == name).cloned();
        let param = known(ours).or_else(|| known(theirs));
        kept.push(param.unwrap_or_else(|| Param {
            name: name.clone(),
            value: String::new(),
            kind: ParamKind::Path,
            enabled: true,
            description: None,
        }));
    }
    Ok(kept)
}

/// Le paramètre de chemin dont l'adresse n'a plus le segment : `true` s'il reste, c'est-à-dire si l'équipe en avait
/// saisi la valeur et ne l'abandonne pas ; sans choix, la valeur de l'équipe reste en place.
fn retire(cx: &mut Cx, param: &Param, base: Option<&[Param]>) -> Result<bool, MergeError> {
    let before = base.and_then(|list| list.iter().find(|b| b.kind == ParamKind::Path && b.name == param.name));
    let typed = !param.value.trim().is_empty() && before.is_none_or(|b| !param.same_value(b));
    if !typed {
        return Ok(false);
    }
    let ident = param_ident(param);
    let reason = "Segment retiré de l'adresse par la spec, mais valeur saisie par l'équipe";
    let mut change = cx.change(&ident.id, Field::Param, &ident.label, Kind::Conflict, reason);
    (change.base, change.ours) = (before.map(Entry::show), Some(param.show()));
    change.choices = vec![Choice::Team, Choice::Spec];
    let kept = cx.decide(&mut change)?.is_none_or(|decision| decision.choice == Choice::Team);
    change.result = (change.decided && kept).then(|| param.show());
    cx.changes.push(change);
    Ok(kept)
}
