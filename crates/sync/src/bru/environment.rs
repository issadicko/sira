//! Fichier d'environnement `.bru` : port de `envToJson.js` (bruno-lang v2) et de `parseBruEnvironment` (bruno-filestore).
//! Le nom de l'environnement est celui du fichier ; ses variables viennent de `vars`, `vars:secret` (sans valeur,
//! elles ne sont pas dans le fichier) et `vars:externalsecrets:<fournisseur>`.

use serde_json::{json, Map, Value as Json};

use super::scanner::{Annotation, Dialect, ParseError, Scanner, Value};

type Read = (Json, Vec<(usize, String)>);

pub fn read(text: &str) -> Result<Read, ParseError> {
    let mut s = Scanner::new(text, Dialect::Environment);
    let mut variables: Vec<Json> = Vec::new();
    let mut env = Map::new();
    let mut skipped = Vec::new();
    loop {
        s.skip_space();
        if s.eof() {
            break;
        }
        let line = s.line();
        if s.rest().starts_with("extends") && !s.rest().starts_with("extends_") {
            s.eat("extends");
            env.extend(extends(&mut s)?);
        } else if s.eat("color:") {
            let color = s.rest_of_line();
            env.insert("color".into(), json!(color));
        } else {
            let name = s.name();
            match name.as_str() {
                "vars" => {
                    s.open(&name)?;
                    for pair in s.pairs(false)? {
                        variables.push(variable(&pair.key, &pair.value, &pair.annotations, false));
                    }
                }
                "vars:secret" => {
                    for (name, annotations) in s.name_list()?.into_iter().filter(|(n, _)| !n.is_empty()) {
                        variables.push(variable(&name, &Value::Text(String::new()), &annotations, true));
                    }
                }
                other if other.starts_with("vars:externalsecrets:") => {
                    let provider = other["vars:externalsecrets:".len()..].to_owned();
                    s.open(other)?;
                    let list: Vec<Json> = s
                        .pairs(false)?
                        .iter()
                        .map(|p| json!({ "name": p.key.trim_start_matches('~'), "value": p.value.text().unwrap_or_default() }))
                        .collect();
                    env.insert("externalSecrets".into(), json!({ "type": provider, "variables": list }));
                }
                "" => return Err(s.error(format!("« {} » n'est pas un bloc", s.snippet()))),
                other => {
                    s.skip_unknown(other)?;
                    skipped.push((line, other.to_owned()));
                }
            }
        }
    }
    env.insert("variables".into(), Json::Array(variables));
    Ok((Json::Object(env), skipped))
}

fn variable(key: &str, value: &Value, annotations: &[Annotation], secret: bool) -> Json {
    let (name, enabled) = match key.strip_prefix('~') {
        Some(rest) => (rest, false),
        None => (key, true),
    };
    let mut var = Map::new();
    var.insert("name".into(), json!(name));
    var.insert("value".into(), json!(value.text().unwrap_or_default()));
    var.insert("enabled".into(), json!(enabled));
    // Plusieurs `@description` : la dernière décrit la variable.
    if let Some(annotation) = annotations.iter().rev().find(|a| a.name == "description") {
        var.insert("description".into(), json!(annotation.value.clone().unwrap_or_default()));
    }
    if let Some(annotation) =
        annotations.iter().rev().find(|a| matches!(a.name.as_str(), "number" | "boolean" | "object"))
    {
        var.insert("dataType".into(), json!(annotation.name));
        let raw = var["value"].as_str().unwrap_or_default().to_owned();
        let typed = match annotation.name.as_str() {
            "number" if !raw.trim().is_empty() => {
                let n = crate::js::string_to_number(&raw);
                (!n.is_nan()).then(|| crate::js::number_value(n))
            }
            "boolean" if raw == "true" => Some(json!(true)),
            "boolean" if raw == "false" => Some(json!(false)),
            "object" if !raw.trim().is_empty() => {
                serde_json::from_str::<Json>(raw.trim()).ok().filter(|v| v.is_object() || v.is_array())
            }
            _ => None,
        };
        if let Some(typed) = typed {
            var.insert("value".into(), typed);
        }
    }
    var.insert("secret".into(), json!(secret));
    var.insert("type".into(), json!("text"));
    Json::Object(var)
}

/// `extends: Base` ou `extends [Base, "Autre, nom"]`.
fn extends(s: &mut Scanner<'_>) -> Result<Map<String, Json>, ParseError> {
    let mut out = Map::new();
    if s.eat(":") {
        let name = s.rest_of_line();
        if !name.is_empty() {
            out.insert("extends".into(), json!(name));
        }
        return Ok(out);
    }
    s.skip_st();
    if !s.eat("[") {
        return Err(s.error("« : » ou « [ » attendu après « extends »"));
    }
    let rest = s.rest();
    let Some(end) = rest.find(']') else { return Err(s.error("« ] » attendu")) };
    let names = split_names(&rest[..end]);
    s.skip_bytes(end + 1);
    if !names.is_empty() {
        out.insert("extends".into(), json!(names));
    }
    Ok(out)
}

/// Noms séparés par des virgules ; un nom entre guillemets peut contenir virgules et crochets.
fn split_names(list: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut chars = list.chars().peekable();
    loop {
        while chars.peek().is_some_and(|c| c.is_whitespace()) {
            chars.next();
        }
        let mut name = String::new();
        if chars.peek() == Some(&'"') {
            chars.next();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => {
                        if let Some(next) = chars.next() {
                            name.push(match next {
                                'r' => '\r',
                                'n' => '\n',
                                't' => '\t',
                                other => other,
                            });
                        }
                    }
                    '"' => break,
                    other => name.push(other),
                }
            }
            while chars.peek().is_some_and(|c| *c != ',') {
                chars.next();
            }
        } else {
            while let Some(c) = chars.peek() {
                if *c == ',' {
                    break;
                }
                name.push(*c);
                chars.next();
            }
        }
        let name = name.trim().to_owned();
        if !name.is_empty() {
            names.push(name);
        }
        if chars.next().is_none() {
            return names;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variables_secrets_and_color_are_read() {
        let (env, skipped) = read(
            "vars {\n  baseUrl: https://shop.test\n  ~off: x\n  @description('Clé')\n  @number\n  retries: 3\n  pem: '''\n    -----BEGIN-----\n    abc\n  '''\n}\nvars:secret [\n  token,\n  apiKey\n]\ncolor: #ff0000\n",
        )
        .unwrap();
        assert!(skipped.is_empty());
        let names: Vec<_> = env["variables"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| (v["name"].as_str().unwrap(), v["secret"].as_bool().unwrap(), v["enabled"].as_bool().unwrap()))
            .collect();
        assert_eq!(
            names,
            [
                ("baseUrl", false, true),
                ("off", false, false),
                ("retries", false, true),
                ("pem", false, true),
                ("token", true, true),
                ("apiKey", true, true)
            ]
        );
        let retries = &env["variables"][2];
        assert_eq!(
            (retries["value"].clone(), retries["dataType"].as_str(), retries["description"].as_str()),
            (json!(3), Some("number"), Some("Clé"))
        );
        assert_eq!(env["variables"][3]["value"], "-----BEGIN-----\nabc");
        assert_eq!(env["color"], "#ff0000");
    }

    #[test]
    fn extends_accepts_a_name_or_a_list() {
        let (env, _) = read("extends: Base\nvars {\n  a: 1\n}\n").unwrap();
        assert_eq!(env["extends"], "Base");
        let (env, _) = read("extends [Base, \"Autre, nom\"]\nvars {\n  a: 1\n}\n").unwrap();
        assert_eq!(env["extends"], json!(["Base", "Autre, nom"]));
    }

    #[test]
    fn an_empty_file_has_no_variables() {
        let (env, _) = read("").unwrap();
        assert_eq!(env["variables"], json!([]));
    }

    #[test]
    fn unknown_blocks_are_reported() {
        let (_, skipped) = read("future {\n  a: b\n}\nvars {\n  a: 1\n}\n").unwrap();
        assert_eq!(skipped, [(1, "future".to_owned())]);
    }
}
