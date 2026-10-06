use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::collection::{read_environment, read_tree, resolve_path, COLLECTION_FILE, FOLDER_FILE};
use crate::request::{key_values, RequestDoc};
use crate::yaml::Map;
use crate::CoreError;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Rung {
    pub level: String,
    pub source: String,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VariableInfo {
    pub name: String,
    pub value: Option<String>,
    pub level: Option<String>,
    pub secret: bool,
    pub rungs: Vec<Rung>,
}

struct Layer {
    level: String,
    source: String,
    vars: Vec<(String, String)>,
}

/// Fichiers parents d'une requête : la collection et ses dossiers, du plus extérieur au plus proche.
pub struct Context {
    pub collection: Map,
    pub folders: Vec<(String, Map)>,
    pub dotenv: HashMap<String, String>,
}

impl Context {
    pub fn load(root: &Path, request_path: &str) -> Result<Self, CoreError> {
        let collection = read_tree(&root.join(COLLECTION_FILE))?;
        let mut folders = Vec::new();
        let mut dir = String::new();
        let parts: Vec<&str> = request_path.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            dir = if dir.is_empty() { (*part).to_owned() } else { format!("{dir}/{part}") };
            let meta = resolve_path(root, &format!("{dir}/{FOLDER_FILE}"))?;
            let tree = if meta.is_file() { read_tree(&meta)? } else { Map::default() };
            folders.push((dir.clone(), tree));
        }
        Ok(Self { collection, folders, dotenv: read_dotenv(&root.join(".env")) })
    }

    pub fn request_section(map: &Map) -> Option<&Map> {
        map.map("request")
    }
}

fn read_dotenv(path: &Path) -> HashMap<String, String> {
    let Ok(text) = fs::read_to_string(path) else { return HashMap::new() };
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (k, v) = line.trim_start_matches("export ").split_once('=')?;
            let v = v.trim();
            let v = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')).unwrap_or(v);
            Some((k.trim().to_owned(), v.to_owned()))
        })
        .collect()
}

/// Valeurs que des scripts ont données à l'environnement, à la collection ou aux variables globales : elles
/// remplacent celles des fichiers pour la requête qui suit.
#[derive(Debug, Clone, Default)]
pub struct ScopeOverrides {
    pub global: Vec<(String, String)>,
    pub collection: Option<Vec<(String, String)>>,
    pub env: Option<Vec<(String, String)>>,
    /// Valeurs des variables que l'environnement déclare secrètes (`secret: true`), gardées hors des fichiers. Une valeur
    /// dont le nom n'est pas déclaré secret est ignorée.
    pub secrets: Vec<(String, String)>,
}

/// Ce que l'interface voit à la place de la valeur d'un secret : qu'il en ait une, jamais laquelle.
pub const SECRET_MASK: &str = "••••••••";

/// Pile de portées dans l'ordre de Bruno : runtime > requête > dossier > environnement > collection > globales.
pub struct Scope {
    layers: Vec<Layer>,
    secrets: Vec<String>,
    dotenv: HashMap<String, String>,
}

fn enabled_pairs(vars: Vec<crate::request::KeyValue>) -> Vec<(String, String)> {
    vars.into_iter().filter(|v| v.enabled).map(|v| (v.name, v.value)).collect()
}

impl Scope {
    pub fn build(
        root: &Path,
        ctx: &Context,
        request_path: &str,
        doc: &RequestDoc,
        env: Option<&str>,
        runtime: &HashMap<String, String>,
    ) -> Result<Self, CoreError> {
        let mut layers = vec![
            Layer { level: "Runtime".into(), source: "bru.setVar()".into(), vars: sorted(runtime) },
            Layer { level: "Requête".into(), source: request_path.into(), vars: enabled_pairs(doc.variables.clone()) },
        ];
        for (dir, tree) in ctx.folders.iter().rev() {
            let name = tree.map("info").and_then(|i| i.str("name")).unwrap_or(dir).to_owned();
            let vars = Context::request_section(tree)
                .map(|r| enabled_pairs(key_values(r.seq("variables"))))
                .unwrap_or_default();
            layers.push(Layer { level: format!("Dossier {name}"), source: format!("{dir}/{FOLDER_FILE}"), vars });
        }
        let mut secrets = Vec::new();
        if let Some(env) = env {
            let vars = read_environment(root, env)?;
            secrets = vars.iter().filter(|v| v.secret).map(|v| v.name.clone()).collect();
            layers.push(Layer {
                level: format!("Environnement {env}"),
                source: format!("environments/{env}.yml"),
                vars: vars.into_iter().filter(|v| v.enabled).filter_map(|v| Some((v.name, v.value?))).collect(),
            });
        }
        let collection_vars = Context::request_section(&ctx.collection)
            .map(|r| enabled_pairs(key_values(r.seq("variables"))))
            .unwrap_or_default();
        layers.push(Layer { level: "Collection".into(), source: COLLECTION_FILE.into(), vars: collection_vars });
        Ok(Self { layers, secrets, dotenv: ctx.dotenv.clone() })
    }

    pub fn with_overrides(mut self, overrides: ScopeOverrides) -> Self {
        let ScopeOverrides { global, collection, env, secrets } = overrides;
        if let Some(vars) = env {
            match self.layers.iter_mut().find(|l| l.level.starts_with("Environnement")) {
                Some(layer) => layer.vars = vars,
                None => {
                    let at = self.layers.iter().position(|l| l.level == "Collection").unwrap_or(self.layers.len());
                    self.layers
                        .insert(at, Layer { level: "Environnement".into(), source: "bru.setEnvVar()".into(), vars });
                }
            }
        }
        // Après les écritures des scripts : une valeur saisie depuis a priorité sur un instantané plus ancien.
        let secrets: Vec<(String, String)> =
            secrets.into_iter().filter(|(name, _)| self.secrets.contains(name)).collect();
        if !secrets.is_empty() {
            let at = self.layers.iter().position(|l| l.level.starts_with("Environnement"));
            if let Some(layer) = at.map(|i| &mut self.layers[i]) {
                for (name, value) in secrets {
                    match layer.vars.iter_mut().find(|(k, _)| *k == name) {
                        Some(slot) => slot.1 = value,
                        None => layer.vars.push((name, value)),
                    }
                }
            }
        }
        if let Some(vars) = collection {
            if let Some(layer) = self.layers.iter_mut().find(|l| l.level == "Collection") {
                layer.vars = vars;
            }
        }
        if !global.is_empty() {
            self.layers.push(Layer { level: "Globales".into(), source: "bru.setGlobalEnvVar()".into(), vars: global });
        }
        self
    }

    /// Variables des portées dont le nom commence par `level` (`Requête`, `Dossier`, `Environnement`, `Collection`),
    /// fusionnées du plus faible au plus fort, sans celles du runtime.
    pub fn vars_of(&self, level: &str) -> Vec<(String, String)> {
        let mut merged: Vec<(String, String)> = Vec::new();
        for layer in self.layers.iter().rev().filter(|l| l.level.starts_with(level)) {
            for (name, value) in &layer.vars {
                match merged.iter_mut().find(|(k, _)| k == name) {
                    Some(slot) => slot.1.clone_from(value),
                    None => merged.push((name.clone(), value.clone())),
                }
            }
        }
        merged
    }

    /// Les variables à saisir (`{{?nom}}`) que la requête ou une valeur de variable demande, sans doublon, dans l'ordre :
    /// adresse, paramètres, en-têtes, corps, authentification, scripts, puis variables et fichier `.env`. Bruno ne sait pas
    /// les demander en ligne de commande : il ignore la requête.
    pub fn prompt_variables(&self, doc: &RequestDoc) -> Vec<String> {
        let mut found = Vec::new();
        let mut scan = |text: &str| prompts_in(text, &mut found);
        scan(&doc.url);
        for p in &doc.params {
            scan(&p.name);
            scan(&p.value);
        }
        for h in &doc.headers {
            scan(&h.name);
            scan(&h.value);
        }
        scan(&serde_json::to_string(&doc.body).unwrap_or_default());
        scan(&serde_json::to_string(&doc.auth).unwrap_or_default());
        for script in &doc.scripts {
            scan(&script.code);
        }
        for layer in &self.layers {
            for (_, value) in &layer.vars {
                scan(value);
            }
        }
        let mut dotenv: Vec<(&String, &String)> = self.dotenv.iter().collect();
        dotenv.sort();
        for (_, value) in dotenv {
            scan(value);
        }
        found
    }

    /// Variables du fichier `.env` de la collection.
    pub fn dotenv(&self) -> &HashMap<String, String> {
        &self.dotenv
    }

    pub fn lookup(&self, name: &str) -> Option<&str> {
        self.layers.iter().find_map(|l| l.vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str()))
    }

    pub fn infos(&self) -> Vec<VariableInfo> {
        let mut names: Vec<&str> = Vec::new();
        for layer in &self.layers {
            for (k, _) in &layer.vars {
                if !names.contains(&k.as_str()) {
                    names.push(k);
                }
            }
        }
        for s in &self.secrets {
            if !names.contains(&s.as_str()) {
                names.push(s);
            }
        }
        names.sort_unstable();
        names.into_iter().map(|n| self.info(n)).collect()
    }

    pub fn info(&self, name: &str) -> VariableInfo {
        let rungs: Vec<Rung> = self
            .layers
            .iter()
            .map(|l| Rung {
                level: l.level.clone(),
                source: l.source.clone(),
                value: l.vars.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()),
            })
            .collect();
        let secret = self.secrets.iter().any(|s| s == name);
        let rungs: Vec<Rung> = if secret {
            rungs.into_iter().map(|r| Rung { value: r.value.map(|_| SECRET_MASK.to_owned()), ..r }).collect()
        } else {
            rungs
        };
        let winner = rungs.iter().find(|r| r.value.is_some());
        VariableInfo {
            name: name.to_owned(),
            value: winner.and_then(|r| r.value.clone()),
            level: winner.map(|r| r.level.clone()),
            secret,
            rungs,
        }
    }

    /// Remplace les `{{var}}`. Les noms introuvables restent tels quels et sont listés.
    pub fn interpolate(&self, input: &str, unresolved: &mut Vec<String>) -> String {
        self.interpolate_depth(input, unresolved, 0)
    }

    fn interpolate_depth(&self, input: &str, unresolved: &mut Vec<String>, depth: u8) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let Some(end) = after.find("}}") else {
                out.push_str(&rest[start..]);
                return out;
            };
            let name = after[..end].trim();
            match self.value_of(name) {
                Some(v) if depth < 4 => out.push_str(&self.interpolate_depth(&v, unresolved, depth + 1)),
                Some(v) => out.push_str(&v),
                None => {
                    if !unresolved.iter().any(|u| u == name) {
                        unresolved.push(name.to_owned());
                    }
                    out.push_str(&rest[start..start + 2 + end + 2]);
                }
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        out
    }

    fn value_of(&self, name: &str) -> Option<String> {
        if let Some(key) = name.strip_prefix("process.env.") {
            return self.dotenv.get(key).cloned().or_else(|| std::env::var(key).ok());
        }
        if let Some(dynamic) = name.strip_prefix('$') {
            return dynamic_value(dynamic)
                .or_else(|| name.starts_with("$oauth2.").then(|| self.lookup(name).map(str::to_owned)).flatten());
        }
        self.lookup(name).map(str::to_owned)
    }
}

fn sorted(map: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut v: Vec<_> = map.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    v.sort();
    v
}

/// Ajoute à `found` les noms `{{?nom}}` de `text` : un nom ne contient ni accolade ni espace en tête ou en queue.
fn prompts_in(text: &str, found: &mut Vec<String>) {
    let mut rest = text;
    while let Some(start) = rest.find("{{?") {
        let after = &rest[start + 3..];
        let end = after.find(['{', '}']).unwrap_or(after.len());
        let name = &after[..end];
        let trimmed = name.chars().next().is_some_and(|c| !c.is_whitespace())
            && name.chars().last().is_some_and(|c| !c.is_whitespace());
        if after[end..].starts_with("}}") && trimmed {
            if !found.iter().any(|n| n == name) {
                found.push(name.to_owned());
            }
            rest = &after[end + 2..];
        } else {
            rest = &rest[start + 1..];
        }
    }
}

pub fn dynamic_value(name: &str) -> Option<String> {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    match name {
        "guid" | "randomUUID" => Some(uuid::Uuid::new_v4().to_string()),
        "timestamp" => Some(now.as_secs().to_string()),
        "isoTimestamp" => Some(iso8601(now.as_millis())),
        "randomInt" => Some((uuid::Uuid::new_v4().as_u128() % 1000).to_string()),
        _ => None,
    }
}

fn iso8601(millis: u128) -> String {
    let secs = (millis / 1000) as i64;
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        millis % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::{iso8601, prompts_in};

    fn prompts(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        prompts_in(text, &mut found);
        found
    }

    #[test]
    fn iso_timestamp_is_utc() {
        assert_eq!(iso8601(1_790_816_588_196), "2026-10-01T01:03:08.196Z");
        assert_eq!(iso8601(0), "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn ef_run_01_prompt_variables_are_the_names_between_double_braces_after_a_question_mark() {
        assert_eq!(prompts("{{?Code}}"), ["Code"]);
        assert_eq!(prompts("a={{?one}}&b={{?two words}}&c={{?one}}"), ["one", "two words"]);
        assert_eq!(prompts("{{?  padded }}"), Vec::<String>::new());
        assert_eq!(prompts("{{? x}} {{?x }} {{?}} {{?a{b}} {{?{{?ok}}"), ["ok"]);
        assert_eq!(prompts("{{plain}} {{ ?no}} {?no}"), Vec::<String>::new());
        assert_eq!(prompts("{{?é}}"), ["é"]);
    }
}
