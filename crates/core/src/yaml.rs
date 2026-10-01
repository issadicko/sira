//! Arbre YAML ordonné et émetteur calqué sur celui de Bruno
//! (`yaml` 2.3.4, `lineWidth: 0`, `indent: 2`, `defaultStringType: PLAIN`).

use yaml_rust2::{Yaml, YamlLoader};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(String),
    Str(String),
    Seq(Vec<Value>),
    Map(Map),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Map(pub Vec<(String, Value)>);

#[derive(Debug, thiserror::Error)]
#[error("YAML invalide : {0}")]
pub struct YamlError(String);

impl Map {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let i = self.0.iter().position(|(k, _)| k == key)?;
        Some(self.0.remove(i).1)
    }

    /// Remplace la valeur en place, ou l'insère à sa position canonique selon `order`.
    pub fn set(&mut self, key: &str, value: Value, order: &[&str]) {
        if let Some(slot) = self.get_mut(key) {
            *slot = value;
            return;
        }
        let rank = |k: &str| order.iter().position(|o| *o == k);
        let at = rank(key).and_then(|mine| self.0.iter().position(|(k, _)| rank(k).is_some_and(|r| r > mine)));
        match at {
            Some(i) => self.0.insert(i, (key.to_owned(), value)),
            None => self.0.push((key.to_owned(), value)),
        }
    }

    pub fn map_mut_or_insert(&mut self, key: &str, order: &[&str]) -> &mut Map {
        if !matches!(self.get(key), Some(Value::Map(_))) {
            self.set(key, Value::Map(Map::default()), order);
        }
        match self.get_mut(key) {
            Some(Value::Map(m)) => m,
            _ => unreachable!(),
        }
    }

    pub fn str(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Value::as_str)
    }

    pub fn map(&self, key: &str) -> Option<&Map> {
        self.get(key).and_then(Value::as_map)
    }

    pub fn seq(&self, key: &str) -> &[Value] {
        self.get(key).and_then(Value::as_seq).unwrap_or(&[])
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Value {
    pub fn str(s: impl Into<String>) -> Self {
        Self::Str(s.into())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_map(&self) -> Option<&Map> {
        match self {
            Self::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_seq(&self) -> Option<&[Value]> {
        match self {
            Self::Seq(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    pub fn is_true(&self) -> bool {
        matches!(self, Self::Bool(true))
    }

    /// Représentation texte d'un scalaire, quel que soit son type YAML.
    pub fn scalar(&self) -> Option<String> {
        match self {
            Self::Str(s) | Self::Float(s) => Some(s.clone()),
            Self::Int(i) => Some(i.to_string()),
            Self::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }
}

pub fn parse(text: &str) -> Result<Value, YamlError> {
    let docs = YamlLoader::load_from_str(text).map_err(|e| YamlError(e.to_string()))?;
    match docs.into_iter().next() {
        Some(doc) => convert(doc),
        None => Ok(Value::Map(Map::default())),
    }
}

fn convert(y: Yaml) -> Result<Value, YamlError> {
    Ok(match y {
        Yaml::Null => Value::Null,
        Yaml::Boolean(b) => Value::Bool(b),
        Yaml::Integer(i) => Value::Int(i),
        Yaml::Real(s) => Value::Float(s),
        Yaml::String(s) => Value::Str(s),
        Yaml::Array(a) => Value::Seq(a.into_iter().map(convert).collect::<Result<_, _>>()?),
        Yaml::Hash(h) => {
            let mut map = Map::default();
            for (k, v) in h {
                let key = match convert(k)? {
                    Value::Str(s) => s,
                    other => other.scalar().unwrap_or_default(),
                };
                map.0.push((key, convert(v)?));
            }
            Value::Map(map)
        }
        Yaml::Alias(_) => return Err(YamlError("les alias ne sont pas pris en charge".into())),
        Yaml::BadValue => return Err(YamlError("valeur illisible".into())),
    })
}

/// Émet un document. `blank_before` liste les clés de premier niveau précédées d'une ligne vide.
pub fn emit(value: &Value, blank_before: &[&str]) -> String {
    let mut out = String::new();
    match value {
        Value::Map(m) if !m.is_empty() => {
            for (i, (k, v)) in m.0.iter().enumerate() {
                if i > 0 && blank_before.contains(&k.as_str()) {
                    out.push('\n');
                }
                out.push_str(&key(k));
                out.push(':');
                after_key(&mut out, v, 0);
            }
        }
        other => {
            out.push_str(&inline(other));
            out.push('\n');
        }
    }
    out
}

fn pad(out: &mut String, n: usize) {
    out.extend(std::iter::repeat_n(' ', n));
}

fn after_key(out: &mut String, v: &Value, indent: usize) {
    match v {
        Value::Map(m) if !m.is_empty() => {
            out.push('\n');
            map_block(out, m, indent + 2);
        }
        Value::Seq(s) if !s.is_empty() => {
            out.push('\n');
            seq_block(out, s, indent + 2);
        }
        Value::Str(s) if is_block(s) => {
            out.push(' ');
            block(out, s, indent + 2);
        }
        _ => {
            out.push(' ');
            out.push_str(&inline(v));
            out.push('\n');
        }
    }
}

fn map_block(out: &mut String, m: &Map, indent: usize) {
    for (k, v) in &m.0 {
        pad(out, indent);
        out.push_str(&key(k));
        out.push(':');
        after_key(out, v, indent);
    }
}

fn map_after_dash(out: &mut String, m: &Map, indent: usize) {
    for (i, (k, v)) in m.0.iter().enumerate() {
        if i > 0 {
            pad(out, indent);
        }
        out.push_str(&key(k));
        out.push(':');
        after_key(out, v, indent);
    }
}

fn seq_block(out: &mut String, s: &[Value], indent: usize) {
    for item in s {
        pad(out, indent);
        out.push_str("- ");
        match item {
            Value::Map(m) if !m.is_empty() => map_after_dash(out, m, indent + 2),
            Value::Seq(inner) if !inner.is_empty() => {
                out.push('\n');
                seq_block(out, inner, indent + 2);
            }
            Value::Str(text) if is_block(text) => block(out, text, indent + 2),
            other => {
                out.push_str(&inline(other));
                out.push('\n');
            }
        }
    }
}

fn is_block(s: &str) -> bool {
    s.contains('\n') && !s.contains('\r')
}

fn block(out: &mut String, s: &str, indent: usize) {
    let body = s.trim_end_matches('\n');
    let trailing = s.len() - body.len();
    out.push('|');
    if s.starts_with(' ') {
        out.push('2');
    }
    out.push_str(match trailing {
        0 => "-",
        1 => "",
        _ => "+",
    });
    out.push('\n');
    let content = if trailing > 1 { &s[..s.len() - 1] } else { body };
    for line in content.split('\n') {
        if !line.is_empty() {
            pad(out, indent);
            out.push_str(line);
        }
        out.push('\n');
    }
}

fn inline(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.clone(),
        Value::Str(s) => scalar(s),
        Value::Seq(_) => "[]".into(),
        Value::Map(_) => "{}".into(),
    }
}

fn key(k: &str) -> String {
    scalar(k)
}

fn scalar(s: &str) -> String {
    if plain_safe(s) {
        return s.to_owned();
    }
    if s.contains('"') && !s.contains('\'') && !s.chars().any(char::is_control) {
        return format!("'{s}'");
    }
    double_quoted(s)
}

fn double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\x{:02X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn plain_safe(s: &str) -> bool {
    let Some(first) = s.chars().next() else { return false };
    if s.starts_with(' ') || s.ends_with(' ') || s.chars().any(char::is_control) {
        return false;
    }
    if "[]{}#&*!|>'\"%@`,".contains(first) {
        return false;
    }
    if "-?:".contains(first) && s.chars().nth(1).is_none_or(|c| c == ' ') {
        return false;
    }
    if s.contains(": ") || s.ends_with(':') || s.contains(" #") {
        return false;
    }
    if s.starts_with("---") || s.starts_with("...") {
        return false;
    }
    !looks_like_non_string(s)
}

fn looks_like_non_string(s: &str) -> bool {
    const WORDS: [&str; 15] = [
        "null", "Null", "NULL", "~", "true", "True", "TRUE", "false", "False", "FALSE", ".nan", ".NaN", ".NAN", ".inf",
        ".Inf",
    ];
    if WORDS.contains(&s) || matches!(s.trim_start_matches(['+', '-']), ".inf" | ".Inf" | ".INF") {
        return true;
    }
    let digits = |t: &str, radix: u32| !t.is_empty() && t.chars().all(|c| c.is_digit(radix));
    if let Some(h) = s.strip_prefix("0x") {
        return digits(h, 16);
    }
    if let Some(o) = s.strip_prefix("0o") {
        return digits(o, 8);
    }
    is_float(s.trim_start_matches(['+', '-']))
}

fn is_float(s: &str) -> bool {
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let mantissa_ok = match mantissa.split_once('.') {
        Some((int, frac)) => {
            (!int.is_empty() || !frac.is_empty())
                && int.chars().all(|c| c.is_ascii_digit())
                && frac.chars().all(|c| c.is_ascii_digit())
        }
        None => !mantissa.is_empty() && mantissa.chars().all(|c| c.is_ascii_digit()),
    };
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.trim_start_matches(['+', '-']);
        !e.is_empty() && e.chars().all(|c| c.is_ascii_digit())
    });
    mantissa_ok && exponent_ok
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_quoted_scalars_match_bruno() {
        let cases = [
            ("Bearer {{t}}", "Bearer {{t}}"),
            ("1.0.0", "1.0.0"),
            ("yes", "yes"),
            ("a,b", "a,b"),
            ("?x", "?x"),
            ("https://x.test/a?b=1&c=2", "https://x.test/a?b=1&c=2"),
            ("{{x}}", "\"{{x}}\""),
            ("10", "\"10\""),
            ("true", "\"true\""),
            ("null", "\"null\""),
            ("1e3", "\"1e3\""),
            ("0x1F", "\"0x1F\""),
            ("~", "\"~\""),
            ("#x", "\"#x\""),
            ("@x", "\"@x\""),
            ("a: b", "\"a: b\""),
            ("x:", "\"x:\""),
            ("", "\"\""),
            (" lead", "\" lead\""),
            ("trail ", "\"trail \""),
            ("*ref", "\"*ref\""),
            ("[x]", "\"[x]\""),
            ("{\"a\":1}", "'{\"a\":1}'"),
            ("\tx", "\"\\tx\""),
            ("crlf\r\nline", "\"crlf\\r\\nline\""),
        ];
        for (input, expected) in cases {
            assert_eq!(scalar(input), expected, "scalaire {input:?}");
        }
    }

    #[test]
    fn block_literals_use_bruno_chomping() {
        let doc = |s: &str| emit(&Value::Map(Map(vec![("k".into(), Value::str(s))])), &[]);
        assert_eq!(doc("a\nb"), "k: |-\n  a\n  b\n");
        assert_eq!(doc("a\nb\n"), "k: |\n  a\n  b\n");
        assert_eq!(doc("a\n\nb"), "k: |-\n  a\n\n  b\n");
        assert_eq!(doc(" a\nb"), "k: |2-\n   a\n  b\n");
    }

    #[test]
    fn set_inserts_at_canonical_position() {
        let mut m = Map(vec![("method".into(), Value::str("GET")), ("auth".into(), Value::str("inherit"))]);
        m.set("url", Value::str("https://x.test"), &["method", "url", "headers", "params", "body", "auth"]);
        let keys: Vec<_> = m.0.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["method", "url", "auth"]);
    }
}
