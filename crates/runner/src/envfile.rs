//! `xc run --env-file` : un environnement lu dans un fichier choisi, hors de `environments/`.

use std::path::Path;

use serde_json::{Map, Value};
use xc_core::read_environment_file;

use crate::session::EnvWrites;

/// Les variables actives d'un fichier `.json` (`{ name?, variables: [{ name, value, enabled?, secret? }] }`, la forme
/// d'export de Bruno) ou `.yml` / `.yaml` (un environnement OpenCollection, `extends` laissé de côté), avec le nom de
/// l'environnement (`bru.getEnvName()`) : celui du fichier JSON s'il en a un, sinon le nom du fichier sans extension.
///
/// Le résultat est l'état d'environnement d'une session sans nom de fichier : le run ne cherche rien dans
/// `environments/`, les variables viennent toutes de ce fichier.
pub fn load_env_file(path: &Path) -> Result<EnvWrites, String> {
    let (name, mut vars) = read_variables(path)?;
    vars.insert(xc_script::ENV_NAME.into(), Value::String(name));
    Ok(EnvWrites { name: None, vars })
}

fn read_variables(path: &Path) -> Result<(String, Map<String, Value>), String> {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("environment").to_owned();
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    match extension.as_str() {
        "json" => {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("lecture de {} impossible : {e}", path.display()))?;
            let parsed: Value = serde_json::from_str(&text)
                .map_err(|e| format!("{} n'est pas du JSON valide : {e}", path.display()))?;
            let variables = parsed.get("variables").and_then(Value::as_array).ok_or_else(|| {
                format!("{} : un environnement attendu, objet avec une liste « variables »", path.display())
            })?;
            let mut vars = Map::new();
            for variable in variables.iter().filter(|v| v.get("enabled").and_then(Value::as_bool) != Some(false)) {
                if let Some(name) = variable.get("name").and_then(Value::as_str) {
                    vars.insert(
                        name.to_owned(),
                        variable.get("value").cloned().unwrap_or(Value::String(String::new())),
                    );
                }
            }
            let name =
                parsed.get("name").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty()).unwrap_or(&stem);
            Ok((name.to_owned(), vars))
        }
        "yml" | "yaml" => {
            let variables = read_environment_file(path).map_err(|e| e.to_string())?;
            let vars = variables
                .into_iter()
                .filter(|v| v.enabled)
                .filter_map(|v| v.value.map(|value| (v.name, Value::String(value))))
                .collect();
            Ok((stem, vars))
        }
        other => Err(format!(
            "{} : le format « .{other} » n'est pas pris en charge, utilise un fichier .json ou .yml",
            path.display()
        )),
    }
}
