//! Effet observable de `validateSchema` (yup, mode strict) : la conversion échoue quand une
//! valeur recopiée telle quelle de la spec n'a pas le type attendu. Seuls ces champs-là sont
//! vérifiés, les autres sont produits par le convertisseur avec le bon type.

use serde_json::Value;

use super::{OpenApiError, R};

#[derive(Clone, Copy)]
enum Kind {
    Text,
    Flag,
}

fn check(v: &Value, key: &str, kind: Kind, path: &str) -> R<()> {
    let ok = matches!(
        (v.get(key), kind),
        (None | Some(Value::Null | Value::String(_)), Kind::Text) | (None | Some(Value::Bool(_)), Kind::Flag)
    );
    if ok {
        return Ok(());
    }
    let expected = match kind {
        Kind::Text => "une chaîne",
        Kind::Flag => "un booléen",
    };
    Err(OpenApiError::Schema(format!("{path}.{key} doit être {expected}")))
}

fn list<'a>(v: &'a Value, key: &str) -> impl Iterator<Item = (usize, &'a Value)> {
    v.get(key).and_then(Value::as_array).into_iter().flatten().enumerate()
}

fn key_values(v: &Value, key: &str, path: &str) -> R<()> {
    for (i, entry) in list(v, key) {
        let path = format!("{path}.{key}[{i}]");
        for field in ["name", "value", "description"] {
            check(entry, field, Kind::Text, &path)?;
        }
        check(entry, "enabled", Kind::Flag, &path)?;
    }
    Ok(())
}

fn multipart(body: &Value, path: &str) -> R<()> {
    for (i, entry) in list(body, "multipartForm") {
        let path = format!("{path}.multipartForm[{i}]");
        check(entry, "name", Kind::Text, &path)?;
        check(entry, "description", Kind::Text, &path)?;
        check(entry, "enabled", Kind::Flag, &path)?;
        if entry.get("type").and_then(Value::as_str) != Some("file") {
            check(entry, "value", Kind::Text, &path)?;
        }
    }
    Ok(())
}

fn request(req: &Value, path: &str) -> R<()> {
    key_values(req, "headers", path)?;
    key_values(req, "params", path)?;
    if let Some(body) = req.get("body") {
        key_values(body, "formUrlEncoded", &format!("{path}.body"))?;
        multipart(body, &format!("{path}.body"))?;
    }
    for (i, var) in req.get("vars").into_iter().flat_map(|v| list(v, "req")) {
        check(var, "name", Kind::Text, &format!("{path}.vars.req[{i}]"))?;
    }
    auth(req, path)
}

fn auth(req: &Value, path: &str) -> R<()> {
    let Some(auth) = req.get("auth") else { return Ok(()) };
    if let Some(apikey) = auth.get("apikey").filter(|a| a.is_object()) {
        check(apikey, "key", Kind::Text, &format!("{path}.auth.apikey"))?;
    }
    if let Some(oauth2) = auth.get("oauth2").filter(|a| a.is_object()) {
        for field in ["authorizationUrl", "accessTokenUrl", "refreshTokenUrl", "scope"] {
            check(oauth2, field, Kind::Text, &format!("{path}.auth.oauth2"))?;
        }
    }
    Ok(())
}

fn items(v: &Value, path: &str) -> R<()> {
    for (i, item) in list(v, "items") {
        let path = format!("{path}items[{i}]");
        if let Some(req) = item.get("request") {
            request(req, &format!("{path}.request"))?;
        }
        for (j, example) in list(item, "examples") {
            let path = format!("{path}.examples[{j}]");
            check(example, "description", Kind::Text, &path)?;
            if let Some(req) = example.get("request") {
                request(req, &format!("{path}.request"))?;
            }
            if let Some(res) = example.get("response") {
                key_values(res, "headers", &format!("{path}.response"))?;
            }
        }
        items(item, &format!("{path}."))?;
    }
    Ok(())
}

pub fn collection(c: &Value) -> R<()> {
    items(c, "")?;
    for (i, env) in list(c, "environments") {
        for (j, var) in list(env, "variables") {
            check(var, "name", Kind::Text, &format!("environments[{i}].variables[{j}]"))?;
        }
    }
    if let Some(req) = c.get("root").and_then(|r| r.get("request")) {
        auth(req, "root.request")?;
    }
    Ok(())
}
