//! Sémantique JavaScript utilisée par le sérialiseur de Bruno : véracité, `||`, `String()`,
//! `Number()`, `JSON.stringify` et ordre des propriétés d'objet.

use serde_json::{Map as JsonMap, Value as Json};
use xc_core::yaml::{is_js_space, json_string, Map, Value};

pub fn get<'a>(v: Option<&'a Json>, key: &str) -> Option<&'a Json> {
    v.and_then(|v| v.get(key))
}

pub fn truthy(v: Option<&Json>) -> bool {
    match v {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Number(n)) => n.as_f64() != Some(0.0),
        Some(Json::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `x?.length` est vrai.
pub fn has_length(v: Option<&Json>) -> bool {
    match v {
        Some(Json::Array(a)) => !a.is_empty(),
        Some(Json::String(s)) => !s.is_empty(),
        _ => false,
    }
}

pub fn non_empty_array(v: Option<&Json>) -> Option<&Vec<Json>> {
    v.and_then(Json::as_array).filter(|a| !a.is_empty())
}

/// `isNonEmptyString(x)` : la chaîne d'origine si elle contient autre chose que des blancs.
pub fn non_empty(v: Option<&Json>) -> Option<&str> {
    v.and_then(Json::as_str).filter(|s| !trim(s).is_empty())
}

pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

pub fn is_false(v: Option<&Json>) -> bool {
    v == Some(&Json::Bool(false))
}

pub fn is_true(v: Option<&Json>) -> bool {
    v == Some(&Json::Bool(true))
}

/// `x || fallback`.
pub fn or(v: Option<&Json>, fallback: Value) -> Value {
    match v {
        Some(v) if truthy(Some(v)) => yaml(v),
        _ => fallback,
    }
}

/// `x ?? fallback`.
pub fn nullish(v: Option<&Json>, fallback: Value) -> Value {
    match v {
        None | Some(Json::Null) => fallback,
        Some(v) => yaml(v),
    }
}

pub fn yaml(v: &Json) -> Value {
    match v {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::Float(number(n.as_f64().unwrap_or(0.0))),
        Json::String(s) => Value::str(s),
        Json::Array(a) => Value::Seq(a.iter().map(yaml).collect()),
        Json::Object(o) => Value::Map(Map(entries(o).into_iter().map(|(k, v)| (k.clone(), yaml(v))).collect())),
    }
}

/// Propriétés dans l'ordre d'énumération JavaScript : indices entiers croissants, puis ordre d'insertion.
fn entries(o: &JsonMap<String, Json>) -> Vec<(&String, &Json)> {
    let index = |k: &str| {
        k.parse::<u32>().ok().filter(|i| *i < u32::MAX && (k == "0" || !k.starts_with('0')) && !k.starts_with('+'))
    };
    let mut indexed: Vec<_> = o.iter().filter_map(|(k, v)| index(k).map(|i| (i, k, v))).collect();
    indexed.sort_by_key(|(i, _, _)| *i);
    let named = o.iter().filter(|(k, _)| index(k).is_none());
    indexed.into_iter().map(|(_, k, v)| (k, v)).chain(named).collect()
}

/// `Number.prototype.toString()` pour un nombre fini.
pub fn number(f: f64) -> String {
    if f == 0.0 {
        return "0".into();
    }
    let sci = format!("{:e}", f.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits = mantissa.replace('.', "");
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap_or(0) + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat(-n as usize))
    } else {
        let point = if k > 1 { format!("{}.{}", &digits[..1], &digits[1..]) } else { digits };
        format!("{point}e{}{}", if n > 0 { "+" } else { "-" }, (n - 1).abs())
    };
    if f < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

/// `String(x)` pour une valeur définie.
pub fn string(v: &Json) -> String {
    match v {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => number(n.as_f64().unwrap_or(0.0)),
        Json::String(s) => s.clone(),
        Json::Array(a) => {
            a.iter().map(|e| if e.is_null() { String::new() } else { string(e) }).collect::<Vec<_>>().join(",")
        }
        Json::Object(_) => "[object Object]".into(),
    }
}

/// `ensureString(x)` : chaîne vide pour `null` et `undefined`.
pub fn ensure_string(v: Option<&Json>) -> String {
    match v {
        None | Some(Json::Null) => String::new(),
        Some(v) => string(v),
    }
}

/// `Number(x)`.
pub fn to_number(v: Option<&Json>) -> f64 {
    match v {
        None => f64::NAN,
        Some(Json::Null) => 0.0,
        Some(Json::Bool(b)) => f64::from(u8::from(*b)),
        Some(Json::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Json::String(s)) => string_to_number(s),
        Some(a @ Json::Array(_)) => string_to_number(&string(a)),
        Some(Json::Object(_)) => f64::NAN,
    }
}

fn string_to_number(s: &str) -> f64 {
    let s = trim(s);
    if s.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(d) = s.strip_prefix(prefix) {
            return u64::from_str_radix(d, radix).map_or(f64::NAN, |n| n as f64);
        }
    }
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if unsigned == "Infinity" {
        return if s.starts_with('-') { f64::NEG_INFINITY } else { f64::INFINITY };
    }
    let decimal = unsigned.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-'));
    if decimal && unsigned.starts_with(|c: char| c.is_ascii_digit() || c == '.') {
        s.parse().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

/// `JSON.stringify(x, null, 2)`.
pub fn json_pretty(v: &Json) -> String {
    let mut out = String::new();
    write_json(&mut out, v, 0);
    out
}

fn write_json(out: &mut String, v: &Json, depth: usize) {
    let close = |out: &mut String, c: char| {
        out.push('\n');
        out.push_str(&"  ".repeat(depth));
        out.push(c);
    };
    let open = |out: &mut String, i: usize| {
        out.push_str(if i == 0 { "\n" } else { ",\n" });
        out.push_str(&"  ".repeat(depth + 1));
    };
    match v {
        Json::Array(a) if !a.is_empty() => {
            out.push('[');
            for (i, e) in a.iter().enumerate() {
                open(out, i);
                write_json(out, e, depth + 1);
            }
            close(out, ']');
        }
        Json::Object(o) if !o.is_empty() => {
            out.push('{');
            for (i, (k, e)) in entries(o).into_iter().enumerate() {
                open(out, i);
                out.push_str(&json_string(k));
                out.push_str(": ");
                write_json(out, e, depth + 1);
            }
            close(out, '}');
        }
        Json::Array(_) => out.push_str("[]"),
        Json::Object(_) => out.push_str("{}"),
        Json::String(s) => out.push_str(&json_string(s)),
        other => out.push_str(&string(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_print_like_javascript() {
        let cases = [
            (1.0, "1"),
            (-0.0, "0"),
            (1.5, "1.5"),
            (0.1, "0.1"),
            (123456789.0, "123456789"),
            (1e21, "1e+21"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (-2.5e30, "-2.5e+30"),
            (100.0, "100"),
        ];
        for (n, expected) in cases {
            assert_eq!(number(n), expected, "{n}");
        }
    }

    #[test]
    fn number_conversion_follows_javascript() {
        let cases = [("200", 200.0), (" 12 ", 12.0), ("0x10", 16.0), ("1e2", 100.0), ("", 0.0)];
        for (s, expected) in cases {
            assert_eq!(to_number(Some(&Json::from(s))), expected, "{s:?}");
        }
        assert!(to_number(Some(&Json::from("inf"))).is_nan());
        assert!(to_number(Some(&Json::from("abc"))).is_nan());
    }

    #[test]
    fn objects_enumerate_integer_keys_first() {
        let v: Json = serde_json::from_str(r#"{"b":1,"2":2,"a":3,"1":4,"01":5}"#).unwrap();
        assert_eq!(json_pretty(&v), "{\n  \"1\": 4,\n  \"2\": 2,\n  \"b\": 1,\n  \"a\": 3,\n  \"01\": 5\n}");
    }
}
