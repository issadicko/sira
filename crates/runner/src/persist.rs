//! `xc run --persist-vars` : écrire dans les fichiers les variables que les scripts ont posées, avec les règles de
//! fusion de `bru run` (`persist-variables.js`) : une variable présente reçoit sa nouvelle valeur, une variable neuve
//! s'ajoute en fin de liste, une variable désactivée reste telle quelle, une variable active que le script a retirée
//! (`bru.deleteEnvVar`) disparaît du fichier. Deux garde-fous de plus que Bruno : une ligne de secret n'est jamais
//! retirée (un trousseau absent ne la rend pas au script), et sa valeur n'est jamais écrite.

use std::path::Path;

use serde_json::{Map, Value};
use xc_core::{read_environment, save_collection_variables, save_environment, CollectionVar, EnvVar};

use crate::session::Session;

/// Ce que `persist_variables` a écrit.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Persisted {
    /// Le fichier d'environnement réécrit (`environments/dev.yml`).
    pub environment: Option<String>,
    /// `opencollection.yml` a été réécrit.
    pub collection: bool,
}

/// Le texte de la valeur : un objet ou une liste s'écrit en JSON indenté de deux espaces, comme Bruno.
fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        Value::Object(_) | Value::Array(_) => serde_json::to_string_pretty(value).unwrap_or_default(),
        other => other.to_string(),
    }
}

/// Le type que le fichier garde pour la valeur : le texte est le type par défaut et ne s'écrit pas.
fn data_type(value: &Value) -> Option<String> {
    match value {
        Value::Number(_) => Some("number".into()),
        Value::Bool(_) => Some("boolean".into()),
        Value::Object(_) | Value::Array(_) => Some("object".into()),
        Value::String(_) | Value::Null => None,
    }
}

fn merge_environment(existing: Vec<EnvVar>, script: &Map<String, Value>) -> Vec<EnvVar> {
    let mut next: Vec<EnvVar> = existing
        .into_iter()
        .filter(|v| !v.enabled || v.secret || script.contains_key(&v.name))
        .map(|v| match (v.enabled, v.secret, script.get(&v.name)) {
            (true, false, Some(value)) => EnvVar { value: Some(text(value)), data_type: data_type(value), ..v },
            _ => v,
        })
        .collect();
    let present: Vec<String> = next.iter().filter(|v| v.enabled).map(|v| v.name.clone()).collect();
    let disabled_secrets: Vec<String> =
        next.iter().filter(|v| !v.enabled && v.secret).map(|v| v.name.clone()).collect();
    for (name, value) in script {
        if present.contains(name) || disabled_secrets.contains(name) {
            continue;
        }
        next.push(EnvVar {
            name: name.clone(),
            value: Some(text(value)),
            secret: false,
            enabled: true,
            description: None,
            data_type: data_type(value),
        });
    }
    next
}

fn merge_collection(existing: Vec<CollectionVar>, script: &Map<String, Value>) -> Vec<CollectionVar> {
    let mut next: Vec<CollectionVar> = existing
        .into_iter()
        .filter(|v| !v.enabled || script.contains_key(&v.name))
        .map(|v| match (v.enabled, script.get(&v.name)) {
            (true, Some(value)) => CollectionVar { value: text(value), data_type: data_type(value), ..v },
            _ => v,
        })
        .collect();
    let present: Vec<String> = next.iter().filter(|v| v.enabled).map(|v| v.name.clone()).collect();
    for (name, value) in script {
        if !present.contains(name) {
            next.push(CollectionVar {
                name: name.clone(),
                value: text(value),
                data_type: data_type(value),
                enabled: true,
                description: None,
            });
        }
    }
    next
}

/// Écrit les variables d'environnement et de collection que les scripts de `session` ont posées. L'environnement n'est
/// écrit que s'il a un fichier dans `environments/` (`env`) et que sa variable `__name__` n'est pas une variable ; les
/// variables runtime et globales ne sont jamais écrites. Rien n'est lu ni écrit pour ce qu'aucun script n'a touché.
pub fn persist_variables(root: &Path, session: &Session, env: Option<&str>) -> Result<Persisted, String> {
    let mut done = Persisted::default();
    if let (Some(name), Some(writes)) = (env, session.env.as_ref().filter(|w| w.name.as_deref() == env)) {
        let mut script = writes.vars.clone();
        script.shift_remove(xc_script::ENV_NAME);
        let existing = read_environment(root, name).map_err(|e| e.to_string())?;
        let next = merge_environment(existing, &script);
        if save_environment(root, name, &next, false).map_err(|e| e.to_string())? {
            done.environment = Some(format!("environments/{name}.yml"));
        }
    }
    if let Some(script) = &session.collection {
        let existing = xc_core::read_collection_variables(root).map_err(|e| e.to_string())?;
        let next = merge_collection(existing, script);
        done.collection = save_collection_variables(root, &next).map_err(|e| e.to_string())?;
    }
    Ok(done)
}
