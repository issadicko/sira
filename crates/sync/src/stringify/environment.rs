//! Environnements (`stringifyEnvironment.ts`).

use serde_json::Value as Json;
use xc_core::yaml::{is_js_space, Map, Value};

use super::common::{map, put, put_some, typed_value, variable_value};
use super::js::{is_false, non_empty_array, or, trim, truthy, yaml};

pub fn environment(env: &Json) -> Map {
    let mut m = Map::default();
    put_some(&mut m, "name", env.get("name").map(yaml));
    put_some(&mut m, "extends", extends(env.get("extends")));
    if truthy(env.get("color")) {
        put_some(&mut m, "color", env.get("color").map(yaml));
    }
    if let Some(vars) = non_empty_array(env.get("variables")) {
        put(&mut m, "variables", Value::Seq(vars.iter().map(variable).collect()));
    }
    let secrets = env.get("externalSecrets");
    if truthy(secrets) {
        let copies = secrets.and_then(|s| s.get("variables")).and_then(Json::as_array).into_iter().flatten().map(|v| {
            if v.is_object() {
                yaml(v)
            } else {
                Value::Map(Map::default())
            }
        });
        let mut s = Map::default();
        put_some(&mut s, "type", secrets.and_then(|s| s.get("type")).map(yaml));
        put(&mut s, "variables", Value::Seq(copies.collect()));
        put(&mut m, "externalSecrets", Value::Map(s));
    }
    m
}

fn variable(v: &Json) -> Value {
    let typed = v.get("dataType").is_some_and(|t| truthy(Some(t)) && t.as_str() != Some("string"));
    let mut m = if v.get("secret") == Some(&Json::Bool(true)) {
        let mut m = map([("secret", Value::Bool(true)), ("name", or(v.get("name"), Value::str("")))]);
        if typed {
            put_some(&mut m, "type", v.get("dataType").map(yaml));
        }
        m
    } else {
        map([("name", or(v.get("name"), Value::str(""))), ("value", typed_value(v, variable_value(v.get("value"))))])
    };
    if is_false(v.get("enabled")) {
        put(&mut m, "disabled", Value::Bool(true));
    }
    put_some(&mut m, "description", v.get("description").map(yaml));
    Value::Map(m)
}

fn extends(reference: Option<&Json>) -> Option<Value> {
    match reference? {
        Json::String(s) => valid_name(s).map(Value::str),
        Json::Array(list) if !list.is_empty() => {
            let names: Option<Vec<Value>> =
                list.iter().map(|n| n.as_str().and_then(valid_name).map(Value::str)).collect();
            names.map(Value::Seq)
        }
        _ => None,
    }
}

/// `validatedEnvironmentName` : nom nettoyé s'il est utilisable comme nom de fichier.
fn valid_name(reference: &str) -> Option<&str> {
    let name = trim(reference);
    let invalid = |c: char| "<>:\"/\\|?*".contains(c) || c < ' ';
    let upper = name.to_ascii_uppercase();
    let reserved = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit());
    let first = name.chars().next()?;
    let last = name.chars().last()?;
    let rejected = name.encode_utf16().count() > 255
        || reserved
        || name.chars().any(invalid)
        || is_js_space(first)
        || first == '-'
        || is_js_space(last)
        || last == '.';
    (!rejected).then_some(name)
}
