//! `JSON.parse` et `JSON.stringify(valeur, null, 2)` avec la sémantique JavaScript (nombres flottants, ordre des
//! clés d'objet), pour le corps GraphQL.

use serde_json::{Number, Value};

use super::js::JsObject;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(JsObject<JsonValue>),
}

impl JsonValue {
    pub(crate) fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(object) => object.get(key),
            _ => None,
        }
    }

    /// Valeur telle que `JSON.stringify` la sérialise.
    pub(crate) fn to_value(&self) -> Value {
        match self {
            JsonValue::Null => Value::Null,
            JsonValue::Bool(b) => Value::Bool(*b),
            JsonValue::Number(n) if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 => {
                Value::Number(Number::from(*n as i64))
            }
            JsonValue::Number(n) => Number::from_f64(*n).map_or(Value::Null, Value::Number),
            JsonValue::String(s) => Value::String(s.clone()),
            JsonValue::Array(items) => Value::Array(items.iter().map(JsonValue::to_value).collect()),
            JsonValue::Object(object) => object.to_json(|v| Some(v.to_value())),
        }
    }
}

/// `JSON.parse(text)` ; `None` correspond à une `SyntaxError`.
pub(crate) fn parse(text: &str) -> Option<JsonValue> {
    let mut parser = Parser { s: text.as_bytes(), pos: 0 };
    let value = parser.value()?;
    parser.skip_ws();
    (parser.pos == parser.s.len()).then_some(value)
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.pos).copied()
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn eat(&mut self, literal: &str) -> Option<()> {
        self.s[self.pos..].starts_with(literal.as_bytes()).then(|| self.pos += literal.len())
    }

    fn value(&mut self) -> Option<JsonValue> {
        self.skip_ws();
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string().map(JsonValue::String),
            b't' => self.eat("true").map(|_| JsonValue::Bool(true)),
            b'f' => self.eat("false").map(|_| JsonValue::Bool(false)),
            b'n' => self.eat("null").map(|_| JsonValue::Null),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Option<JsonValue> {
        self.pos += 1;
        let mut object = JsObject::null_proto();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Some(JsonValue::Object(object));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return None;
            }
            let key = self.string()?;
            self.skip_ws();
            self.eat(":")?;
            let value = self.value()?;
            object.set(&key, value);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b'}' => {
                    self.pos += 1;
                    return Some(JsonValue::Object(object));
                }
                _ => return None,
            }
        }
    }

    fn array(&mut self) -> Option<JsonValue> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            return Some(JsonValue::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek()? {
                b',' => self.pos += 1,
                b']' => {
                    self.pos += 1;
                    return Some(JsonValue::Array(items));
                }
                _ => return None,
            }
        }
    }

    fn string(&mut self) -> Option<String> {
        self.pos += 1;
        let mut units: Vec<u16> = Vec::new();
        loop {
            let start = self.pos;
            while self.peek().is_some_and(|b| b != b'"' && b != b'\\' && b >= 0x20) {
                self.pos += 1;
            }
            units.extend(std::str::from_utf8(&self.s[start..self.pos]).ok()?.encode_utf16());
            match self.peek()? {
                b'"' => {
                    self.pos += 1;
                    return Some(String::from_utf16_lossy(&units));
                }
                b'\\' => {
                    let escape = *self.s.get(self.pos + 1)?;
                    self.pos += 2;
                    let unit = match escape {
                        b'"' => 0x22,
                        b'\\' => 0x5C,
                        b'/' => 0x2F,
                        b'b' => 0x08,
                        b'f' => 0x0C,
                        b'n' => 0x0A,
                        b'r' => 0x0D,
                        b't' => 0x09,
                        b'u' => {
                            let hex = std::str::from_utf8(self.s.get(self.pos..self.pos + 4)?).ok()?;
                            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                                return None;
                            }
                            self.pos += 4;
                            u16::from_str_radix(hex, 16).ok()?
                        }
                        _ => return None,
                    };
                    units.push(unit);
                }
                _ => return None,
            }
        }
    }

    fn number(&mut self) -> Option<JsonValue> {
        let start = self.pos;
        let digits = |p: &mut Self| {
            let from = p.pos;
            while p.peek().is_some_and(|b| b.is_ascii_digit()) {
                p.pos += 1;
            }
            p.pos - from
        };
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        if self.peek() == Some(b'0') {
            self.pos += 1;
        } else if digits(self) == 0 {
            return None;
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if digits(self) == 0 {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if digits(self) == 0 {
                return None;
            }
        }
        let text = std::str::from_utf8(&self.s[start..self.pos]).ok()?;
        text.parse::<f64>().ok().map(JsonValue::Number)
    }
}

/// `JSON.stringify(value, null, 2)`.
pub(crate) fn stringify_pretty(value: &JsonValue) -> String {
    let mut out = String::new();
    write_value(value, 0, &mut out);
    out
}

fn write_value(value: &JsonValue, level: usize, out: &mut String) {
    match value {
        JsonValue::Null => out.push_str("null"),
        JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        JsonValue::Number(n) => out.push_str(&number_to_string(*n)),
        JsonValue::String(s) => write_string(s, out),
        JsonValue::Array(items) if items.is_empty() => out.push_str("[]"),
        JsonValue::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                out.push_str(&"  ".repeat(level + 1));
                write_value(item, level + 1, out);
            }
            out.push('\n');
            out.push_str(&"  ".repeat(level));
            out.push(']');
        }
        JsonValue::Object(object) if object.is_empty() => out.push_str("{}"),
        JsonValue::Object(object) => {
            out.push('{');
            for (i, (key, item)) in object.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                out.push_str(&"  ".repeat(level + 1));
                write_string(key, out);
                out.push_str(": ");
                write_value(item, level + 1, out);
            }
            out.push('\n');
            out.push_str(&"  ".repeat(level));
            out.push('}');
        }
    }
}

fn write_string(s: &str, out: &mut String) {
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

/// `Number.prototype.toString()` (ECMAScript), `null` pour les valeurs non finies comme `JSON.stringify`.
pub(crate) fn number_to_string(n: f64) -> String {
    if !n.is_finite() {
        return "null".to_owned();
    }
    if n == 0.0 {
        return "0".to_owned();
    }
    let sign = if n < 0.0 { "-" } else { "" };
    let scientific = format!("{:e}", n.abs());
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let k = digits.len() as i32;
    let point = exponent.parse::<i32>().unwrap_or(0) + 1;
    let body = if k <= point && point <= 21 {
        format!("{digits}{}", "0".repeat((point - k) as usize))
    } else if 0 < point && point <= 21 {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    } else if -6 < point && point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        let exp = point - 1;
        let exp_sign = if exp < 0 { "-" } else { "+" };
        let fraction = if k == 1 { String::new() } else { format!(".{}", &digits[1..]) };
        format!("{}{fraction}e{exp_sign}{}", &digits[..1], exp.abs())
    };
    format!("{sign}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn ef_imp_01_json_round_trip_orders_keys_like_javascript() {
        let value = parse(r#"{"b": 1, "2": [true, null, "xé"], "a": {}, "1": 1.50}"#).unwrap();
        assert_eq!(
            stringify_pretty(&value),
            "{\n  \"1\": 1.5,\n  \"2\": [\n    true,\n    null,\n    \"xé\"\n  ],\n  \"b\": 1,\n  \"a\": {}\n}"
        );
        assert!(parse("{'a': 1}").is_none());
        assert!(parse("[1,]").is_none());
    }
}
