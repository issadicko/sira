//! Exemples enregistrés d'une requête (`example { … }`) : port des trois grammaires `example`, `example/request` et
//! `example/response` de bruno-lang v2 et de `bruExampleToJson` (bruno-filestore). Ici, les blocs s'écrivent
//! `clé: { … }` (deux-points avant l'accolade) et le corps de la réponse tient entre `'''`.

use serde_json::{json, Map, Value as Json};

use super::scanner::{self, Dialect, Scanner};
use super::semantics;
use crate::js::{string_to_number, trim};

type R<T> = Result<T, String>;

/// L'exemple d'un bloc `example` : `parentMethod` est la méthode de la requête, reprise si l'exemple n'a pas la sienne.
pub fn parse(raw: &str, parent_method: &str) -> R<Json> {
    let text = unindent(raw);
    let mut s = Scanner::new(&text, Dialect::Request);
    let mut name = String::new();
    let mut description = String::new();
    let mut request = Json::Null;
    let mut response = Json::Null;
    loop {
        s.skip_space();
        if s.eof() {
            break;
        }
        if key(&mut s, "name") {
            name = s.rest_of_line();
        } else if key(&mut s, "description") {
            description = trim(&description_value(&mut s)).to_owned();
        } else if key(&mut s, "request") {
            request = parse_request(&braced(&mut s)?)?;
        } else if key(&mut s, "response") {
            response = parse_response(&braced(&mut s)?)?;
        } else {
            return Err(format!(
                "ligne {} : « {} » n'est ni name, ni description, ni request, ni response",
                s.line(),
                s.snippet()
            ));
        }
    }
    Ok(example(&name, &description, &request, &response, parent_method))
}

/// `name`, `description`… suivi de `st* ":" st*`.
fn key(s: &mut Scanner<'_>, word: &str) -> bool {
    let Some(rest) = s.rest().strip_prefix(word) else { return false };
    let after = rest.trim_start_matches([' ', '\t']);
    if !after.starts_with(':') {
        return false;
    }
    let consumed = word.len() + (rest.len() - after.len()) + 1;
    s.skip_bytes(consumed);
    s.skip_st();
    true
}

/// `description` : un texte entre `'''` (retrait de deux espaces) ou le reste de la ligne.
fn description_value(s: &mut Scanner<'_>) -> String {
    let rest = s.rest();
    if let Some(body) = rest.strip_prefix("'''") {
        if let Some(close) = body.find("'''") {
            s.skip_bytes(3 + close + 3);
            let mut value = scanner::outdent(&body[..close], 2);
            let after = s.rest();
            let blanks = after.len() - after.trim_start_matches([' ', '\t']).len();
            if let Some(tail) = after[blanks..].strip_prefix("@contentType(") {
                if let Some(end) = tail.find(')') {
                    value = format!("{value} @contentType({})", &tail[..end]);
                    s.skip_bytes(blanks + "@contentType(".len() + end + 1);
                }
            }
            return value;
        }
    }
    s.rest_of_line()
}

/// `{ … }` d'un bloc `request` ou `response` : tout jusqu'à la ligne `}`, désindenté de deux espaces.
fn braced(s: &mut Scanner<'_>) -> R<String> {
    if !s.eat("{") {
        return Err(format!("ligne {} : « {{ » attendu", s.line()));
    }
    s.text_block().map_err(|e| e.to_string())
}

fn parse_request(content: &str) -> R<Json> {
    let mut s = Scanner::new(content, Dialect::Request);
    let mut request = Json::Object(Map::new());
    loop {
        s.skip_space();
        if s.eof() {
            return Ok(request);
        }
        let line = s.line();
        let part = if key(&mut s, "url") {
            json!({ "url": s.rest_of_line() })
        } else if key(&mut s, "method") {
            json!({ "method": s.rest_of_line() })
        } else if key(&mut s, "mode") {
            let mode = s.rest_of_line();
            json!({ "body": { "mode": if mode.is_empty() { "none".to_owned() } else { mode } } })
        } else if key(&mut s, "params:path") {
            params(&mut s, "path")?
        } else if key(&mut s, "params:query") {
            params(&mut s, "query")?
        } else if key(&mut s, "headers") {
            json!({ "headers": semantics::kv(&pairs(&mut s)?, true, false) })
        } else if key(&mut s, "body:json") {
            text_body(&mut s, "json", "json")?
        } else if key(&mut s, "body:text") {
            text_body(&mut s, "text", "text")?
        } else if key(&mut s, "body:xml") {
            text_body(&mut s, "xml", "xml")?
        } else if key(&mut s, "body:sparql") {
            text_body(&mut s, "sparql", "sparql")?
        } else if key(&mut s, "body:graphql:vars") {
            json!({ "body": { "mode": "graphql", "graphql": { "variables": block_text(&mut s)? } } })
        } else if key(&mut s, "body:graphql") {
            json!({ "body": { "mode": "graphql", "graphql": { "query": block_text(&mut s)? } } })
        } else if key(&mut s, "body:form-urlencoded") {
            json!({ "body": { "mode": "formUrlEncoded", "formUrlEncoded": semantics::kv(&pairs(&mut s)?, true, false) } })
        } else if key(&mut s, "body:multipart-form") {
            json!({ "body": { "mode": "multipartForm", "multipartForm": semantics::multipart(&pairs(&mut s)?) } })
        } else if key(&mut s, "body:file") {
            json!({ "body": { "mode": "file", "file": semantics::file_body(&pairs(&mut s)?) } })
        } else {
            return Err(format!("requête de l'exemple, ligne {line} : « {} » inattendu", s.snippet()));
        };
        merge(&mut request, part);
    }
}

fn params(s: &mut Scanner<'_>, kind: &str) -> R<Json> {
    Ok(semantics::params(&pairs(s)?, kind))
}

fn pairs(s: &mut Scanner<'_>) -> R<Vec<scanner::Pair>> {
    s.skip_st();
    if !s.eat("{") {
        return Err(format!("ligne {} : « {{ » attendu", s.line()));
    }
    s.pairs(false).map_err(|e| e.to_string())
}

fn block_text(s: &mut Scanner<'_>) -> R<String> {
    s.skip_st();
    braced(s)
}

fn text_body(s: &mut Scanner<'_>, mode: &str, field: &str) -> R<Json> {
    Ok(json!({ "body": { "mode": mode, field: block_text(s)? } }))
}

fn merge(into: &mut Json, from: Json) {
    match (into, from) {
        (Json::Object(a), Json::Object(b)) => {
            for (key, value) in b {
                match a.get_mut(&key) {
                    Some(existing) => merge(existing, value),
                    None => {
                        a.insert(key, value);
                    }
                }
            }
        }
        (Json::Array(a), Json::Array(b)) => a.extend(b),
        (slot, value) => *slot = value,
    }
}

fn parse_response(content: &str) -> R<Json> {
    let mut s = Scanner::new(content, Dialect::Request);
    let mut response = Map::new();
    loop {
        s.skip_space();
        if s.eof() {
            return Ok(Json::Object(response));
        }
        let line = s.line();
        if key(&mut s, "headers") {
            response.insert("headers".into(), Json::Array(semantics::kv(&pairs(&mut s)?, true, false)));
        } else if key(&mut s, "status") {
            let list = pairs(&mut s)?;
            let get =
                |name: &str| list.iter().find(|p| p.key == name).and_then(|p| p.value.text()).filter(|v| !v.is_empty());
            response.insert("status".into(), get("code").map_or(json!(200), |c| json!(c)));
            response.insert("statusText".into(), json!(get("text").unwrap_or("OK")));
        } else if key(&mut s, "body") {
            response.insert("body".into(), response_body(&mut s)?);
        } else {
            return Err(format!("réponse de l'exemple, ligne {line} : « {} » inattendu", s.snippet()));
        }
    }
}

/// `body: { type: json  content: '''…''' }`.
fn response_body(s: &mut Scanner<'_>) -> R<Json> {
    s.skip_st();
    if !s.eat("{") {
        return Err(format!("ligne {} : « {{ » attendu", s.line()));
    }
    let mut body = Map::new();
    loop {
        s.skip_space();
        if s.eat("}") {
            return Ok(Json::Object(body));
        }
        if s.eof() {
            return Err("corps de la réponse non fermé".into());
        }
        if key(s, "type") {
            body.insert("type".into(), json!(s.rest_of_line()));
        } else if key(s, "content") {
            body.insert("content".into(), json!(multiline_content(s)?));
        } else {
            return Err(format!("corps de la réponse, ligne {} : « {} » inattendu", s.line(), s.snippet()));
        }
    }
}

/// Le contenu entre `'''` : on retire les deux espaces qui précèdent le `'''` fermant, le premier et le dernier saut
/// de ligne, puis quatre espaces d'indentation.
fn multiline_content(s: &mut Scanner<'_>) -> R<String> {
    let Some(body) = s.rest().strip_prefix("'''") else {
        return Err(format!("ligne {} : « ''' » attendu", s.line()));
    };
    let Some(close) = body.find("'''") else { return Err("contenu non fermé : « ''' » attendu".into()) };
    let content = &body[..close];
    s.skip_bytes(3 + close + 3);
    s.skip_st();
    let content = content.strip_suffix("  ").unwrap_or(content);
    let content = content.strip_prefix('\n').unwrap_or(content);
    let content = content.strip_suffix('\n').unwrap_or(content);
    Ok(scanner::outdent(content, 4))
}

/// `parseExampleContent` : retire l'indentation commune puis les blancs de bout.
fn unindent(raw: &str) -> String {
    let lines: Vec<&str> = raw.split('\n').collect();
    let indent = |l: &str| l.len() - l.trim_start_matches([' ', '\t']).len();
    let common = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| indent(l)).min().unwrap_or(0);
    let joined =
        lines.iter().map(|l| if l.trim().is_empty() { *l } else { &l[common..] }).collect::<Vec<_>>().join("\n");
    trim(&joined).to_owned()
}

/// `bruExampleToJson` : l'exemple tel que le sérialiseur YAML le lit.
fn example(name: &str, description: &str, request: &Json, response: &Json, parent_method: &str) -> Json {
    // Les anciennes importations Postman écrivaient le statut et son libellé permutés.
    let mut status = response
        .get("status")
        .and_then(|s| s.as_str().map(str::to_owned).or_else(|| s.as_i64().map(|n| n.to_string())))
        .unwrap_or_else(|| "200".into());
    let mut text = response.get("statusText").and_then(Json::as_str).unwrap_or("OK").to_owned();
    if string_to_number(&status).is_nan() && !string_to_number(&text).is_nan() {
        std::mem::swap(&mut status, &mut text);
    }
    let code = string_to_number(&status);
    let method = request.get("method").and_then(Json::as_str).filter(|m| !m.is_empty()).unwrap_or(parent_method);
    json!({
        "name": name,
        "description": description,
        "request": {
            "method": method,
            "url": request.get("url").cloned().unwrap_or(Json::Null),
            "headers": request.get("headers").cloned().unwrap_or_else(|| json!([])),
            "body": request.get("body").cloned().unwrap_or_else(|| json!({ "mode": "none" })),
            "params": request.get("params").cloned().unwrap_or_else(|| json!([])),
        },
        "response": {
            "headers": response.get("headers").and_then(Json::as_array).map(|list| list.iter().map(|h| json!({ "name": h["name"], "value": h["value"] })).collect::<Vec<_>>()).unwrap_or_default(),
            "status": if code.is_nan() || code == 0.0 { json!(200) } else { crate::js::number_value(code) },
            "statusText": if text.is_empty() { "OK" } else { &text },
            "body": {
                "type": response.pointer("/body/type").and_then(Json::as_str).unwrap_or("json"),
                "content": response.pointer("/body/content").and_then(Json::as_str).unwrap_or_default(),
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW: &str = "\n  name: Two users\n  description: Réponse nominale\n  request: {\n    url: https://shop.test/users?page=1\n    method: GET\n    mode: none\n    params:query: {\n      page: 1\n    }\n    headers: {\n      Accept: application/json\n    }\n  }\n  response: {\n    headers: {\n      Content-Type: application/json\n    }\n    status: {\n      code: 200\n      text: OK\n    }\n    body: {\n      type: json\n      content: '''\n        {\n          \"users\": 2\n        }\n      '''\n    }\n  }\n";

    #[test]
    fn a_saved_example_is_read_with_its_request_and_response() {
        let example = parse(RAW, "get").unwrap();
        assert_eq!(
            (example["name"].as_str(), example["description"].as_str()),
            (Some("Two users"), Some("Réponse nominale"))
        );
        assert_eq!(example["request"]["url"], "https://shop.test/users?page=1");
        assert_eq!(example["request"]["method"], "GET");
        assert_eq!(example["request"]["params"][0]["name"], "page");
        assert_eq!(example["request"]["headers"][0]["value"], "application/json");
        assert_eq!(example["response"]["status"], json!(200));
        assert_eq!(example["response"]["statusText"], "OK");
        assert_eq!(example["response"]["headers"][0], json!({ "name": "Content-Type", "value": "application/json" }));
        assert_eq!(example["response"]["body"], json!({ "type": "json", "content": "{\n  \"users\": 2\n}" }));
    }

    #[test]
    fn the_parent_method_fills_in_an_example_without_one() {
        let example = parse(
            "name: x\nrequest: {\n  url: u\n}\nresponse: {\n  status: {\n    code: 404\n    text: Not Found\n  }\n}\n",
            "get",
        )
        .unwrap();
        assert_eq!(example["request"]["method"], "get");
        assert_eq!(
            (example["response"]["status"].clone(), example["response"]["statusText"].as_str()),
            (json!(404), Some("Not Found"))
        );
    }

    #[test]
    fn a_swapped_status_and_text_are_put_back() {
        let example = parse("name: x\nresponse: {\n  status: {\n    code: OK\n    text: 202\n  }\n}\n", "get").unwrap();
        assert_eq!(
            (example["response"]["status"].clone(), example["response"]["statusText"].as_str()),
            (json!(202), Some("OK"))
        );
    }

    #[test]
    fn json_bodies_in_the_request_part_are_read() {
        let example =
            parse("name: x\nrequest: {\n  url: u\n  mode: json\n  body:json: {\n    {\"a\": 1}\n  }\n}\n", "post")
                .unwrap();
        assert_eq!(example["request"]["body"], json!({ "mode": "json", "json": "{\"a\": 1}" }));
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(parse("nope", "get").is_err());
    }
}
