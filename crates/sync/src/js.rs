//! Sémantique JavaScript partagée par les portages de cURL, d'OpenAPI et du sérialiseur YAML : objets ordonnés,
//! nombres, chaînes, espaces, véracité, `JSON.stringify`, `decodeURIComponent` et motifs `$` de
//! `String.prototype.replace`.

use std::ops::Range;

use serde_json::{Map as JsonMap, Number, Value as Json};
use xc_core::yaml::json_string;

pub(crate) use xc_core::yaml::is_js_space;

/// Erreur levée là où le code JavaScript de Bruno lèverait une exception.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct JsError(pub String);

impl JsError {
    pub(crate) fn type_error(message: &str) -> Self {
        Self(format!("TypeError: {message}"))
    }
}

/// Objet JavaScript : clés d'index de tableau d'abord (ordre croissant), puis clés textuelles dans l'ordre
/// d'insertion. Un objet littéral (`{}`) ignore l'affectation de `__proto__`, pas un `Object.create(null)`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct JsObject<V> {
    entries: Vec<(String, V)>,
    null_proto: bool,
}

impl<V> JsObject<V> {
    pub(crate) fn new() -> Self {
        Self { entries: Vec::new(), null_proto: false }
    }

    pub(crate) fn null_proto() -> Self {
        Self { entries: Vec::new(), null_proto: true }
    }

    pub(crate) fn get(&self, key: &str) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub(crate) fn contains(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    pub(crate) fn set(&mut self, key: &str, value: V) {
        if !self.null_proto && key == "__proto__" {
            return;
        }
        if let Some(entry) = self.entries.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value;
            return;
        }
        let position = match array_index(key) {
            Some(index) => self.entries.iter().position(|(k, _)| array_index(k).is_none_or(|other| other > index)),
            None => None,
        };
        self.entries.insert(position.unwrap_or(self.entries.len()), (key.to_owned(), value));
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (&String, &V)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.iter_mut().map(|(_, v)| v)
    }

    pub(crate) fn to_json(&self, value: impl Fn(&V) -> Option<Json>) -> Json {
        let map: JsonMap<String, Json> = self.iter().filter_map(|(k, v)| value(v).map(|v| (k.clone(), v))).collect();
        Json::Object(map)
    }
}

impl<V> Default for JsObject<V> {
    fn default() -> Self {
        Self::new()
    }
}

/// Indice de tableau canonique (`"0"`, `"42"`), au sens des clés d'objet JavaScript.
pub(crate) fn array_index(key: &str) -> Option<usize> {
    let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()));
    key.parse::<u32>().ok().filter(|&n| canonical && n != u32::MAX).map(|n| n as usize)
}

/// Ordre des clés d'un objet JavaScript : indices croissants, puis ordre d'insertion.
pub(crate) fn js_order<K: AsRef<str>, T>(mut entries: Vec<(K, T)>) -> Vec<(K, T)> {
    if entries.iter().any(|(k, _)| array_index(k.as_ref()).is_some()) {
        entries.sort_by_key(|(k, _)| array_index(k.as_ref()).map_or((1, 0), |i| (0, i)));
    }
    entries
}

/// Propriétés d'un objet JSON dans l'ordre d'énumération JavaScript.
pub(crate) fn entries(object: &JsonMap<String, Json>) -> Vec<(&String, &Json)> {
    js_order(object.iter().collect())
}

pub(crate) fn trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `s.replace(/\s+/g, with)`.
pub(crate) fn collapse_spaces(s: &str, with: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if is_js_space(c) {
            if !in_run {
                out.push_str(with);
            }
            in_run = true;
        } else {
            out.push(c);
            in_run = false;
        }
    }
    out
}

/// Caractères que `.` ne reconnaît pas dans une expression régulière JavaScript.
pub(crate) fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

pub(crate) fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Longueur JavaScript (unités UTF-16).
pub(crate) fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

pub(crate) fn push_char(out: &mut Vec<u16>, c: char) {
    out.extend_from_slice(c.encode_utf16(&mut [0; 2]));
}

/// Véracité JavaScript d'une valeur JSON ; `None` représente `undefined`.
pub(crate) fn truthy<'a>(v: impl Into<Option<&'a Json>>) -> bool {
    match v.into() {
        None | Some(Json::Null) => false,
        Some(Json::Bool(b)) => *b,
        Some(Json::Number(n)) => n.as_f64() != Some(0.0),
        Some(Json::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `Number.prototype.toString()`.
pub(crate) fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".into();
    }
    if n == 0.0 {
        return "0".into();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.into();
    }
    let exp_form = format!("{:e}", n.abs());
    let (mantissa, exp) = exp_form.split_once('e').unwrap_or((&exp_form, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i64;
    let point = exp.parse::<i64>().unwrap_or(0) + 1;
    let body = if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let fraction = if k > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        let sign = if point > 0 { '+' } else { '-' };
        format!("{}{fraction}e{sign}{}", &digits[..1], (point - 1).abs())
    };
    if n < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

/// Nombre JSON tel que le relirait un analyseur JSON exact après `JSON.stringify(n)` :
/// entier si le texte JavaScript est un entier représentable, flottant sinon.
pub(crate) fn number_value(n: f64) -> Json {
    if !n.is_finite() {
        return Json::Null;
    }
    let text = number_to_string(n);
    if !text.contains(['.', 'e']) {
        if let Ok(i) = text.parse::<i64>() {
            return i.into();
        }
        if let Ok(u) = text.parse::<u64>() {
            return u.into();
        }
    }
    Number::from_f64(n).map_or(Json::Null, Json::Number)
}

/// Valeur d'une suite non vide de chiffres en base `radix`.
pub(crate) fn parse_radix(digits: &str, radix: u32) -> Option<f64> {
    let valid = !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix));
    valid.then(|| digits.chars().fold(0.0, |acc, c| acc * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0))))
}

/// `Number(s)` pour une chaîne.
pub(crate) fn string_to_number(s: &str) -> f64 {
    let t = trim(s);
    if t.is_empty() {
        return 0.0;
    }
    match t {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(rest) = t.strip_prefix(prefix) {
            return parse_radix(rest, radix).unwrap_or(f64::NAN);
        }
    }
    if is_decimal_literal(t) {
        t.parse().unwrap_or(f64::NAN)
    } else {
        f64::NAN
    }
}

fn is_decimal_literal(s: &str) -> bool {
    let b = s.trim_start_matches(['+', '-']);
    if b.len() + 1 < s.len() {
        return false;
    }
    let (mantissa, exponent) = match b.find(['e', 'E']) {
        Some(i) => (&b[..i], Some(&b[i + 1..])),
        None => (b, None),
    };
    let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |x: &str| x.bytes().all(|c| c.is_ascii_digit());
    let mantissa_ok = digits(int) && digits(frac) && !(int.is_empty() && frac.is_empty());
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
    });
    mantissa_ok && exponent_ok
}

/// `Number(x)` pour une valeur JSON ; `None` représente `undefined`.
pub(crate) fn to_number(v: Option<&Json>) -> f64 {
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

/// `String(x)` pour une valeur JSON définie.
pub(crate) fn string(v: &Json) -> String {
    match v {
        Json::Null => "null".into(),
        Json::Bool(b) => b.to_string(),
        Json::Number(n) => number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Json::String(s) => s.clone(),
        Json::Array(a) => {
            a.iter().map(|e| if e.is_null() { String::new() } else { string(e) }).collect::<Vec<_>>().join(",")
        }
        Json::Object(_) => "[object Object]".into(),
    }
}

/// `JSON.stringify(v)`, ou `JSON.stringify(v, null, 2)` quand `pretty`.
pub(crate) fn json_stringify(v: &Json, pretty: bool) -> String {
    let mut out = String::new();
    write_json(&mut out, v, pretty, 0);
    out
}

fn write_json(out: &mut String, v: &Json, pretty: bool, depth: usize) {
    match v {
        Json::Array(items) => {
            write_container(out, ['[', ']'], pretty, depth, items, |out, item| {
                write_json(out, item, pretty, depth + 1);
            });
        }
        Json::Object(object) => {
            write_container(out, ['{', '}'], pretty, depth, &entries(object), |out, (key, item)| {
                out.push_str(&json_string(key));
                out.push_str(if pretty { ": " } else { ":" });
                write_json(out, item, pretty, depth + 1);
            });
        }
        Json::String(s) => out.push_str(&json_string(s)),
        other => out.push_str(&string(other)),
    }
}

fn write_container<T>(
    out: &mut String,
    [open, close]: [char; 2],
    pretty: bool,
    depth: usize,
    items: &[T],
    write_item: impl Fn(&mut String, &T),
) {
    out.push(open);
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        if pretty {
            out.push('\n');
            out.push_str(&"  ".repeat(depth + 1));
        }
        write_item(out, item);
    }
    if pretty && !items.is_empty() {
        out.push('\n');
        out.push_str(&"  ".repeat(depth));
    }
    out.push(close);
}

/// `decodeURIComponent` ; `None` correspond à une `URIError`.
pub(crate) fn decode_uri_component(input: &str) -> Option<String> {
    decode_uri_component_utf16(&utf16(input)).map(|units| String::from_utf16_lossy(&units))
}

pub(crate) fn decode_uri_component_utf16(input: &[u16]) -> Option<Vec<u16>> {
    let mut out = Vec::with_capacity(input.len());
    let mut k = 0;
    while k < input.len() {
        if input[k] != u16::from(b'%') {
            out.push(input[k]);
            k += 1;
            continue;
        }
        let first = hex_byte(input, k)?;
        k += 3;
        if first < 0x80 {
            out.push(u16::from(first));
            continue;
        }
        let count = match first.leading_ones() {
            n @ 2..=4 => n as usize,
            _ => return None,
        };
        let mut bytes = vec![first];
        for _ in 1..count {
            if input.get(k) != Some(&u16::from(b'%')) {
                return None;
            }
            let byte = hex_byte(input, k)?;
            if byte & 0xC0 != 0x80 {
                return None;
            }
            bytes.push(byte);
            k += 3;
        }
        out.extend(std::str::from_utf8(&bytes).ok()?.encode_utf16());
    }
    Some(out)
}

fn hex_byte(input: &[u16], k: usize) -> Option<u8> {
    let digit = |i: usize| input.get(i).and_then(|&u| char::from_u32(u32::from(u))).and_then(|c| c.to_digit(16));
    Some((digit(k + 1)? * 16 + digit(k + 2)?) as u8)
}

/// `haystack.replaceAll(pattern, replacement)` avec un motif chaîne.
pub(crate) fn replace_all(haystack: &str, pattern: &str, replacement: &str) -> String {
    String::from_utf16_lossy(&replace_all_utf16(&utf16(haystack), &utf16(pattern), &utf16(replacement)))
}

/// [`replace_all`] sur des unités UTF-16, qui peuvent contenir des demi-paires de substitution.
pub(crate) fn replace_all_utf16(input: &[u16], pattern: &[u16], replacement: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(input.len());
    let mut last = 0;
    let mut i = 0;
    while i + pattern.len() <= input.len() {
        if input[i..i + pattern.len()] != *pattern {
            i += 1;
            continue;
        }
        out.extend_from_slice(&input[last..i]);
        expand_replacement(&mut out, replacement, input, i..i + pattern.len(), &[]);
        last = i + pattern.len();
        i += pattern.len().max(1);
    }
    out.extend_from_slice(&input[last..]);
    out
}

/// Écrit `replacement` avec les motifs de `String.prototype.replace` (`$$`, `$&`, `` $` ``, `$'`, `$n`, `$nn`) pour
/// la correspondance `matched` dans `input` ; `captures` contient les groupes de capture.
pub(crate) fn expand_replacement(
    out: &mut Vec<u16>,
    replacement: &[u16],
    input: &[u16],
    matched: Range<usize>,
    captures: &[&[u16]],
) {
    let ascii = |i: usize| replacement.get(i).and_then(|&u| u8::try_from(u).ok());
    let mut i = 0;
    while i < replacement.len() {
        if replacement[i] != u16::from(b'$') {
            out.push(replacement[i]);
            i += 1;
            continue;
        }
        let dollar = &replacement[i..=i];
        let (insert, len) = match ascii(i + 1) {
            Some(b'$') => (dollar, 2),
            Some(b'&') => (&input[matched.clone()], 2),
            Some(b'`') => (&input[..matched.start], 2),
            Some(b'\'') => (&input[matched.end..], 2),
            Some(d @ b'0'..=b'9') => {
                let one = usize::from(d - b'0');
                let two = ascii(i + 2).filter(u8::is_ascii_digit).map(|e| one * 10 + usize::from(e - b'0'));
                match (two, one) {
                    (Some(n), _) if (1..=captures.len()).contains(&n) => (captures[n - 1], 3),
                    (_, n) if (1..=captures.len()).contains(&n) => (captures[n - 1], 2),
                    _ => (dollar, 1),
                }
            }
            _ => (dollar, 1),
        };
        out.extend_from_slice(insert);
        i += len;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(units: &[u16]) -> String {
        String::from_utf16_lossy(units)
    }

    #[test]
    fn js_space_matches_ecmascript_whitespace() {
        let ecmascript = |c: char| {
            matches!(
                c,
                '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
                    ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
            )
        };
        assert!(('\0'..=char::MAX).all(|c| is_js_space(c) == ecmascript(c)));
        assert_eq!(trim("\u{feff} a b\u{85}\u{a0}"), "a b\u{85}");
    }

    #[test]
    fn array_index_is_canonical() {
        for (key, index) in [("0", Some(0)), ("42", Some(42)), ("4294967294", Some(4_294_967_294))] {
            assert_eq!(array_index(key), index, "{key}");
        }
        for key in ["", "01", "+1", "-1", "1.5", "a", "4294967295", "99999999999999999999"] {
            assert_eq!(array_index(key), None, "{key:?}");
        }
    }

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
            assert_eq!(number_to_string(n), expected, "{n}");
        }
    }

    #[test]
    fn ef_imp_01_number_to_string_matches_javascript() {
        for (n, s) in [
            (1.0, "1"),
            (-1.5, "-1.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (1.5e-7, "1.5e-7"),
            (0.000001, "0.000001"),
            (123456789012345680000.0, "123456789012345680000"),
            (0.1 + 0.2, "0.30000000000000004"),
        ] {
            assert_eq!(number_to_string(n), s);
        }
    }

    #[test]
    fn ef_imp_02_number_to_string_matches_javascript() {
        let cases = [
            (1.0, "1"),
            (-1.5, "-1.5"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (0.000001, "0.000001"),
            (0.0000001, "1e-7"),
            (1.2345e-10, "1.2345e-10"),
            (0.1 + 0.2, "0.30000000000000004"),
            (-0.0, "0"),
            (5e-324, "5e-324"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-Infinity"),
        ];
        for (n, expected) in cases {
            assert_eq!(number_to_string(n), expected, "{n:e}");
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
    fn ef_imp_02_string_to_number_matches_javascript() {
        assert_eq!(string_to_number(" 200 "), 200.0);
        assert_eq!(string_to_number(""), 0.0);
        assert_eq!(string_to_number("0x1F"), 31.0);
        assert_eq!(string_to_number("1e3"), 1000.0);
        assert_eq!(string_to_number("-Infinity"), f64::NEG_INFINITY);
        for nan in ["default", "2XX", "1_0", "inf", "+-1", "0x", ".", "0x+1", "-0x10"] {
            assert!(string_to_number(nan).is_nan(), "{nan}");
        }
    }

    #[test]
    fn ef_imp_01_object_orders_index_keys_first() {
        let mut o = JsObject::new();
        for k in ["b", "2", "a", "1", "01", "__proto__"] {
            o.set(k, ());
        }
        let keys: Vec<&String> = o.iter().map(|(k, _)| k).collect();
        assert_eq!(keys, ["1", "2", "b", "a", "01"]);
    }

    #[test]
    fn objects_enumerate_integer_keys_first() {
        let v: Json = serde_json::from_str(r#"{"b":1,"2":2,"a":3,"1":4,"01":5}"#).unwrap();
        assert_eq!(json_stringify(&v, true), "{\n  \"1\": 4,\n  \"2\": 2,\n  \"b\": 1,\n  \"a\": 3,\n  \"01\": 5\n}");
        assert_eq!(json_stringify(&v, false), r#"{"1":4,"2":2,"b":1,"a":3,"01":5}"#);
    }

    #[test]
    fn json_stringify_lays_out_containers_like_javascript() {
        let v: Json = serde_json::from_str(r#"{"a":[],"b":{},"c":[1,[2],{"d":"é\u0001"}]}"#).unwrap();
        assert_eq!(
            json_stringify(&v, true),
            "{\n  \"a\": [],\n  \"b\": {},\n  \"c\": [\n    1,\n    [\n      2\n    ],\n    {\n      \"d\": \"é\\u0001\"\n    }\n  ]\n}"
        );
        assert_eq!(json_stringify(&v, false), r#"{"a":[],"b":{},"c":[1,[2],{"d":"é\u0001"}]}"#);
    }

    #[test]
    fn truthy_follows_javascript() {
        let falsy = [Json::Null, Json::Bool(false), Json::from(0), Json::from(0.0), Json::from("")];
        let truthy_values =
            [Json::Bool(true), Json::from(-1), Json::from("0"), serde_json::json!([]), serde_json::json!({})];
        assert!(falsy.iter().all(|v| !truthy(v)));
        assert!(truthy_values.iter().all(truthy));
        assert!(!truthy(None));
    }

    #[test]
    fn ef_imp_01_decode_uri_component_matches_javascript() {
        assert_eq!(decode_uri_component("a%20b%C3%A9%F0%9F%98%80").as_deref(), Some("a bé😀"));
        for bad in ["%", "%2", "%zz", "%C3", "%C0%80", "%ED%A0%80", "%80", "%F8%80%80%80%80"] {
            assert_eq!(decode_uri_component(bad), None, "{bad}");
        }
    }

    #[test]
    fn ef_imp_01_replace_all_expands_dollar_patterns() {
        let out = replace_all_utf16(&utf16("x%24y"), &utf16("%24"), &utf16("[$&|$$|$`|$'|$1]"));
        assert_eq!(text(&out), "x[%24|$|x|y|$1]y");
    }

    #[test]
    fn ef_imp_02_replace_all_expands_dollar_patterns() {
        assert_eq!(replace_all("a{x}b{x}", "{x}", "[$&|$$|$1]"), "a[{x}|$|$1]b[{x}|$|$1]");
        assert_eq!(replace_all("ab", "b", "$`$'"), "aa");
        let mut out = Vec::new();
        expand_replacement(&mut out, &utf16("<$1$2$10$01>"), &utf16("xyz"), 0..1, &[&utf16("P")]);
        assert_eq!(text(&out), "<P$2P0P>");
    }

    #[test]
    fn replace_all_inserts_between_every_unit_for_an_empty_pattern() {
        assert_eq!(replace_all("abc", "", "-"), "-a-b-c-");
        assert_eq!(replace_all("", "", "-"), "-");
    }
}
