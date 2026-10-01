//! Port de `parse` de `shell-quote` 1.11 appelé sans environnement : mots, opérateurs de contrôle, globs,
//! commentaires, guillemets, `$'…'` et variables (développées à vide).

use serde_json::{json, Value};

use super::js::{is_js_whitespace, JsError};

/// Élément produit par `shell-quote` : un mot ou l'un de ses objets (`{op}`, `{op: 'glob'}`, `{comment}`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Token {
    Word(String),
    Op(String),
    Glob(String),
    Comment(String),
}

impl Token {
    pub(crate) fn as_word(&self) -> Option<&str> {
        match self {
            Token::Word(s) => Some(s),
            _ => None,
        }
    }

    /// `String(valeur)` en JavaScript.
    pub(crate) fn js_string(&self) -> String {
        self.as_word().map_or_else(|| "[object Object]".to_owned(), str::to_owned)
    }

    pub(crate) fn to_json(&self) -> Value {
        match self {
            Token::Word(s) => json!(s),
            Token::Op(op) => json!({ "op": op }),
            Token::Glob(pattern) => json!({ "op": "glob", "pattern": pattern }),
            Token::Comment(comment) => json!({ "comment": comment }),
        }
    }
}

const CONTROL_OPERATORS: [&str; 9] = ["||", "&&", ";;", "|&", "<(", "<<<", ">>", ">&", "<&"];
const BAREWORD_ESCAPABLE: &[char] = &['\'', '"', '$', '\\', '|', '&', ';', '(', ')', '<', '>', ' ', '\t'];
const BAREWORD_EXCLUDED: &[char] = &['\'', '"', '$', '|', '&', ';', '(', ')', '<', '>', ' ', '\t'];

pub(crate) fn parse(input: &str) -> Result<Vec<Token>, JsError> {
    let s: Vec<char> = input.chars().collect();
    let mut tokens = Vec::new();
    let mut commented = false;
    let mut p = 0;
    while p < s.len() {
        if let Some(len) = control_at(&s, p) {
            if !commented {
                tokens.push(Token::Op(s[p..p + len].iter().collect()));
            }
            p += len;
            continue;
        }
        let mut end = p;
        while let Some(next) = chunk_part(&s, end) {
            end = next;
        }
        if end == p {
            p += 1;
            continue;
        }
        if !commented {
            scan_chunk(&s, p, end, &mut commented, &mut tokens)?;
        }
        p = end;
    }
    Ok(tokens)
}

fn is_control_char(c: char) -> bool {
    matches!(c, '&' | ';' | '(' | ')' | '|' | '<' | '>')
}

fn starts_with(s: &[char], p: usize, needle: &str) -> bool {
    let mut i = p;
    for c in needle.chars() {
        if s.get(i) != Some(&c) {
            return false;
        }
        i += 1;
    }
    true
}

fn control_at(s: &[char], p: usize) -> Option<usize> {
    if let Some(op) = CONTROL_OPERATORS.iter().find(|op| starts_with(s, p, op)) {
        return Some(op.chars().count());
    }
    s.get(p).copied().filter(|&c| is_control_char(c)).map(|_| 1)
}

fn chunk_part(s: &[char], q: usize) -> Option<usize> {
    if starts_with(s, q, "$'") {
        if let Some(end) = ansi_c_quote_end(s, q) {
            return Some(end);
        }
    }
    let mut end = q;
    while let Some(next) = bareword_unit(s, end) {
        end = next;
    }
    if end > q {
        return Some(end);
    }
    match s.get(q) {
        Some('"') => double_quote_end(s, q),
        Some('\'') => s[q + 1..].iter().position(|&c| c == '\'').map(|i| q + i + 2),
        _ => None,
    }
}

fn ansi_c_quote_end(s: &[char], q: usize) -> Option<usize> {
    let mut r = q + 2;
    loop {
        match s.get(r)? {
            '\'' => return Some(r + 1),
            '\\' if r + 1 < s.len() => r += 2,
            '\\' => return None,
            _ => r += 1,
        }
    }
}

fn double_quote_end(s: &[char], q: usize) -> Option<usize> {
    let mut r = q + 1;
    loop {
        match s.get(r)? {
            '"' => return Some(r + 1),
            '\\' if r + 1 < s.len() => r += 2,
            '\\' => return None,
            _ => r += 1,
        }
    }
}

fn bareword_unit(s: &[char], r: usize) -> Option<usize> {
    let c = *s.get(r)?;
    let next = s.get(r + 1).copied();
    if c == '\\' && next.is_some_and(|n| BAREWORD_ESCAPABLE.contains(&n)) {
        return Some(r + 2);
    }
    if c == '$' {
        if next == Some('$') {
            return Some(r + 2);
        }
        let opens_ansi_c = next == Some('\'') && ansi_c_quote_end(s, r).is_some();
        return (!opens_ansi_c).then_some(r + 1);
    }
    (!is_js_whitespace(c) && !BAREWORD_EXCLUDED.contains(&c)).then_some(r + 1)
}

fn scan_chunk(
    input: &[char],
    start: usize,
    end: usize,
    commented: &mut bool,
    tokens: &mut Vec<Token>,
) -> Result<(), JsError> {
    let s = &input[start..end];
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut out = String::new();
    let mut is_glob = false;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        is_glob = is_glob || (quote.is_none() && (c == '*' || c == '?'));
        if escaped {
            out.push(c);
            escaped = false;
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            } else if q == '\'' {
                out.push(c);
            } else if c == '\\' {
                i += 1;
                match s.get(i) {
                    Some(&n) if matches!(n, '"' | '\\' | '$') => out.push(n),
                    Some(&n) => {
                        out.push('\\');
                        out.push(n);
                    }
                    None => out.push('\\'),
                }
            } else if c == '$' {
                out.push_str(&parse_env_var(s, &mut i)?);
            } else {
                out.push(c);
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if is_control_char(c) {
            tokens.push(Token::Op(s.iter().collect()));
            return Ok(());
        } else if c == '#' {
            *commented = true;
            if !out.is_empty() {
                tokens.push(Token::Word(out));
            }
            tokens.push(Token::Comment(input[start + i + 1..].iter().collect()));
            return Ok(());
        } else if c == '\\' {
            escaped = true;
        } else if c == '$' {
            match ansi_c_quote_end(s, i).filter(|_| s.get(i + 1) == Some(&'\'')) {
                Some(close) => {
                    out.push_str(&expand_ansi_c(&s[i + 2..close - 1]));
                    i = close - 1;
                }
                None => out.push_str(&parse_env_var(s, &mut i)?),
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    tokens.push(if is_glob { Token::Glob(out) } else { Token::Word(out) });
    Ok(())
}

fn parse_env_var(s: &[char], i: &mut usize) -> Result<String, JsError> {
    *i += 1;
    let name: String = match s.get(*i) {
        Some('{') => {
            *i += 1;
            if s.get(*i) == Some(&'}') {
                return Err(JsError("Error: Bad substitution".into()));
            }
            let mut depth = 1;
            let mut end = *i;
            while depth > 0 && end < s.len() {
                if s[end] == '{' && s[end - 1] == '$' {
                    depth += 1;
                } else if s[end] == '}' {
                    depth -= 1;
                }
                end += 1;
            }
            if depth != 0 {
                return Err(JsError("Error: Bad substitution".into()));
            }
            end -= 1;
            let name = s[*i..end].iter().collect();
            *i = end;
            name
        }
        Some(&c) if "*@#?$!-".contains(c) => c.to_string(),
        _ => {
            let rest = &s[*i..];
            match rest.iter().position(|c| !(c.is_ascii_alphanumeric() || *c == '_')) {
                None => {
                    *i = s.len();
                    rest.iter().collect()
                }
                Some(k) => {
                    *i = *i + k - 1;
                    rest[..k].iter().collect()
                }
            }
        }
    };
    Ok(env_value(&name))
}

/// Lecture de `{}[nom]` : vide, sauf `$` seul et propriétés héritées d'`Object.prototype`.
fn env_value(name: &str) -> String {
    let function = match name {
        "" => return "$".to_owned(),
        "constructor" => "Object",
        "__defineGetter__"
        | "__defineSetter__"
        | "hasOwnProperty"
        | "__lookupGetter__"
        | "__lookupSetter__"
        | "isPrototypeOf"
        | "propertyIsEnumerable"
        | "toString"
        | "valueOf"
        | "toLocaleString" => name,
        _ => return String::new(),
    };
    format!("function {function}() {{ [native code] }}")
}

fn expand_ansi_c(body: &[char]) -> String {
    let mut out: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        if body[i] == '\\' {
            if let Some(len) = ansi_c_escape_len(&body[i + 1..]) {
                let escape: String = body[i + 1..i + 1 + len].iter().collect();
                let matched: String = body[i..i + 1 + len].iter().collect();
                expand_ansi_c_escape(&matched, &escape, &mut out);
                i += 1 + len;
                continue;
            }
        }
        let mut buf = [0u16; 2];
        out.extend_from_slice(body[i].encode_utf16(&mut buf));
        i += 1;
    }
    let end = out.iter().position(|&u| u == 0).unwrap_or(out.len());
    String::from_utf16_lossy(&out[..end])
}

fn ansi_c_escape_len(rest: &[char]) -> Option<usize> {
    let run = |from: usize, max: usize, accept: fn(&char) -> bool| {
        rest[from.min(rest.len())..].iter().take(max).take_while(|c| accept(c)).count()
    };
    let first = *rest.first()?;
    let octal = run(0, 3, |c| ('0'..='7').contains(c));
    if octal > 0 {
        return Some(octal);
    }
    let hex_max = match first {
        'x' => 2,
        'u' => 4,
        'U' => 8,
        _ => 0,
    };
    if hex_max > 0 {
        let digits = run(1, hex_max, char::is_ascii_hexdigit);
        if digits > 0 {
            return Some(1 + digits);
        }
    }
    if first == 'c' {
        return match (rest.get(1), rest.get(2)) {
            (Some('\\'), Some('\\')) => Some(3),
            (Some(_), _) => Some(2),
            (None, _) => None,
        };
    }
    "abeEfnrtv\\'\"?".contains(first).then_some(1)
}

fn expand_ansi_c_escape(matched: &str, escape: &str, out: &mut Vec<u16>) {
    let mut chars = escape.chars();
    let kind = chars.next().unwrap_or_default();
    let unit = match kind {
        'c' => {
            let ctrl = chars.next().unwrap_or_default();
            if ctrl == '?' {
                0x7F
            } else {
                let mut buf = [0u16; 2];
                let units = ctrl.encode_utf16(&mut buf);
                out.push(units[0] & 0x1F);
                out.extend_from_slice(&units[1..]);
                return;
            }
        }
        'x' | 'u' | 'U' => {
            let cp = u32::from_str_radix(&escape[1..], 16).unwrap_or(0);
            if cp > 0x10FFFF {
                out.extend(matched.encode_utf16());
                return;
            }
            if cp > 0xFFFF {
                out.push((0xD7C0 + (cp >> 10)) as u16);
                out.push((0xDC00 + (cp & 0x3FF)) as u16);
                return;
            }
            cp as u16
        }
        '0'..='7' => (u32::from_str_radix(escape, 8).unwrap_or(0) & 0xFF) as u16,
        _ => match "abeEfnrtv".find(kind) {
            Some(index) => [0x07, 0x08, 0x1B, 0x1B, 0x0C, 0x0A, 0x0D, 0x09, 0x0B][index],
            None => kind as u16,
        },
    };
    out.push(unit);
}
