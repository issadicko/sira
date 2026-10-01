//! Port de `prettifyJsonString` de Bruno : les `{{variables}}` sont masquées (`patternHasher`), puis le texte
//! est indenté par `format` de `jsonc-parser` 3 (2 espaces), ou par `fast-json-format` si une ligne dépasse
//! 20 000 caractères.

use super::js::{replace_all, utf16, utf16_len};

const LONG_LINE_LIMIT: usize = 20_000;

pub(crate) fn prettify_json_string(text: &str) -> String {
    let (hashed, originals) = hash_variables(text);
    let formatted = if has_long_line(text) { fast_json_format(&hashed) } else { jsonc_format(&hashed) };
    let mut restored = utf16(&formatted);
    for (hash, original) in &originals {
        restored = replace_all(&restored, &utf16(hash), &utf16(original));
    }
    String::from_utf16_lossy(&restored)
}

fn has_long_line(text: &str) -> bool {
    text.split(['\n', '\r']).any(|line| utf16_len(line) > LONG_LINE_LIMIT)
}

/// `patternHasher(input)` : chaque `{{…}}` devient `bruno-var-hash-<djb2>`.
fn hash_variables(input: &str) -> (String, Vec<(String, String)>) {
    let mut originals: Vec<(String, String)> = Vec::new();
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let run = after.find('}').unwrap_or(after.len());
        if run == 0 || !after[run..].starts_with("}}") {
            out.push_str(&rest[..start + 1]);
            rest = &rest[start + 1..];
            continue;
        }
        let end = start + 2 + run + 2;
        let variable = &rest[start..end];
        let hash = format!("bruno-var-hash-{}", djb2(variable));
        match originals.iter_mut().find(|(h, _)| *h == hash) {
            Some(entry) => entry.1 = variable.to_owned(),
            None => originals.push((hash.clone(), variable.to_owned())),
        }
        out.push_str(&rest[..start]);
        out.push_str(&hash);
        rest = &rest[end..];
    }
    out.push_str(rest);
    (out, originals)
}

fn djb2(text: &str) -> i32 {
    text.encode_utf16()
        .fold(5381i32, |hash, c| (i64::from(hash.wrapping_shl(5)) + i64::from(hash) + i64::from(c)) as i32)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Comma,
    Colon,
    Literal,
    StringLiteral,
    Number,
    LineComment,
    BlockComment,
    LineBreak,
    Trivia,
    Unknown,
    Eof,
}

struct Scanner<'a> {
    text: &'a [u8],
    pos: usize,
    offset: usize,
    error: bool,
}

fn is_space(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn is_line_break(b: u8) -> bool {
    b == b'\n' || b == b'\r'
}

impl Scanner<'_> {
    fn at(&self, i: usize) -> Option<u8> {
        self.text.get(i).copied()
    }

    fn end(&self) -> usize {
        self.pos.min(self.text.len())
    }

    fn scan(&mut self) -> Kind {
        self.error = false;
        self.offset = self.pos;
        let Some(code) = self.at(self.pos) else {
            self.offset = self.text.len();
            return Kind::Eof;
        };
        if is_space(code) {
            while self.at(self.pos).is_some_and(is_space) {
                self.pos += 1;
            }
            return Kind::Trivia;
        }
        if is_line_break(code) {
            self.pos += 1;
            if code == b'\r' && self.at(self.pos) == Some(b'\n') {
                self.pos += 1;
            }
            return Kind::LineBreak;
        }
        let single = match code {
            b'{' => Some(Kind::OpenBrace),
            b'}' => Some(Kind::CloseBrace),
            b'[' => Some(Kind::OpenBracket),
            b']' => Some(Kind::CloseBracket),
            b':' => Some(Kind::Colon),
            b',' => Some(Kind::Comma),
            _ => None,
        };
        if let Some(kind) = single {
            self.pos += 1;
            return kind;
        }
        match code {
            b'"' => {
                self.pos += 1;
                self.scan_string();
                Kind::StringLiteral
            }
            b'/' => self.scan_slash(),
            b'-' => {
                self.pos += 1;
                if !self.at(self.pos).is_some_and(|b| b.is_ascii_digit()) {
                    return Kind::Unknown;
                }
                self.scan_number();
                Kind::Number
            }
            b'0'..=b'9' => {
                self.scan_number();
                Kind::Number
            }
            _ => {
                while self.at(self.pos).is_some_and(is_unknown_content) {
                    self.pos += 1;
                }
                if self.offset == self.pos {
                    self.pos += 1;
                    return Kind::Unknown;
                }
                match &self.text[self.offset..self.pos] {
                    b"true" | b"false" | b"null" => Kind::Literal,
                    _ => Kind::Unknown,
                }
            }
        }
    }

    fn scan_slash(&mut self) -> Kind {
        let len = self.text.len();
        match self.at(self.pos + 1) {
            Some(b'/') => {
                self.pos += 2;
                while self.at(self.pos).is_some_and(|b| !is_line_break(b)) {
                    self.pos += 1;
                }
                Kind::LineComment
            }
            Some(b'*') => {
                self.pos += 2;
                let mut closed = false;
                while self.pos < len.saturating_sub(1) {
                    let ch = self.text[self.pos];
                    if ch == b'*' && self.at(self.pos + 1) == Some(b'/') {
                        self.pos += 2;
                        closed = true;
                        break;
                    }
                    self.pos += 1;
                    if ch == b'\r' && self.at(self.pos) == Some(b'\n') {
                        self.pos += 1;
                    }
                }
                if !closed {
                    self.pos += 1;
                    self.error = true;
                }
                Kind::BlockComment
            }
            _ => {
                self.pos += 1;
                Kind::Unknown
            }
        }
    }

    fn scan_string(&mut self) {
        loop {
            let Some(ch) = self.at(self.pos) else {
                self.error = true;
                return;
            };
            if ch == b'"' {
                self.pos += 1;
                return;
            }
            if ch == b'\\' {
                self.pos += 1;
                let Some(escape) = self.at(self.pos) else {
                    self.error = true;
                    return;
                };
                self.pos += 1;
                match escape {
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                    b'u' => {
                        let digits = self.text[self.pos..].iter().take(4).take_while(|b| b.is_ascii_hexdigit()).count();
                        self.pos += digits;
                        self.error |= digits < 4;
                    }
                    _ => self.error = true,
                }
                continue;
            }
            if ch <= 0x1f {
                if is_line_break(ch) {
                    self.error = true;
                    return;
                }
                self.error = true;
            }
            self.pos += 1;
        }
    }

    fn scan_number(&mut self) {
        let digit = |s: &Self, i: usize| s.at(i).is_some_and(|b| b.is_ascii_digit());
        if self.at(self.pos) == Some(b'0') {
            self.pos += 1;
        } else {
            self.pos += 1;
            while digit(self, self.pos) {
                self.pos += 1;
            }
        }
        if self.at(self.pos) == Some(b'.') {
            self.pos += 1;
            if !digit(self, self.pos) {
                self.error = true;
                return;
            }
            while digit(self, self.pos) {
                self.pos += 1;
            }
        }
        if matches!(self.at(self.pos), Some(b'E' | b'e')) {
            self.pos += 1;
            if matches!(self.at(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !digit(self, self.pos) {
                self.error = true;
                return;
            }
            while digit(self, self.pos) {
                self.pos += 1;
            }
        }
    }
}

fn is_unknown_content(b: u8) -> bool {
    !is_space(b) && !is_line_break(b) && !matches!(b, b'}' | b']' | b'{' | b'[' | b'"' | b':' | b',' | b'/')
}

fn detect_eol(text: &str) -> &'static str {
    match text.find(['\r', '\n']) {
        Some(i) if text[i..].starts_with("\r\n") => "\r\n",
        Some(i) if text[i..].starts_with('\r') => "\r",
        _ => "\n",
    }
}

struct Formatter<'a> {
    scanner: Scanner<'a>,
    line_breaks: bool,
    has_error: bool,
    level: i64,
    eol: &'static str,
    edits: Vec<(usize, usize, String)>,
}

impl Formatter<'_> {
    fn scan_next(&mut self) -> Kind {
        let mut token = self.scanner.scan();
        self.line_breaks = false;
        while matches!(token, Kind::Trivia | Kind::LineBreak) {
            self.line_breaks |= token == Kind::LineBreak;
            token = self.scanner.scan();
        }
        self.has_error = token == Kind::Unknown || self.scanner.error;
        token
    }

    fn newline(&self) -> String {
        format!("{}{}", self.eol, "  ".repeat(self.level.max(0) as usize))
    }

    fn add_edit(&mut self, content: String, start: usize, end: usize) {
        let text = self.scanner.text;
        let (start, end) = (start.min(text.len()), end.min(text.len()));
        if !self.has_error && start <= end && text[start..end] != *content.as_bytes() {
            self.edits.push((start, end, content));
        }
    }

    fn token_end(&self) -> usize {
        self.scanner.end()
    }
}

fn is_comment(kind: Kind) -> bool {
    matches!(kind, Kind::LineComment | Kind::BlockComment)
}

/// `applyEdits(text, format(text, undefined, { tabSize: 2, insertSpaces: true }))`.
pub(crate) fn jsonc_format(text: &str) -> String {
    let mut f = Formatter {
        scanner: Scanner { text: text.as_bytes(), pos: 0, offset: 0, error: false },
        line_breaks: false,
        has_error: false,
        level: 0,
        eol: detect_eol(text),
        edits: Vec::new(),
    };
    let mut first = f.scan_next();
    if first != Kind::Eof {
        let start = f.scanner.offset;
        f.add_edit(String::new(), 0, start);
    }
    while first != Kind::Eof {
        let mut first_end = f.token_end();
        let mut second = f.scan_next();
        let mut replace = String::new();
        let mut needs_line_break = false;
        while !f.line_breaks && is_comment(second) {
            let comment_start = f.scanner.offset;
            f.add_edit(" ".into(), first_end, comment_start);
            first_end = f.token_end();
            needs_line_break = second == Kind::LineComment;
            replace = if needs_line_break { f.newline() } else { String::new() };
            second = f.scan_next();
        }
        match (second, first) {
            (Kind::CloseBrace, Kind::OpenBrace) | (Kind::CloseBracket, Kind::OpenBracket) => {}
            (Kind::CloseBrace | Kind::CloseBracket, _) => {
                f.level -= 1;
                replace = f.newline();
            }
            _ => {
                match first {
                    Kind::OpenBrace | Kind::OpenBracket => {
                        f.level += 1;
                        replace = f.newline();
                    }
                    Kind::Comma | Kind::LineComment => replace = f.newline(),
                    Kind::BlockComment if f.line_breaks => replace = f.newline(),
                    Kind::BlockComment | Kind::Colon if !needs_line_break => replace = " ".into(),
                    Kind::StringLiteral if second == Kind::Colon && !needs_line_break => replace = String::new(),
                    Kind::Literal | Kind::Number | Kind::CloseBrace | Kind::CloseBracket => {
                        if is_comment(second) && !needs_line_break {
                            replace = " ".into();
                        } else if second != Kind::Comma && second != Kind::Eof {
                            f.has_error = true;
                        }
                    }
                    Kind::Unknown => f.has_error = true,
                    _ => {}
                }
                if f.line_breaks && is_comment(second) {
                    replace = f.newline();
                }
            }
        }
        if second == Kind::Eof {
            replace = String::new();
        }
        let second_start = f.scanner.offset;
        f.add_edit(replace, first_end, second_start);
        first = second;
    }
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (start, end, content) in &f.edits {
        out.push_str(&text[last..*start]);
        out.push_str(content);
        last = *end;
    }
    out.push_str(&text[last..]);
    out
}

/// `fastJsonFormat(input, '  ')` de `fast-json-format` 0.4.
pub(crate) fn fast_json_format(input: &str) -> String {
    let s: Vec<u16> = utf16(input);
    let n = s.len();
    let unit = |c: u8| u16::from(c);
    let is_space = |c: u16| c == unit(b' ') || c == unit(b'\t') || c == unit(b'\n') || c == unit(b'\r');
    let skip_ws = |mut i: usize| {
        while i < n && is_space(s[i]) {
            i += 1;
        }
        i
    };
    let indent = |level: usize| utf16(&"  ".repeat(level));
    let mut out: Vec<u16> = Vec::with_capacity(n * 2);
    let mut level = 0usize;
    let mut i = 0;
    while i < n {
        i = skip_ws(i);
        if i >= n {
            break;
        }
        let c = s[i];
        if c == unit(b'"') {
            i = scan_fast_string(&s, i, &mut out);
        } else if c == unit(b'{') || c == unit(b'[') {
            let close = if c == unit(b'{') { unit(b'}') } else { unit(b']') };
            let k = skip_ws(i + 1);
            if k < n && s[k] == close {
                out.extend([c, close]);
                i = k + 1;
                continue;
            }
            out.push(c);
            out.push(unit(b'\n'));
            out.extend(indent(level + 1));
            level += 1;
            i += 1;
        } else if c == unit(b'}') || c == unit(b']') {
            level = level.saturating_sub(1);
            out.push(unit(b'\n'));
            out.extend(indent(level));
            out.push(c);
            i += 1;
        } else if c == unit(b',') {
            out.push(c);
            out.push(unit(b'\n'));
            out.extend(indent(level));
            i += 1;
        } else if c == unit(b':') {
            out.extend([c, unit(b' ')]);
            i += 1;
        } else {
            let start = i;
            while i < n && !is_space(s[i]) && !b"\"{}[],:".iter().any(|&b| s[i] == unit(b)) {
                i += 1;
            }
            out.extend_from_slice(&s[start..i]);
        }
    }
    String::from_utf16_lossy(&out)
}

fn scan_fast_string(s: &[u16], i: usize, out: &mut Vec<u16>) -> usize {
    let n = s.len();
    let quote = u16::from(b'"');
    let backslash = u16::from(b'\\');
    out.push(quote);
    let mut j = i + 1;
    let mut last_copy = j;
    while j < n {
        let c = s[j];
        if c == quote {
            out.extend_from_slice(&s[last_copy..j]);
            out.push(quote);
            return j + 1;
        }
        if c == backslash {
            let backslash_pos = j;
            j += 1;
            if j < n && s[j] == u16::from(b'u') {
                let hex: Option<u16> = s
                    .get(j + 1..j + 5)
                    .and_then(|digits| String::from_utf16(digits).ok())
                    .filter(|digits| digits.chars().all(|c| c.is_ascii_hexdigit()))
                    .and_then(|digits| u16::from_str_radix(&digits, 16).ok());
                if let Some(code) = hex {
                    out.extend_from_slice(&s[last_copy..backslash_pos]);
                    out.push(code);
                    j += 5;
                    last_copy = j;
                    continue;
                }
                j = backslash_pos + 1;
            } else if j < n && s[j] == u16::from(b'/') {
                out.extend_from_slice(&s[last_copy..backslash_pos]);
                out.push(u16::from(b'/'));
                j += 1;
                last_copy = j;
                continue;
            }
            if j < n {
                j += 1;
            }
            continue;
        }
        j += 1;
    }
    out.extend_from_slice(&s[last_copy..n]);
    n
}
