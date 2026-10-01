//! `JSON.parse` avec la sémantique JavaScript (nombres flottants, ordre des clés d'objet), pour le corps GraphQL.

use serde_json::Value;

use crate::js::{number_value, JsObject};

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
            JsonValue::Number(n) => number_value(*n),
            JsonValue::String(s) => Value::String(s.clone()),
            JsonValue::Array(items) => Value::Array(items.iter().map(JsonValue::to_value).collect()),
            JsonValue::Object(object) => object.to_json(|v| Some(v.to_value())),
        }
    }
}

/// Imbrication au-delà de laquelle le texte est refusé comme une `SyntaxError` (le parseur est récursif).
const MAX_DEPTH: usize = 512;

/// `JSON.parse(text)` ; `None` correspond à une `SyntaxError`.
pub(crate) fn parse(text: &str) -> Option<JsonValue> {
    let mut parser = Parser { s: text.as_bytes(), pos: 0, depth: 0 };
    let value = parser.value()?;
    parser.skip_ws();
    (parser.pos == parser.s.len()).then_some(value)
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
    depth: usize,
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
            b'{' => self.nested(Self::object),
            b'[' => self.nested(Self::array),
            b'"' => self.string().map(JsonValue::String),
            b't' => self.eat("true").map(|_| JsonValue::Bool(true)),
            b'f' => self.eat("false").map(|_| JsonValue::Bool(false)),
            b'n' => self.eat("null").map(|_| JsonValue::Null),
            _ => self.number(),
        }
    }

    fn nested(&mut self, parse: fn(&mut Self) -> Option<JsonValue>) -> Option<JsonValue> {
        if self.depth == MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let value = parse(self);
        self.depth -= 1;
        value
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js::json_stringify;

    #[test]
    fn ef_imp_01_json_round_trip_orders_keys_like_javascript() {
        let value = parse(r#"{"b": 1, "2": [true, null, "xé"], "a": {}, "1": 1.50}"#).unwrap();
        assert_eq!(
            json_stringify(&value.to_value(), true),
            "{\n  \"1\": 1.5,\n  \"2\": [\n    true,\n    null,\n    \"xé\"\n  ],\n  \"b\": 1,\n  \"a\": {}\n}"
        );
        assert!(parse("{'a': 1}").is_none());
        assert!(parse("[1,]").is_none());
    }
}
