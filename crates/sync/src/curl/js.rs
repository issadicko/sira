//! Sémantique JavaScript nécessaire au port : objets ordonnés, espaces, `decodeURIComponent`, motifs `$` de
//! `String.prototype.replace`.

use serde_json::{Map, Value};

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

    pub(crate) fn to_json(&self, value: impl Fn(&V) -> Option<Value>) -> Value {
        let map: Map<String, Value> = self.iter().filter_map(|(k, v)| value(v).map(|v| (k.clone(), v))).collect();
        Value::Object(map)
    }
}

impl<V> Default for JsObject<V> {
    fn default() -> Self {
        Self::new()
    }
}

fn array_index(key: &str) -> Option<u32> {
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_digit()) || (key.len() > 1 && key.starts_with('0')) {
        return None;
    }
    key.parse::<u32>().ok().filter(|&n| n != u32::MAX)
}

/// `\s` des expressions régulières JavaScript (et ensemble retiré par `String.prototype.trim`).
pub(crate) fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

/// Caractères que `.` ne reconnaît pas dans une expression régulière JavaScript.
pub(crate) fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

pub(crate) fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

/// Longueur JavaScript (unités UTF-16).
pub(crate) fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

/// `decodeURIComponent` ; `None` correspond à une `URIError`.
pub(crate) fn decode_uri_component(input: &str) -> Option<String> {
    let units: Vec<u16> = input.encode_utf16().collect();
    decode_uri_component_utf16(&units).map(|units| String::from_utf16_lossy(&units))
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

/// Remplacement de chaque occurrence littérale de `pattern`, avec l'interprétation JavaScript des motifs
/// `$$`, `$&`, `` $` `` et `$'` dans `replacement` (sans groupes de capture).
pub(crate) fn replace_all(input: &[u16], pattern: &[u16], replacement: &[u16]) -> Vec<u16> {
    if pattern.is_empty() {
        return input.to_vec();
    }
    let mut out = Vec::with_capacity(input.len());
    let mut last = 0;
    let mut i = 0;
    while i + pattern.len() <= input.len() {
        if &input[i..i + pattern.len()] != pattern {
            i += 1;
            continue;
        }
        out.extend_from_slice(&input[last..i]);
        substitute(&mut out, replacement, input, i, i + pattern.len());
        i += pattern.len();
        last = i;
    }
    out.extend_from_slice(&input[last..]);
    out
}

fn substitute(out: &mut Vec<u16>, replacement: &[u16], input: &[u16], start: usize, end: usize) {
    let dollar = u16::from(b'$');
    let mut i = 0;
    while i < replacement.len() {
        let next = replacement.get(i + 1).copied();
        if replacement[i] == dollar {
            let insert: Option<&[u16]> = match next.and_then(|u| u8::try_from(u).ok()) {
                Some(b'$') => Some(&replacement[i..i + 1]),
                Some(b'&') => Some(&input[start..end]),
                Some(b'`') => Some(&input[..start]),
                Some(b'\'') => Some(&input[end..]),
                _ => None,
            };
            if let Some(insert) = insert {
                out.extend_from_slice(insert);
                i += 2;
                continue;
            }
        }
        out.push(replacement[i]);
        i += 1;
    }
}

pub(crate) fn utf16(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

/// Valeur de vérité JavaScript d'une valeur JSON.
pub(crate) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ef_imp_01_decode_uri_component_matches_javascript() {
        assert_eq!(decode_uri_component("a%20b%C3%A9%F0%9F%98%80").as_deref(), Some("a bé😀"));
        for bad in ["%", "%2", "%zz", "%C3", "%C0%80", "%ED%A0%80", "%80", "%F8%80%80%80%80"] {
            assert_eq!(decode_uri_component(bad), None, "{bad}");
        }
    }

    #[test]
    fn ef_imp_01_replace_all_expands_dollar_patterns() {
        let out = replace_all(&utf16("x%24y"), &utf16("%24"), &utf16("[$&|$$|$`|$'|$1]"));
        assert_eq!(String::from_utf16_lossy(&out), "x[%24|$|x|y|$1]y");
    }
}
