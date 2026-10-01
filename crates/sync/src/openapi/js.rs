//! Valeurs et opérations JavaScript nécessaires au port fidèle des convertisseurs de Bruno :
//! identité des objets, ordre des clés, `String(x)`, `JSON.stringify`, véracité, chaînes.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::Value;

use super::{OpenApiError, R};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Id(usize);

/// Valeur JavaScript : les tableaux et objets vivent dans un [`Heap`] et gardent leur identité.
#[derive(Clone, Debug)]
pub enum Js {
    Undef,
    Null,
    Bool(bool),
    Num(f64),
    Str(Rc<str>),
    Arr(Id),
    Obj(Id),
}

pub type Entries = Vec<(Rc<str>, Js)>;

enum Slot {
    Arr(Vec<Js>),
    Obj(Entries),
}

#[derive(Default)]
pub struct Heap {
    slots: RefCell<Vec<Slot>>,
}

pub fn type_error(msg: impl Into<String>) -> OpenApiError {
    OpenApiError::Invalid(msg.into())
}

impl Js {
    pub fn str(s: &str) -> Self {
        Self::Str(s.into())
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn is_undef(&self) -> bool {
        matches!(self, Self::Undef)
    }

    pub fn is_nullish(&self) -> bool {
        matches!(self, Self::Undef | Self::Null)
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Self::Arr(_))
    }

    pub fn truthy(&self) -> bool {
        match self {
            Self::Undef | Self::Null => false,
            Self::Bool(b) => *b,
            Self::Num(n) => *n != 0.0 && !n.is_nan(),
            Self::Str(s) => !s.is_empty(),
            Self::Arr(_) | Self::Obj(_) => true,
        }
    }

    /// `a || b`
    pub fn or(self, other: Js) -> Js {
        if self.truthy() {
            self
        } else {
            other
        }
    }

    /// `a ?? b`
    pub fn coalesce(self, other: Js) -> Js {
        if self.is_nullish() {
            other
        } else {
            self
        }
    }

    pub fn strict_eq(&self, other: &Js) -> bool {
        match (self, other) {
            (Self::Undef, Self::Undef) | (Self::Null, Self::Null) => true,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Num(a), Self::Num(b)) => a == b,
            (Self::Str(a), Self::Str(b)) => a == b,
            (Self::Arr(a), Self::Arr(b)) | (Self::Obj(a), Self::Obj(b)) => a == b,
            _ => false,
        }
    }

    pub fn is_str(&self, s: &str) -> bool {
        self.as_str() == Some(s)
    }
}

impl Heap {
    fn push(&self, slot: Slot) -> Id {
        let mut slots = self.slots.borrow_mut();
        slots.push(slot);
        Id(slots.len() - 1)
    }

    pub fn arr(&self, items: Vec<Js>) -> Js {
        Js::Arr(self.push(Slot::Arr(items)))
    }

    pub fn obj(&self, entries: Entries) -> Js {
        Js::Obj(self.push(Slot::Obj(js_order(entries))))
    }

    pub fn reserve(&self) -> Id {
        self.push(Slot::Obj(Vec::new()))
    }

    pub fn fill(&self, id: Id, entries: Entries) {
        self.slots.borrow_mut()[id.0] = Slot::Obj(js_order(entries));
    }

    /// `{ ...base, k: v }` ; une valeur `undefined` retire la clé.
    pub fn spread(&self, base: &Js, overrides: &[(&str, Js)]) -> Js {
        let mut entries = self.own_entries(base);
        for (key, value) in overrides {
            let at = entries.iter().position(|(k, _)| &**k == *key);
            match (at, value.is_undef()) {
                (Some(i), true) => {
                    entries.remove(i);
                }
                (Some(i), false) => entries[i].1 = value.clone(),
                (None, false) => entries.push(((*key).into(), value.clone())),
                (None, true) => {}
            }
        }
        self.obj(entries)
    }

    fn own_entries(&self, v: &Js) -> Entries {
        match v {
            Js::Obj(id) => match &self.slots.borrow()[id.0] {
                Slot::Obj(e) => e.clone(),
                Slot::Arr(_) => Vec::new(),
            },
            Js::Arr(_) | Js::Str(_) => {
                self.values_of(v).into_iter().enumerate().map(|(i, x)| (i.to_string().into(), x)).collect()
            }
            _ => Vec::new(),
        }
    }

    fn values_of(&self, v: &Js) -> Vec<Js> {
        match v {
            Js::Arr(id) => match &self.slots.borrow()[id.0] {
                Slot::Arr(items) => items.clone(),
                Slot::Obj(_) => Vec::new(),
            },
            Js::Str(s) => s.chars().map(|c| Js::str(c.encode_utf8(&mut [0; 4]))).collect(),
            _ => Vec::new(),
        }
    }

    /// Lecture de propriété tolérante : `undefined` sur `null`/`undefined` au lieu d'une erreur.
    pub fn get(&self, v: &Js, key: &str) -> Js {
        match v {
            Js::Obj(id) => match &self.slots.borrow()[id.0] {
                Slot::Obj(entries) => entries.iter().find(|(k, _)| &**k == key).map(|(_, x)| x.clone()),
                Slot::Arr(_) => None,
            }
            .unwrap_or(Js::Undef),
            Js::Arr(id) => {
                let slots = self.slots.borrow();
                let Slot::Arr(items) = &slots[id.0] else { return Js::Undef };
                match key {
                    "length" => Js::Num(items.len() as f64),
                    _ => array_index(key).and_then(|i| items.get(i).cloned()).unwrap_or(Js::Undef),
                }
            }
            Js::Str(s) => match key {
                "length" => Js::Num(s.encode_utf16().count() as f64),
                _ => array_index(key)
                    .and_then(|i| s.encode_utf16().nth(i))
                    .map(|u| {
                        Js::Str(char::decode_utf16([u]).map(|c| c.unwrap_or('\u{fffd}')).collect::<String>().into())
                    })
                    .unwrap_or(Js::Undef),
            },
            _ => Js::Undef,
        }
    }

    /// Lecture de propriété stricte : `TypeError` sur `null`/`undefined`, comme en JavaScript.
    pub fn prop(&self, v: &Js, key: &str) -> R<Js> {
        if v.is_nullish() {
            return Err(type_error(format!("lecture de « {key} » sur une valeur nulle")));
        }
        Ok(self.get(v, key))
    }

    pub fn path(&self, v: &Js, keys: &[&str]) -> Js {
        keys.iter().fold(v.clone(), |cur, k| self.get(&cur, k))
    }

    pub fn index(&self, v: &Js, i: usize) -> Js {
        self.get(v, &i.to_string())
    }

    pub fn has_own(&self, v: &Js, key: &str) -> R<bool> {
        match v {
            Js::Undef | Js::Null => Err(type_error(format!("Object.hasOwn sur une valeur nulle (« {key} »)"))),
            Js::Obj(id) => Ok(matches!(&self.slots.borrow()[id.0], Slot::Obj(e) if e.iter().any(|(k, _)| &**k == key))),
            Js::Arr(_) | Js::Str(_) => {
                Ok(key == "length" || array_index(key).is_some_and(|i| !self.index(v, i).is_undef()))
            }
            _ => Ok(false),
        }
    }

    /// `Array.isArray(v) ? v : None`
    pub fn items(&self, v: &Js) -> Option<Vec<Js>> {
        v.is_array().then(|| self.values_of(v))
    }

    /// Éléments d'un tableau sur lequel le JavaScript appelle `map`/`forEach`/`join`.
    pub fn array_strict(&self, v: &Js, what: &str) -> R<Vec<Js>> {
        self.items(v).ok_or_else(|| type_error(format!("{what} n'est pas un tableau")))
    }

    /// `Object.entries(v)`
    pub fn entries(&self, v: &Js) -> R<Entries> {
        if v.is_nullish() {
            return Err(type_error("Object.entries sur une valeur nulle"));
        }
        Ok(self.own_entries(v))
    }

    pub fn keys(&self, v: &Js) -> R<Vec<Rc<str>>> {
        Ok(self.entries(v)?.into_iter().map(|(k, _)| k).collect())
    }

    pub fn values(&self, v: &Js) -> R<Vec<Js>> {
        Ok(self.entries(v)?.into_iter().map(|(_, x)| x).collect())
    }

    /// `lodash.each` : clés numériques pour les tableaux et chaînes, rien pour les autres valeurs.
    pub fn each(&self, v: &Js) -> Vec<(Js, Js)> {
        match v {
            Js::Obj(_) => self.own_entries(v).into_iter().map(|(k, x)| (Js::Str(k), x)).collect(),
            Js::Arr(_) | Js::Str(_) => {
                self.values_of(v).into_iter().enumerate().map(|(i, x)| (Js::Num(i as f64), x)).collect()
            }
            _ => Vec::new(),
        }
    }

    /// `x > 0`
    pub fn gt_zero(&self, v: &Js) -> bool {
        self.to_number(v) > 0.0
    }

    pub fn to_number(&self, v: &Js) -> f64 {
        match v {
            Js::Undef => f64::NAN,
            Js::Null => 0.0,
            Js::Bool(b) => f64::from(u8::from(*b)),
            Js::Num(n) => *n,
            Js::Str(s) => string_to_number(s),
            Js::Arr(_) | Js::Obj(_) => string_to_number(&self.to_string(v)),
        }
    }

    /// `String(v)`
    pub fn to_string(&self, v: &Js) -> String {
        let mut out = String::new();
        self.write_string(v, &mut Vec::new(), &mut out);
        out
    }

    fn write_string(&self, v: &Js, stack: &mut Vec<Id>, out: &mut String) {
        match v {
            Js::Undef => out.push_str("undefined"),
            Js::Null => out.push_str("null"),
            Js::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Js::Num(n) => out.push_str(&number_to_string(*n)),
            Js::Str(s) => out.push_str(s),
            Js::Obj(_) => out.push_str("[object Object]"),
            Js::Arr(id) => {
                if stack.contains(id) {
                    return;
                }
                stack.push(*id);
                for (i, item) in self.values_of(v).iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if !item.is_nullish() {
                        self.write_string(item, stack, out);
                    }
                }
                stack.pop();
            }
        }
    }

    /// `array.join(sep)` avec la conversion `String` des éléments (`null`/`undefined` → vide).
    pub fn join(&self, items: &[Js], sep: &str) -> String {
        items
            .iter()
            .map(|x| if x.is_nullish() { String::new() } else { self.to_string(x) })
            .collect::<Vec<_>>()
            .join(sep)
    }

    /// `JSON.stringify(v)` ou `JSON.stringify(v, null, 2)` ; `None` pour `undefined`.
    pub fn stringify(&self, v: &Js, pretty: bool) -> R<Option<String>> {
        let mut out = String::new();
        Ok(self.write_json(v, pretty, 0, &mut Vec::new(), &mut out)?.then_some(out))
    }

    fn write_json(&self, v: &Js, pretty: bool, depth: usize, stack: &mut Vec<Id>, out: &mut String) -> R<bool> {
        match v {
            Js::Undef => return Ok(false),
            Js::Null => out.push_str("null"),
            Js::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Js::Num(n) if n.is_finite() => out.push_str(&number_to_string(*n)),
            Js::Num(_) => out.push_str("null"),
            Js::Str(s) => quote(s, out),
            Js::Arr(id) | Js::Obj(id) => {
                if stack.contains(id) {
                    return Err(type_error("structure circulaire impossible à sérialiser en JSON"));
                }
                stack.push(*id);
                let (open, close) = if v.is_array() { ('[', ']') } else { ('{', '}') };
                let mut parts = Vec::new();
                if v.is_array() {
                    for item in self.values_of(v) {
                        let mut part = String::new();
                        if !self.write_json(&item, pretty, depth + 1, stack, &mut part)? {
                            part = "null".into();
                        }
                        parts.push(part);
                    }
                } else {
                    for (k, item) in self.own_entries(v) {
                        let mut part = String::new();
                        quote(&k, &mut part);
                        part.push_str(if pretty { ": " } else { ":" });
                        if self.write_json(&item, pretty, depth + 1, stack, &mut part)? {
                            parts.push(part);
                        }
                    }
                }
                stack.pop();
                out.push(open);
                if !parts.is_empty() {
                    if pretty {
                        let inner = format!("\n{}", "  ".repeat(depth + 1));
                        out.push_str(&inner);
                        out.push_str(&parts.join(&format!(",{inner}")));
                        out.push('\n');
                        out.push_str(&"  ".repeat(depth));
                    } else {
                        out.push_str(&parts.join(","));
                    }
                }
                out.push(close);
            }
        }
        Ok(true)
    }

    /// Valeur telle que la verrait `JSON.parse(JSON.stringify(v))` ; `None` pour `undefined`.
    pub fn to_value(&self, v: &Js) -> R<Option<Value>> {
        self.value_of(v, &mut Vec::new())
    }

    fn value_of(&self, v: &Js, stack: &mut Vec<Id>) -> R<Option<Value>> {
        Ok(Some(match v {
            Js::Undef => return Ok(None),
            Js::Null => Value::Null,
            Js::Bool(b) => Value::Bool(*b),
            Js::Num(n) => number_value(*n),
            Js::Str(s) => Value::String(s.to_string()),
            Js::Arr(id) | Js::Obj(id) => {
                if stack.contains(id) {
                    return Err(type_error("structure circulaire impossible à sérialiser en JSON"));
                }
                stack.push(*id);
                let value = if v.is_array() {
                    let mut items = Vec::new();
                    for item in self.values_of(v) {
                        items.push(self.value_of(&item, stack)?.unwrap_or(Value::Null));
                    }
                    Value::Array(items)
                } else {
                    let mut map = serde_json::Map::new();
                    for (k, item) in self.own_entries(v) {
                        if let Some(x) = self.value_of(&item, stack)? {
                            map.insert(k.to_string(), x);
                        }
                    }
                    Value::Object(map)
                };
                stack.pop();
                value
            }
        }))
    }

    /// Copie d'une valeur JSON dans le tas, clés dans l'ordre des objets JavaScript.
    pub fn import(&self, v: &Value) -> Js {
        match v {
            Value::Null => Js::Null,
            Value::Bool(b) => Js::Bool(*b),
            Value::Number(n) => Js::Num(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => Js::str(s),
            Value::Array(items) => self.arr(items.iter().map(|x| self.import(x)).collect()),
            Value::Object(map) => self.obj(map.iter().map(|(k, x)| (k.as_str().into(), self.import(x))).collect()),
        }
    }
}

fn quote(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Indice de tableau canonique (`"0"`, `"42"`), au sens des clés d'objet JavaScript.
pub fn array_index(key: &str) -> Option<usize> {
    let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()));
    key.parse::<u64>().ok().filter(|n| canonical && *n < u64::from(u32::MAX)).map(|n| n as usize)
}

/// Ordre des clés d'un objet JavaScript : indices croissants, puis ordre d'insertion.
pub fn js_order<T>(mut entries: Vec<(Rc<str>, T)>) -> Vec<(Rc<str>, T)> {
    if entries.iter().any(|(k, _)| array_index(k).is_some()) {
        entries.sort_by_key(|(k, _)| array_index(k).map_or((1, 0), |i| (0, i)));
    }
    entries
}

/// `Number.prototype.toString()`
pub fn number_to_string(n: f64) -> String {
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
pub fn number_value(n: f64) -> Value {
    if !n.is_finite() {
        return Value::Null;
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
    serde_json::Number::from_f64(n).map_or(Value::Null, Value::Number)
}

/// `Number(s)`
pub fn string_to_number(s: &str) -> f64 {
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
            if rest.is_empty() || !rest.chars().all(|c| c.is_digit(radix)) {
                return f64::NAN;
            }
            return rest.chars().fold(0.0, |acc, c| acc * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0)));
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

/// Espaces au sens de `\s` et de `String.prototype.trim` en JavaScript.
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
    )
}

pub fn trim(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// Remplace chaque suite d'espaces par `with`.
pub fn collapse_spaces(s: &str, with: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_run = false;
    for c in s.chars() {
        if is_space(c) {
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

/// Motif de remplacement de `String.prototype.replace` (`$$`, `$&`, `` $` ``, `$'`, `$n`).
pub fn substitute(replacement: &str, haystack: &str, start: usize, end: usize, captures: &[&str]) -> String {
    let mut out = String::new();
    let mut chars = replacement.char_indices();
    while let Some((i, c)) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let rest = &replacement[i + 1..];
        let mut consumed = 1;
        match rest.chars().next() {
            Some('$') => out.push('$'),
            Some('&') => out.push_str(&haystack[start..end]),
            Some('`') => out.push_str(&haystack[..start]),
            Some('\'') => out.push_str(&haystack[end..]),
            Some(d) if d.is_ascii_digit() => {
                let two = rest
                    .get(..2)
                    .filter(|x| x.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|x| x.parse::<usize>().ok());
                let one = d.to_digit(10).unwrap_or(0) as usize;
                match (two, one) {
                    (Some(n), _) if n >= 1 && n <= captures.len() => {
                        out.push_str(captures[n - 1]);
                        consumed = 2;
                    }
                    (_, n) if n >= 1 && n <= captures.len() => out.push_str(captures[n - 1]),
                    _ => {
                        out.push('$');
                        consumed = 0;
                    }
                }
            }
            _ => {
                out.push('$');
                consumed = 0;
            }
        }
        for _ in 0..consumed {
            chars.next();
        }
    }
    out
}

/// `haystack.replaceAll(pattern, replacement)` avec un motif chaîne.
pub fn replace_all(haystack: &str, pattern: &str, replacement: &str) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (start, m) in haystack.match_indices(pattern) {
        out.push_str(&haystack[last..start]);
        out.push_str(&substitute(replacement, haystack, start, start + m.len(), &[]));
        last = start + m.len();
    }
    out.push_str(&haystack[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

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
        ];
        for (n, expected) in cases {
            assert_eq!(number_to_string(n), expected, "{n:e}");
        }
    }

    #[test]
    fn ef_imp_02_string_to_number_matches_javascript() {
        assert_eq!(string_to_number(" 200 "), 200.0);
        assert_eq!(string_to_number(""), 0.0);
        assert_eq!(string_to_number("0x1F"), 31.0);
        assert_eq!(string_to_number("1e3"), 1000.0);
        assert_eq!(string_to_number("-Infinity"), f64::NEG_INFINITY);
        for nan in ["default", "2XX", "1_0", "inf", "+-1", "0x", "."] {
            assert!(string_to_number(nan).is_nan(), "{nan}");
        }
    }

    #[test]
    fn ef_imp_02_stringify_and_string_follow_javascript() {
        let heap = Heap::default();
        let v = heap.import(&serde_json::json!({"b": [1.0, null, "é\u{1}"], "2": {}, "a": []}));
        assert_eq!(heap.stringify(&v, false).unwrap().unwrap(), r#"{"2":{},"b":[1,null,"é\u0001"],"a":[]}"#);
        assert_eq!(
            heap.stringify(&v, true).unwrap().unwrap(),
            "{\n  \"2\": {},\n  \"b\": [\n    1,\n    null,\n    \"é\\u0001\"\n  ],\n  \"a\": []\n}"
        );
        let list = heap.get(&v, "b");
        assert_eq!(heap.to_string(&list), "1,,é\u{1}");
        assert_eq!(heap.to_string(&v), "[object Object]");
    }

    #[test]
    fn ef_imp_02_replace_all_expands_dollar_patterns() {
        assert_eq!(replace_all("a{x}b{x}", "{x}", "[$&|$$|$1]"), "a[{x}|$|$1]b[{x}|$|$1]");
        assert_eq!(replace_all("ab", "b", "$`$'"), "aa");
        assert_eq!(substitute("<$1$2$10>", "xyz", 0, 1, &["P"]), "<P$2P0>");
    }
}
