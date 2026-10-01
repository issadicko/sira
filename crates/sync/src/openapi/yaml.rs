//! Chargement JSON ou YAML avec la sémantique de `js-yaml` 4 (`load`, schéma par défaut) :
//! résolution des scalaires du schéma core, clés converties par `String(x)`, fusion `<<`,
//! clés dupliquées refusées, dates non quotées réduites à `{}` comme après `resolveRefs`.

use std::collections::{HashMap, HashSet};

use serde_json::{Map, Value};
use yaml_rust2::parser::{Event, EventReceiver, Parser, Tag};
use yaml_rust2::scanner::TScalarStyle;

use super::{OpenApiError, R};
use crate::js::{js_order, number_value, parse_radix, string};

pub fn load(text: &str) -> R<Value> {
    let mut builder = Builder::default();
    Parser::new_from_str(text).load(&mut builder, true).map_err(|e| OpenApiError::Syntax(e.to_string()))?;
    if let Some(err) = builder.error {
        return Err(OpenApiError::Syntax(err));
    }
    match builder.docs.len() {
        0 if has_document(text) => Ok(Value::Null),
        0 => Err(OpenApiError::Empty),
        1 => Ok(builder.docs.remove(0)),
        _ => Err(OpenApiError::Syntax("un seul document YAML est attendu".into())),
    }
}

/// `js-yaml` lit un document (nul s'il n'a pas de contenu) dès que le texte, sans BOM ni
/// espaces de tête et terminé par un saut de ligne, compte plus d'un caractère.
fn has_document(text: &str) -> bool {
    let body = text.strip_prefix('\u{feff}').unwrap_or(text);
    let ends_with_break = body.is_empty() || body.ends_with(['\n', '\r']);
    let rest = body.trim_start_matches(' ');
    rest.chars().count() + usize::from(!ends_with_break) > 1
}

enum Key {
    Merge,
    Name(String),
}

enum Frame {
    Seq(Vec<Value>, usize),
    Map { map: Map<String, Value>, overridable: HashSet<String>, key: Option<Key>, anchor: usize },
}

const MAX_DEPTH: usize = 100;
const MAX_MERGED_KEYS: usize = 10_000;

#[derive(Default)]
struct Builder {
    stack: Vec<Frame>,
    anchors: HashMap<usize, Value>,
    docs: Vec<Value>,
    merged: usize,
    error: Option<String>,
}

impl EventReceiver for Builder {
    fn on_event(&mut self, ev: Event) {
        if self.error.is_some() {
            return;
        }
        if let Err(e) = self.handle(ev) {
            self.error = Some(e);
        }
    }
}

impl Builder {
    fn handle(&mut self, ev: Event) -> Result<(), String> {
        let opens_node =
            matches!(ev, Event::Scalar(..) | Event::SequenceStart(..) | Event::MappingStart(..) | Event::Alias(_));
        if opens_node && self.stack.len() >= MAX_DEPTH {
            return Err(format!("imbrication YAML au-delà de {MAX_DEPTH} niveaux"));
        }
        match ev {
            Event::Scalar(text, style, anchor, tag) => {
                if let Some(Frame::Map { key: key @ None, .. }) = self.stack.last_mut() {
                    let plain = matches!(style, TScalarStyle::Plain) && tag.is_none();
                    let value = scalar(&text, &style, tag.as_ref())?;
                    *key = Some(match value {
                        _ if plain && text == "<<" => Key::Merge,
                        Value::Object(_) => Key::Name(text),
                        ref v => Key::Name(string(v)),
                    });
                    if anchor > 0 {
                        self.anchors.insert(anchor, value);
                    }
                    return Ok(());
                }
                let value = scalar(&text, &style, tag.as_ref())?;
                self.push(value, anchor)
            }
            Event::SequenceStart(anchor, tag) => {
                collection_tag(tag.as_ref(), "seq")?;
                self.stack.push(Frame::Seq(Vec::new(), anchor));
                Ok(())
            }
            Event::MappingStart(anchor, tag) => {
                collection_tag(tag.as_ref(), "map")?;
                self.stack.push(Frame::Map { map: Map::new(), overridable: HashSet::new(), key: None, anchor });
                Ok(())
            }
            Event::SequenceEnd | Event::MappingEnd => match self.stack.pop() {
                Some(Frame::Seq(items, anchor)) => self.push(Value::Array(items), anchor),
                Some(Frame::Map { map, anchor, .. }) => self.push(ordered(map), anchor),
                None => Ok(()),
            },
            Event::Alias(id) => {
                let value = self.anchors.get(&id).cloned().ok_or("alias YAML inconnu ou récursif")?;
                self.push(value, 0)
            }
            _ => Ok(()),
        }
    }

    fn push(&mut self, value: Value, anchor: usize) -> Result<(), String> {
        if anchor > 0 {
            self.anchors.insert(anchor, value.clone());
        }
        match self.stack.last_mut() {
            None => self.docs.push(value),
            Some(Frame::Seq(items, _)) => items.push(value),
            Some(Frame::Map { map, overridable, key, .. }) => match key.take() {
                None => *key = Some(Key::Name(string(&value))),
                Some(Key::Merge) => self.merged += merge(map, overridable, value, MAX_MERGED_KEYS - self.merged)?,
                Some(Key::Name(k)) => {
                    if !overridable.remove(&k) && map.contains_key(&k) {
                        return Err(format!("clé dupliquée « {k} »"));
                    }
                    map.insert(k, value);
                }
            },
        }
        Ok(())
    }
}

/// Fusionne `<<` et renvoie le nombre de clés parcourues.
fn merge(
    map: &mut Map<String, Value>,
    overridable: &mut HashSet<String>,
    value: Value,
    budget: usize,
) -> Result<usize, String> {
    let sources = match value {
        Value::Array(items) => items,
        other => vec![other],
    };
    let mut seen = 0;
    for source in sources {
        let entries: Vec<(String, Value)> = match source {
            Value::Object(m) => m.into_iter().collect(),
            Value::Array(items) => items.into_iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
            _ => return Err("fusion « << » impossible : la source n'est pas une table".into()),
        };
        for (k, v) in entries {
            seen += 1;
            if seen > budget {
                return Err(format!("plus de {MAX_MERGED_KEYS} clés fusionnées"));
            }
            if !map.contains_key(&k) {
                overridable.insert(k.clone());
                map.insert(k, v);
            }
        }
    }
    Ok(seen)
}

fn ordered(map: Map<String, Value>) -> Value {
    Value::Object(js_order(map.into_iter().collect()).into_iter().collect())
}

fn is_non_specific(tag: &Tag) -> bool {
    matches!((tag.handle.as_str(), tag.suffix.as_str()), ("!", "") | ("", "!"))
}

fn collection_tag(tag: Option<&Tag>, kind: &str) -> Result<(), String> {
    match tag {
        None => Ok(()),
        Some(t) if t.handle == "tag:yaml.org,2002:" && t.suffix == kind => Ok(()),
        Some(t) if is_non_specific(t) => Ok(()),
        Some(t) => Err(format!("étiquette YAML non prise en charge : {}{}", t.handle, t.suffix)),
    }
}

fn scalar(text: &str, style: &TScalarStyle, tag: Option<&Tag>) -> Result<Value, String> {
    let Some(tag) = tag else {
        return Ok(match style {
            TScalarStyle::Plain => implicit(text),
            _ => Value::String(text.to_owned()),
        });
    };
    let explicit =
        |v: Option<Value>| v.ok_or_else(|| format!("« {text} » ne correspond pas à l'étiquette {}", tag.suffix));
    match (tag.handle.as_str(), tag.suffix.as_str()) {
        _ if is_non_specific(tag) => Ok(Value::String(text.to_owned())),
        ("tag:yaml.org,2002:", "str") => Ok(Value::String(text.to_owned())),
        ("tag:yaml.org,2002:", "null") => explicit(is_null(text).then_some(Value::Null)),
        ("tag:yaml.org,2002:", "bool") => explicit(boolean(text).map(Value::Bool)),
        ("tag:yaml.org,2002:", "int") => explicit(integer(text).map(number_value)),
        ("tag:yaml.org,2002:", "float") => explicit(float(text).map(number_value)),
        ("tag:yaml.org,2002:", "timestamp") => explicit(is_timestamp(text).then(|| Value::Object(Map::new()))),
        (handle, suffix) => Err(format!("étiquette YAML non prise en charge : {handle}{suffix}")),
    }
}

fn implicit(text: &str) -> Value {
    if is_null(text) {
        return Value::Null;
    }
    if let Some(b) = boolean(text) {
        return Value::Bool(b);
    }
    if let Some(n) = integer(text).or_else(|| float(text)) {
        return number_value(n);
    }
    if is_timestamp(text) {
        return Value::Object(Map::new());
    }
    Value::String(text.to_owned())
}

fn is_null(text: &str) -> bool {
    matches!(text, "" | "~" | "null" | "Null" | "NULL")
}

fn boolean(text: &str) -> Option<bool> {
    match text {
        "true" | "True" | "TRUE" => Some(true),
        "false" | "False" | "FALSE" => Some(false),
        _ => None,
    }
}

fn split_sign(text: &str) -> (f64, &str) {
    match text.as_bytes().first() {
        Some(b'-') => (-1.0, &text[1..]),
        Some(b'+') => (1.0, &text[1..]),
        _ => (1.0, text),
    }
}

fn digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

fn integer(text: &str) -> Option<f64> {
    let (sign, body) = split_sign(text);
    if body == "0" {
        return Some(0.0);
    }
    let value = match body.strip_prefix('0').and_then(|r| r.chars().next().map(|c| (c, &r[1..]))) {
        Some(('b', rest)) => parse_radix(rest, 2)?,
        Some(('x', rest)) => parse_radix(rest, 16)?,
        Some(('o', rest)) => parse_radix(rest, 8)?,
        _ if digits(body) => body.parse::<f64>().ok()?,
        _ => return None,
    };
    value.is_finite().then_some(sign * value)
}

fn float(text: &str) -> Option<f64> {
    let (sign, unsigned) = split_sign(text);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") {
        return Some(sign * f64::INFINITY);
    }
    if matches!(text, ".nan" | ".NaN" | ".NAN") {
        return Some(f64::NAN);
    }
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(i) => (&unsigned[..i], Some(&unsigned[i + 1..])),
        None => (unsigned, None),
    };
    let exponent_ok = exponent.is_none_or(|e| digits(e.strip_prefix(['-', '+']).unwrap_or(e)));
    let mantissa_ok = match mantissa.split_once('.') {
        Some(("", frac)) => unsigned.len() == text.len() && digits(frac),
        Some((int, frac)) => digits(int) && frac.bytes().all(|b| b.is_ascii_digit()),
        None => digits(mantissa),
    };
    if !(mantissa_ok && exponent_ok) {
        return None;
    }
    let value = sign * unsigned.to_lowercase().parse::<f64>().ok()?;
    value.is_finite().then_some(value)
}

fn is_timestamp(text: &str) -> bool {
    let b = text.as_bytes();
    let digit = |i: usize| b.get(i).is_some_and(u8::is_ascii_digit);
    let date_only = b.len() == 10
        && (0..4).all(digit)
        && b[4] == b'-'
        && digit(5)
        && digit(6)
        && b[7] == b'-'
        && digit(8)
        && digit(9);
    date_only || is_datetime(text)
}

fn is_datetime(text: &str) -> bool {
    let mut cur = Cursor { b: text.as_bytes(), i: 0 };
    let date = cur.digits(4, 4) && cur.eat(b'-') && cur.digits(1, 2) && cur.eat(b'-') && cur.digits(1, 2);
    let sep = cur.eat(b'T') || cur.eat(b't') || cur.blanks() > 0;
    let time = cur.digits(1, 2) && cur.eat(b':') && cur.digits(2, 2) && cur.eat(b':') && cur.digits(2, 2);
    if !(date && sep && time) {
        return false;
    }
    if cur.eat(b'.') {
        cur.digits(0, usize::MAX);
    }
    let checkpoint = cur.i;
    cur.blanks();
    if cur.eat(b'Z') {
        return cur.done();
    }
    if cur.eat(b'-') || cur.eat(b'+') {
        let zone = cur.digits(1, 2) && (!cur.eat(b':') || cur.digits(2, 2));
        return zone && cur.done();
    }
    cur.i = checkpoint;
    cur.done()
}

struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
}

impl Cursor<'_> {
    fn eat(&mut self, c: u8) -> bool {
        let ok = self.b.get(self.i) == Some(&c);
        self.i += usize::from(ok);
        ok
    }

    fn digits(&mut self, min: usize, max: usize) -> bool {
        let n = self.b[self.i..].iter().take(max).take_while(|c| c.is_ascii_digit()).count();
        self.i += n;
        n >= min
    }

    fn blanks(&mut self) -> usize {
        let n = self.b[self.i..].iter().take_while(|c| **c == b' ' || **c == b'\t').count();
        self.i += n;
        n
    }

    fn done(&self) -> bool {
        self.i == self.b.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ef_imp_02_yaml_scalars_follow_js_yaml_core_schema() {
        let doc = load(
            "a: 0x1F\nb: 1_000\nc: 012\nd: 1.0\ne: .5\nf: yes\ng: ~\nh: 2017-07-21\ni: '2017-07-21'\nj: 1e3\nk: +12\nl: 0o17\n\
             m: 2001-12-14t21:59:43.10-05:00\nn: TRUE\no: 1.\np: -0\nq: \"1\"\nr: -.5\ns: 1e400\nt: !!int \"42\"\n",
        )
        .unwrap();
        assert_eq!(
            doc,
            json!({"a": 31, "b": "1_000", "c": 12, "d": 1, "e": 0.5, "f": "yes", "g": null, "h": {}, "i": "2017-07-21",
                   "j": 1000, "k": 12, "l": 15, "m": {}, "n": true, "o": 1, "p": 0, "q": "1", "r": "-.5", "s": "1e400",
                   "t": 42})
        );
    }

    #[test]
    fn ef_imp_02_yaml_reads_json_like_js_yaml() {
        let doc =
            load("{\n\t\"a\": {\n\t\t\"b\": \"x\\/y\\u00e9\", \"c\": -0, \"d\": 1E2, \"e\": 1.5e3\n\t}\n}").unwrap();
        assert_eq!(doc, json!({"a": {"b": "x/yé", "c": 0, "d": 100, "e": 1500}}));
        assert_eq!(load("a: ! 12\nb: 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF\n").unwrap()["a"], json!("12"));
        assert_eq!(load("m:\n  <<: [[a, b]]\n  c: 1\n").unwrap(), json!({"m": {"0": "a", "1": "b", "c": 1}}));
    }

    #[test]
    fn ef_imp_02_yaml_limits_nesting_like_js_yaml() {
        let nested = |n: usize| (0..n).map(|i| format!("{}a:", "  ".repeat(i))).collect::<Vec<_>>().join("\n") + " 1\n";
        assert!(load(&nested(99)).is_ok());
        assert!(matches!(load(&nested(100)), Err(OpenApiError::Syntax(_))));
    }

    #[test]
    fn ef_imp_02_yaml_keys_are_stringified_and_ordered_like_javascript() {
        let doc = load("default: x\n404: y\n'200': z\n1.0: w\ntrue: v\n").unwrap();
        let keys: Vec<_> = doc.as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["1", "200", "404", "default", "true"]);
    }

    #[test]
    fn ef_imp_02_yaml_merge_keys_and_duplicates() {
        let doc = load("base: &b {x: 1, y: 2}\nm:\n  <<: *b\n  y: 3\n  z: 4\n").unwrap();
        assert_eq!(doc["m"], json!({"x": 1, "y": 3, "z": 4}));
        assert!(matches!(load("a: 1\na: 2\n"), Err(OpenApiError::Syntax(_))));
        assert!(matches!(load("m:\n  <<: {x: 1}\n  x: 2\n  x: 3\n"), Err(OpenApiError::Syntax(_))));
    }

    #[test]
    fn ef_imp_02_yaml_rejects_empty_and_multiple_documents() {
        assert!(matches!(load(""), Err(OpenApiError::Empty)));
        assert!(matches!(load("  \n"), Err(OpenApiError::Empty)));
        assert_eq!(load("# rien\n").unwrap(), Value::Null);
        assert!(matches!(load("a: 1\n---\nb: 2\n"), Err(OpenApiError::Syntax(_))));
        assert!(matches!(load("a: [1"), Err(OpenApiError::Syntax(_))));
    }
}
