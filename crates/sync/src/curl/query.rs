//! Chaînes de requête : `parseQueryParams` / `buildQueryString` de `@usebruno/common`, `parse` de
//! `query-string` 7 (`sort: false`) et `decode-uri-component` 0.2.

use serde_json::{json, Value};

use super::js::{decode_uri_component_utf16, js_trim, replace_all, utf16, JsObject};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QueryParam {
    pub name: String,
    pub value: Option<String>,
}

impl QueryParam {
    pub(crate) fn to_json(&self) -> Value {
        match &self.value {
            Some(value) => json!({ "name": self.name, "value": value }),
            None => json!({ "name": self.name }),
        }
    }
}

/// `parseQueryParams(query, { decode: false })`.
pub(crate) fn parse_query_params(query: Option<&str>) -> Vec<QueryParam> {
    let Some(query) = query.filter(|q| !q.is_empty()) else { return Vec::new() };
    let query = query.split('#').next().unwrap_or_default();
    query
        .split('&')
        .filter_map(|pair| {
            let (name, value) = match pair.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (pair, None),
            };
            (!name.is_empty()).then(|| QueryParam { name: name.to_owned(), value })
        })
        .collect()
}

/// `buildQueryString(params, { encode: false })`.
pub(crate) fn build_query_string(params: &[QueryParam]) -> String {
    params
        .iter()
        .filter(|p| !js_trim(&p.name).is_empty())
        .map(|p| match &p.value {
            Some(value) => format!("{}={value}", p.name),
            None => p.name.clone(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Valeur d'une clé après `query-string.parse` : `null` (pas de `=`), texte, ou liste si la clé se répète.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum QsValue {
    Null,
    Str(String),
    List(Vec<Option<String>>),
}

impl QsValue {
    pub(crate) fn to_json(&self) -> Value {
        match self {
            QsValue::Null => Value::Null,
            QsValue::Str(s) => json!(s),
            QsValue::List(items) => json!(items),
        }
    }

    fn into_items(self) -> Vec<Option<String>> {
        match self {
            QsValue::Null => vec![None],
            QsValue::Str(s) => vec![Some(s)],
            QsValue::List(items) => items,
        }
    }
}

/// `queryString.parse(query, { sort: false })`.
pub(crate) fn query_string_parse(query: &str) -> JsObject<QsValue> {
    let mut result = JsObject::null_proto();
    let query = js_trim(query);
    let query = query.strip_prefix(['?', '#', '&']).unwrap_or(query);
    if query.is_empty() {
        return result;
    }
    for param in query.split('&').filter(|p| !p.is_empty()) {
        let param = param.replace('+', " ");
        let (key, value) = match param.split_once('=') {
            Some((key, value)) => (key, QsValue::Str(decode_component(value))),
            None => (param.as_str(), QsValue::Null),
        };
        let key = decode_component(key);
        let merged = match result.get(&key) {
            None => value,
            Some(existing) => {
                let mut items = existing.clone().into_items();
                items.extend(value.into_items());
                QsValue::List(items)
            }
        };
        result.set(&key, merged);
    }
    result
}

/// `decode-uri-component` : `decodeURIComponent` puis, en cas d'échec, décodage morceau par morceau.
pub(crate) fn decode_component(input: &str) -> String {
    let units = utf16(&input.replace('+', " "));
    let decoded = decode_uri_component_utf16(&units).unwrap_or_else(|| custom_decode(units));
    String::from_utf16_lossy(&decoded)
}

fn custom_decode(mut input: Vec<u16>) -> Vec<u16> {
    let replacement_char = utf16("\u{FFFD}\u{FFFD}");
    let mut replacements: Vec<(Vec<u16>, Vec<u16>)> =
        vec![(utf16("%FE%FF"), replacement_char.clone()), (utf16("%FF%FE"), replacement_char)];
    for run in percent_runs(&input) {
        match decode_uri_component_utf16(&run) {
            Some(decoded) => set(run, decoded, &mut replacements),
            None => {
                let result = decode_fallback(run.clone());
                if result != run {
                    set(run, result, &mut replacements);
                }
            }
        }
    }
    set(utf16("%C2"), utf16("\u{FFFD}"), &mut replacements);
    for (key, value) in &replacements {
        input = replace_all(&input, key, value);
    }
    input
}

fn set(key: Vec<u16>, value: Vec<u16>, map: &mut Vec<(Vec<u16>, Vec<u16>)>) {
    match map.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => map.push((key, value)),
    }
}

fn is_percent_token(input: &[u16], i: usize) -> bool {
    let hex = |k: usize| input.get(k).is_some_and(|&u| u < 128 && (u as u8).is_ascii_hexdigit());
    input.get(i) == Some(&u16::from(b'%')) && hex(i + 1) && hex(i + 2)
}

fn percent_runs(input: &[u16]) -> Vec<Vec<u16>> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < input.len() {
        let start = i;
        while is_percent_token(input, i) {
            i += 3;
        }
        if i > start {
            runs.push(input[start..i].to_vec());
        } else {
            i += 1;
        }
    }
    runs
}

fn single_tokens(input: &[u16]) -> Vec<Vec<u16>> {
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < input.len() {
        if is_percent_token(input, i) {
            tokens.push(input[i..i + 3].to_vec());
            i += 3;
        } else {
            if input[i] != u16::from(b'%') {
                tokens.push(vec![input[i]]);
            }
            i += 1;
        }
    }
    tokens
}

fn decode_fallback(mut input: Vec<u16>) -> Vec<u16> {
    if let Some(decoded) = decode_uri_component_utf16(&input) {
        return decoded;
    }
    let mut tokens = single_tokens(&input);
    let mut i = 1;
    while i < tokens.len() {
        input = decode_components(&tokens, i).concat();
        tokens = single_tokens(&input);
        i += 1;
    }
    input
}

fn decode_components(components: &[Vec<u16>], split: usize) -> Vec<Vec<u16>> {
    if let Some(decoded) = decode_uri_component_utf16(&components.concat()) {
        return vec![decoded];
    }
    if components.len() == 1 {
        return components.to_vec();
    }
    let (left, right) = components.split_at(split.min(components.len()));
    let mut out = decode_components(left, 1);
    out.extend(decode_components(right, 1));
    out
}
