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

    /// Copie dont les clés de chaque table sont triées : deux arbres équivalents donnent la même copie.
    pub fn sorted(&self) -> Self {
        match self {
            Self::Map(m) => {
                let mut entries: Vec<_> = m.0.iter().map(|(k, v)| (k.clone(), v.sorted())).collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                Self::Map(Map(entries))
            }
            Self::Seq(items) => Self::Seq(items.iter().map(Self::sorted).collect()),
            other => other.clone(),
        }
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

fn load(text: &str) -> Result<Vec<Yaml>, YamlError> {
    YamlLoader::load_from_str(text).map_err(|e| YamlError(e.to_string()))
}

fn first_document(documents: Vec<Yaml>) -> Result<Value, YamlError> {
    match documents.into_iter().next() {
        Some(doc) => convert(doc),
        None => Ok(Value::Map(Map::default())),
    }
}

pub fn parse(text: &str) -> Result<Value, YamlError> {
    first_document(load(text)?)
}

/// Comme [`parse`], mais refuse un flux de plusieurs documents, dont la réécriture perdrait les suivants.
pub fn parse_single(text: &str) -> Result<Value, YamlError> {
    let documents = load(text)?;
    if documents.len() > 1 {
        return Err(YamlError("le fichier contient plusieurs documents YAML".into()));
    }
    first_document(documents)
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

/// Émet un document. `blank_before` liste les clés de premier niveau précédées d'une ligne vide,
/// sauf si la ligne précédente est déjà blanche (comme le post-traitement de Bruno).
pub fn emit(value: &Value, blank_before: &[&str]) -> String {
    let mut out = String::new();
    match value {
        Value::Map(m) if !m.is_empty() => {
            for (i, (k, v)) in m.0.iter().enumerate() {
                if i > 0 && blank_before.contains(&k.as_str()) && !ends_with_blank_line(&out) {
                    out.push('\n');
                }
                out.push_str(&key(k));
                out.push(':');
                after_key(&mut out, v, 0);
            }
        }
        other => {
            out.push_str(&inline(other, 0));
            out.push('\n');
        }
    }
    out
}

fn ends_with_blank_line(out: &str) -> bool {
    let body = out.strip_suffix('\n').unwrap_or(out);
    body.rsplit('\n').next().is_some_and(|line| line.trim().is_empty())
}

fn pad(out: &mut String, n: usize) {
    out.extend(std::iter::repeat_n(' ', n));
}

fn after_key(out: &mut String, v: &Value, indent: usize) {
    match v {
        Value::Map(m) if !m.is_empty() => {
            out.push('\n');
            map_block(out, m, indent + 2, false);
        }
        Value::Seq(s) if !s.is_empty() => {
            out.push('\n');
            seq_block(out, s, indent + 2, false);
        }
        _ => {
            out.push(' ');
            out.push_str(&inline(v, indent + 2));
            out.push('\n');
        }
    }
}

fn map_block(out: &mut String, m: &Map, indent: usize, after_dash: bool) {
    for (i, (k, v)) in m.0.iter().enumerate() {
        if i > 0 || !after_dash {
            pad(out, indent);
        }
        out.push_str(&key(k));
        out.push(':');
        after_key(out, v, indent);
    }
}

fn seq_block(out: &mut String, s: &[Value], indent: usize, after_dash: bool) {
    for (i, item) in s.iter().enumerate() {
        if i > 0 || !after_dash {
            pad(out, indent);
        }
        out.push_str("- ");
        match item {
            Value::Map(m) if !m.is_empty() => map_block(out, m, indent + 2, true),
            Value::Seq(inner) if !inner.is_empty() => seq_block(out, inner, indent + 2, true),
            other => {
                out.push_str(&inline(other, indent + 2));
                out.push('\n');
            }
        }
    }
}

fn inline(v: &Value, indent: usize) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.clone(),
        Value::Str(s) => string(s, indent, false),
        Value::Seq(_) => "[]".into(),
        Value::Map(_) => "{}".into(),
    }
}

fn key(k: &str) -> String {
    string(k, 0, true)
}

/// Rendu d'une chaîne dont le contexte est indenté de `indent` espaces (`stringifyString` de `yaml`).
fn string(s: &str, indent: usize, implicit_key: bool) -> String {
    if s.chars().any(forces_double_quotes) {
        return double_quoted(s, indent, implicit_key);
    }
    let multiline = s.contains('\n');
    if implicit_key && multiline {
        return quoted(s, indent, implicit_key);
    }
    if s.is_empty() || plain_forbidden(s) {
        return if implicit_key || !multiline { quoted(s, indent, implicit_key) } else { block(s, indent) };
    }
    if multiline {
        return block(s, indent);
    }
    if looks_like_non_string(s) {
        return quoted(s, indent, implicit_key);
    }
    s.to_owned()
}

fn forces_double_quotes(c: char) -> bool {
    matches!(c, '\0'..='\x08' | '\x0b'..='\x1f' | '\x7f'..='\u{9f}')
}

fn plain_forbidden(s: &str) -> bool {
    let b = s.as_bytes();
    if b"\n\t ,[]{}#&*!|>'\"%@`".contains(&b[0]) {
        return true;
    }
    if matches!(b[0], b'?' | b'-') && b.get(1).is_none_or(|c| matches!(c, b' ' | b'\t')) {
        return true;
    }
    let inside = b.windows(2).any(|w| {
        matches!((w[0], w[1]), (b'\n' | b':', b' ' | b'\t') | (b' ' | b'\t', b'\n') | (b'\n' | b'\t' | b' ', b'#'))
    });
    inside || matches!(b[b.len() - 1], b'\n' | b'\t' | b' ' | b':')
}

fn quoted(s: &str, indent: usize, implicit_key: bool) -> String {
    if s.contains('"') && !s.contains('\'') {
        single_quoted(s, indent, implicit_key)
    } else {
        double_quoted(s, indent, implicit_key)
    }
}

fn single_quoted(s: &str, indent: usize, implicit_key: bool) -> String {
    let space_around_newline =
        s.as_bytes().windows(2).any(|w| matches!((w[0], w[1]), (b' ' | b'\t', b'\n') | (b'\n', b' ' | b'\t')));
    if (implicit_key && s.contains('\n')) || space_around_newline {
        return double_quoted(s, indent, implicit_key);
    }
    let suffix = format!("\n{}", " ".repeat(indent));
    format!("'{}'", after_newline_runs(&s.replace('\'', "''"), &suffix, false))
}

fn double_quoted(s: &str, indent: usize, implicit_key: bool) -> String {
    let json = json_string(s);
    let b = json.as_bytes();
    let fold = !implicit_key && json.encode_utf16().count() >= 40;
    let mut out = String::new();
    let mut start = 0;
    let mut i = 0;
    while i < b.len() {
        let mut ch = b[i];
        if ch == b' ' && b.get(i + 1) == Some(&b'\\') && b.get(i + 2) == Some(&b'n') {
            out.push_str(&json[start..i]);
            out.push_str("\\ ");
            i += 1;
            start = i;
            ch = b'\\';
        }
        if ch == b'\\' {
            match b.get(i + 1) {
                Some(b'u') => {
                    out.push_str(&json[start..i]);
                    match &json[i + 2..i + 6] {
                        "0000" => out.push_str("\\0"),
                        "0007" => out.push_str("\\a"),
                        "000b" => out.push_str("\\v"),
                        "001b" => out.push_str("\\e"),
                        code => {
                            out.push_str("\\x");
                            out.push_str(&code[2..]);
                        }
                    }
                    i += 5;
                    start = i + 1;
                }
                Some(b'n') if fold && b.get(i + 2) != Some(&b'"') => {
                    out.push_str(&json[start..i]);
                    out.push_str("\n\n");
                    while b.get(i + 2) == Some(&b'\\') && b.get(i + 3) == Some(&b'n') && b.get(i + 4) != Some(&b'"') {
                        out.push('\n');
                        i += 2;
                    }
                    pad(&mut out, indent);
                    if b.get(i + 2) == Some(&b' ') {
                        out.push('\\');
                    }
                    i += 1;
                    start = i + 1;
                }
                _ => i += 1,
            }
        }
        i += 1;
    }
    if start == 0 {
        return json;
    }
    out.push_str(&json[start..]);
    out
}

/// Chaîne JSON telle que l'écrit `JSON.stringify`.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn block(s: &str, indent: usize) -> String {
    let trimmed = s.trim_end_matches([' ', '\t']);
    if (trimmed.len() < s.len() && trimmed.ends_with('\n')) || s.chars().all(is_js_space) {
        return quoted(s, indent, false);
    }
    let ind = " ".repeat(indent);
    let (value, end) = s.split_at(s.trim_end_matches(['\n', '\t', ' ']).len());
    let chomp = match end.find('\n') {
        None => "-",
        Some(p) if value.is_empty() || p != end.len() - 1 => "+",
        Some(_) => "",
    };
    let end = after_newline_runs(end.strip_suffix('\n').unwrap_or(end), &ind, true);
    let lead = &value[..value.len() - value.trim_start_matches([' ', '\n']).len()];
    let (start, rest) = value.split_at(lead.rfind('\n').map_or(0, |p| p + 1));
    let width = match (lead.contains(' '), indent) {
        (false, _) => "",
        (true, 0) => "1",
        (true, _) => "2",
    };
    format!(
        "|{width}{chomp}\n{ind}{}{}{end}",
        after_newline_runs(start, &ind, false),
        after_newline_runs(rest, &ind, false)
    )
}

/// Ajoute `suffix` après chaque suite de sauts de ligne, sauf en fin de texte si `except_at_end`.
fn after_newline_runs(s: &str, suffix: &str, except_at_end: bool) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        out.push(c);
        let next = chars.peek();
        if c == '\n' && next != Some(&'\n') && !(except_at_end && next.is_none()) {
            out.push_str(suffix);
        }
    }
    out
}

/// Blanc au sens de `\s` (et de `String.prototype.trim`) en JavaScript.
pub fn is_js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

fn looks_like_non_string(s: &str) -> bool {
    const WORDS: [&str; 10] = ["~", "null", "Null", "NULL", "true", "True", "TRUE", "false", "False", "FALSE"];
    let digits = |t: &str, radix: u32| !t.is_empty() && t.chars().all(|c| c.is_digit(radix));
    if WORDS.contains(&s) || s.strip_prefix("0o").is_some_and(|o| digits(o, 8)) {
        return true;
    }
    if s.strip_prefix("0x").is_some_and(|h| digits(h, 16)) {
        return true;
    }
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    matches!(unsigned, ".inf" | ".Inf" | ".INF" | ".nan" | ".NaN" | ".NAN") || is_float(unsigned)
}

fn is_float(s: &str) -> bool {
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let digits = |t: &str| t.chars().all(|c| c.is_ascii_digit());
    let mantissa_ok = match mantissa.split_once('.') {
        Some((int, frac)) => (!int.is_empty() || !frac.is_empty()) && digits(int) && digits(frac),
        None => !mantissa.is_empty() && digits(mantissa),
    };
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && digits(e)
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
            ("a\tb", "a\tb"),
            ("---x", "---x"),
            ("--1", "--1"),
            ("+.nan", "\"+.nan\""),
            ("bell\x07\x1f\x7f", "\"bell\\a\\x1f\x7f\""),
        ];
        for (input, expected) in cases {
            assert_eq!(string(input, 2, false), expected, "scalaire {input:?}");
        }
    }

    #[test]
    fn long_double_quoted_strings_fold_like_bruno() {
        let crlf = "first line of the text\r\nsecond line of the text";
        assert_eq!(string(crlf, 2, false), "\"first line of the text\\r\n\n  second line of the text\"");
    }

    #[test]
    fn block_literals_use_bruno_chomping() {
        let doc = |s: &str| emit(&Value::Map(Map(vec![("k".into(), Value::str(s))])), &[]);
        assert_eq!(doc("a\nb"), "k: |-\n  a\n  b\n");
        assert_eq!(doc("a\nb\n"), "k: |\n  a\n  b\n");
        assert_eq!(doc("a\n\nb"), "k: |-\n  a\n\n  b\n");
        assert_eq!(doc(" a\nb"), "k: |2-\n   a\n  b\n");
        assert_eq!(doc("\n\na"), "k: |-\n  \n\n  a\n");
        assert_eq!(doc("a\n \n"), "k: |+\n  a\n   \n");
        assert_eq!(doc("a\n  "), "k: \"a\\n  \"\n");
    }

    #[test]
    fn set_inserts_at_canonical_position() {
        let mut m = Map(vec![("method".into(), Value::str("GET")), ("auth".into(), Value::str("inherit"))]);
        m.set("url", Value::str("https://x.test"), &["method", "url", "headers", "params", "body", "auth"]);
        let keys: Vec<_> = m.0.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["method", "url", "auth"]);
    }
}
