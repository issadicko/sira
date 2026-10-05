use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const MAX_PASSES: usize = 16;

/// Les cartes de variables d'un script, avec leurs valeurs typées. Précédence de Bruno pour l'interpolation, de la
/// plus faible à la plus forte : global, collection, environnement, dossier, requête, identifiants OAuth 2, runtime.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Vars {
    pub env: Map<String, Value>,
    pub runtime: Map<String, Value>,
    pub global: Map<String, Value>,
    pub collection: Map<String, Value>,
    pub folder: Map<String, Value>,
    pub request: Map<String, Value>,
    pub oauth2: Map<String, Value>,
    pub process_env: Map<String, Value>,
}

/// Cartes qu'un script a modifiées : l'hôte les réécrit (les autres sont inchangées).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Dirty {
    pub env: bool,
    pub runtime: bool,
    pub global: bool,
    pub collection: bool,
}

pub const ENV_NAME: &str = "__name__";

impl Vars {
    pub fn scope(&self, name: &str) -> Option<&Map<String, Value>> {
        Some(match name {
            "env" => &self.env,
            "runtime" => &self.runtime,
            "global" => &self.global,
            "collection" => &self.collection,
            "folder" => &self.folder,
            "request" => &self.request,
            "oauth2" => &self.oauth2,
            "process" => &self.process_env,
            _ => return None,
        })
    }

    pub fn scope_mut(&mut self, name: &str) -> Option<&mut Map<String, Value>> {
        Some(match name {
            "env" => &mut self.env,
            "runtime" => &mut self.runtime,
            "global" => &mut self.global,
            "collection" => &mut self.collection,
            _ => return None,
        })
    }

    fn lookup(&self, key: &str) -> Option<&Value> {
        [&self.runtime, &self.oauth2, &self.request, &self.folder, &self.env, &self.collection, &self.global]
            .into_iter()
            .find_map(|scope| scope.get(key))
    }

    /// Valeur d'un `{{chemin}}` : clé exacte, sinon chemin `a.b[0]["c"]` à travers les objets (comme `lodash.get`).
    fn resolve(&self, placeholder: &str, dynamic: &dyn Fn(&str) -> Option<String>) -> Option<String> {
        if let Some(name) = placeholder.strip_prefix('$') {
            return dynamic(name);
        }
        if let Some(key) = placeholder.strip_prefix("process.env.") {
            return self.process_env.get(key).map(text);
        }
        if let Some(value) = self.lookup(placeholder) {
            return Some(text(value));
        }
        let mut segments = segments(placeholder).into_iter();
        let mut current = self.lookup(&segments.next()?)?;
        for segment in segments {
            current = match current {
                Value::Object(map) => map.get(&segment)?,
                Value::Array(list) => list.get(segment.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(text(current))
    }

    /// Remplace les `{{nom}}` (puis ceux que les valeurs remplacées contiennent) ; un nom inconnu reste tel quel.
    pub fn interpolate(&self, input: &str, dynamic: &dyn Fn(&str) -> Option<String>) -> String {
        let mut current = input.to_owned();
        for _ in 0..MAX_PASSES {
            let next = self.pass(&current, dynamic);
            if next == current {
                break;
            }
            current = next;
        }
        current
    }

    fn pass(&self, input: &str, dynamic: &dyn Fn(&str) -> Option<String>) -> String {
        let mut out = String::with_capacity(input.len());
        let mut rest = input;
        while let Some(start) = rest.find("{{") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let name = after.split('}').next().unwrap_or_default();
            let closed = !name.is_empty() && after[name.len()..].starts_with("}}");
            match closed.then(|| self.resolve(name, dynamic)).flatten() {
                Some(value) => {
                    out.push_str(&value);
                    rest = &after[name.len() + 2..];
                }
                None => {
                    out.push_str("{{");
                    rest = after;
                }
            }
        }
        out.push_str(rest);
        out
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `a.b[0]["c"]` -> `a`, `b`, `0`, `c`.
fn segments(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = path.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '.' => out.extend((!current.is_empty()).then(|| std::mem::take(&mut current))),
            '[' => {
                out.extend((!current.is_empty()).then(|| std::mem::take(&mut current)));
                let mut inner = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    inner.push(c);
                }
                out.push(inner.trim_matches(['"', '\'']).to_owned());
            }
            c => current.push(c),
        }
    }
    out.extend((!current.is_empty()).then_some(current));
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn vars() -> Vars {
        let map = |v: Value| v.as_object().cloned().unwrap();
        Vars {
            global: map(json!({ "who": "global", "only_global": "g" })),
            collection: map(json!({ "who": "collection" })),
            env: map(
                json!({ "who": "env", "base": "http://x", "n": 3, "user": { "tags": ["a", "b"], "name": "Ada" } }),
            ),
            folder: map(json!({ "who": "folder" })),
            request: map(json!({ "who": "request" })),
            runtime: map(json!({ "name": "{{who}}" })),
            process_env: map(json!({ "HOME": "/home/x" })),
            ..Vars::default()
        }
    }

    fn none(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn ef_scr_02_a_stronger_scope_hides_a_weaker_one() {
        let v = vars();
        assert_eq!(v.interpolate("{{who}} {{only_global}}", &none), "request g");
    }

    #[test]
    fn ef_scr_02_values_are_resolved_recursively_and_unknown_names_stay() {
        let v = vars();
        assert_eq!(v.interpolate("{{name}} / {{missing}}", &none), "request / {{missing}}");
    }

    #[test]
    fn ef_scr_02_paths_reach_into_objects_and_arrays() {
        let v = vars();
        assert_eq!(v.interpolate("{{user.name}} {{user.tags[1]}} {{n}}", &none), "Ada b 3");
        assert_eq!(v.interpolate(r#"{{user["name"]}}"#, &none), "Ada");
    }

    #[test]
    fn ef_scr_02_process_env_and_dynamic_values_have_their_own_names() {
        let v = vars();
        let dynamic = |name: &str| (name == "guid").then(|| "G".to_owned());
        assert_eq!(v.interpolate("{{process.env.HOME}} {{$guid}} {{$nope}}", &dynamic), "/home/x G {{$nope}}");
    }

    #[test]
    fn ef_scr_02_a_name_with_spaces_does_not_resolve_like_in_bruno() {
        assert_eq!(vars().interpolate("{{ who }}", &none), "{{ who }}");
    }
}
