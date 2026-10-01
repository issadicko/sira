use serde::Serialize;
use serde_json::Value as Json;

use crate::request::Assertion;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionResult {
    pub expression: String,
    pub operator: String,
    pub expected: Option<String>,
    pub actual: String,
    pub passed: bool,
    pub error: Option<String>,
}

pub struct ResponseView<'a> {
    pub status: u16,
    pub headers: &'a [(String, String)],
    pub body: &'a [u8],
}

/// Évalue les assertions déclaratives de Bruno (`res.status eq 200`, `res.body.id isString`, …).
pub fn evaluate(assertions: &[Assertion], res: &ResponseView) -> Vec<AssertionResult> {
    let body: Option<Json> = serde_json::from_slice(res.body).ok();
    assertions
        .iter()
        .filter(|a| a.enabled)
        .map(|a| {
            let actual = lookup(&a.expression, res, body.as_ref());
            let outcome = check(&a.operator, actual.as_ref(), a.value.as_deref());
            AssertionResult {
                expression: a.expression.clone(),
                operator: a.operator.clone(),
                expected: a.value.clone(),
                actual: actual.map(|v| v.to_string()).unwrap_or_else(|| "undefined".into()),
                passed: outcome.as_ref().is_ok_and(|ok| *ok),
                error: outcome.err(),
            }
        })
        .collect()
}

fn lookup(expression: &str, res: &ResponseView, body: Option<&Json>) -> Option<Json> {
    let expr = expression.trim();
    if expr == "res.status" || expr == "res.getStatus()" {
        return Some(Json::from(res.status));
    }
    if let Some(name) = expr.strip_prefix("res.headers.") {
        let name = name.trim_matches(['[', ']', '\'', '"']);
        return res.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| Json::from(v.as_str()));
    }
    let path = expr.strip_prefix("res.body").or_else(|| expr.strip_prefix("res.getBody()"))?;
    let mut cur = match body {
        Some(b) => b.clone(),
        None => Json::from(String::from_utf8_lossy(res.body).into_owned()),
    };
    for part in path.split(['.', '[']).filter(|p| !p.is_empty()) {
        let part = part.trim_end_matches(']').trim_matches(['\'', '"']);
        cur = match part.parse::<usize>() {
            Ok(i) if cur.is_array() => cur.get(i)?.clone(),
            _ => cur.get(part)?.clone(),
        };
    }
    Some(cur)
}

fn expected(raw: &str) -> Json {
    let raw = raw.trim();
    if let Some(s) = raw
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| raw.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
    {
        return Json::from(s);
    }
    serde_json::from_str(raw).unwrap_or_else(|_| Json::from(raw))
}

fn loose_eq(a: &Json, b: &Json) -> bool {
    match (a, b) {
        (Json::Number(x), Json::Number(y)) => x.as_f64() == y.as_f64(),
        (Json::String(x), other) | (other, Json::String(x)) if !other.is_string() => {
            x.as_str() == other.to_string().as_str()
        }
        _ => a == b,
    }
}

fn number(v: Option<&Json>) -> Option<f64> {
    match v? {
        Json::Number(n) => n.as_f64(),
        Json::String(s) => s.parse().ok(),
        _ => None,
    }
}

fn check(op: &str, actual: Option<&Json>, value: Option<&str>) -> Result<bool, String> {
    let want = value.map(expected);
    let need = || want.clone().ok_or_else(|| format!("l'opérateur {op} attend une valeur"));
    let compare = |f: fn(f64, f64) -> bool| -> Result<bool, String> {
        let w = need()?;
        Ok(matches!((number(actual), number(Some(&w))), (Some(a), Some(b)) if f(a, b)))
    };
    let truthy = |v: Option<&Json>| match v {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Number(n)) => n.as_f64() != Some(0.0),
        Some(Json::String(s)) => !s.is_empty(),
        Some(_) => true,
    };
    Ok(match op {
        "eq" => {
            let w = need()?;
            actual.is_some_and(|a| loose_eq(a, &w))
        }
        "neq" => {
            let w = need()?;
            !actual.is_some_and(|a| loose_eq(a, &w))
        }
        "gt" => compare(|a, b| a > b)?,
        "gte" => compare(|a, b| a >= b)?,
        "lt" => compare(|a, b| a < b)?,
        "lte" => compare(|a, b| a <= b)?,
        "contains" | "notContains" => {
            let w = need()?;
            let needle = w.as_str().map(str::to_owned).unwrap_or_else(|| w.to_string());
            let found = match actual {
                Some(Json::String(s)) => s.contains(&needle),
                Some(Json::Array(items)) => items.iter().any(|i| loose_eq(i, &w)),
                _ => false,
            };
            found == (op == "contains")
        }
        "isNumber" => matches!(actual, Some(Json::Number(_))),
        "isString" => matches!(actual, Some(Json::String(_))),
        "isBoolean" => matches!(actual, Some(Json::Bool(_))),
        "isArray" => matches!(actual, Some(Json::Array(_))),
        "isJson" => matches!(actual, Some(Json::Object(_) | Json::Array(_))),
        "isNull" => matches!(actual, Some(Json::Null)),
        "isDefined" => actual.is_some(),
        "isUndefined" => actual.is_none(),
        "isTruthy" => truthy(actual),
        "isFalsy" => !truthy(actual),
        "isEmpty" => match actual {
            Some(Json::String(s)) => s.is_empty(),
            Some(Json::Array(a)) => a.is_empty(),
            Some(Json::Object(o)) => o.is_empty(),
            _ => false,
        },
        other => return Err(format!("opérateur non pris en charge : {other}")),
    })
}
