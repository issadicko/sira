//! Découpage du format `.bru` : la grammaire `Bru` de bruno-lang v2 (PEG, sans backtracking sauf là où elle en
//! prévoit). Un fichier est une suite de blocs `nom { … }` : un dictionnaire de `clé: valeur` ou un bloc de texte. Un
//! bloc se termine à la première ligne qui commence par `}` ; une valeur tient sur une ligne ou entre `'''`.
//!
//! Un bloc de nom inconnu (fichier écrit par une version plus récente de Bruno) est sauté et signalé, là où Bruno
//! refuserait tout le fichier.

use std::fmt;

use crate::js::trim;

#[derive(Debug, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ligne {} : {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

type R<T> = Result<T, ParseError>;

const UNCLOSED: &str = "bloc non fermé : une ligne `}` est attendue";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Annotation {
    pub name: String,
    pub value: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Text(String),
    List(Vec<String>),
}

impl Value {
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text(s) => Some(s),
            Self::List(_) => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pair {
    pub key: String,
    pub value: Value,
    pub annotations: Vec<Annotation>,
}

/// Ce que contient un bloc, selon son nom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `clé: valeur`.
    Pairs,
    /// `expression: opérateur valeur` : la clé peut contenir des espaces.
    Assert,
    /// Texte libre (corps, scripts, tests, docs).
    Text,
    /// Texte gardé tel quel jusqu'à la fin du bloc (`example`).
    Raw,
}

#[derive(Clone, Debug)]
pub enum Content {
    Pairs(Vec<Pair>),
    Text(String),
    Raw(String),
}

#[derive(Clone, Debug)]
pub struct Block {
    pub name: String,
    pub content: Content,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub blocks: Vec<Block>,
    /// Blocs de nom inconnu, avec leur ligne.
    pub skipped: Vec<(usize, String)>,
}

/// Façon d'écrire les valeurs sur plusieurs lignes : les fichiers d'environnement ouvrent `'''` seul sur sa ligne.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Request,
    Environment,
}

/// Lit un fichier `.bru` de requête, de dossier ou de collection ; `kind` dit quel contenu porte chaque nom de bloc.
pub fn parse(text: &str, kind: impl Fn(&str) -> Option<Kind>) -> R<Parsed> {
    let mut s = Scanner::new(text, Dialect::Request);
    let mut parsed = Parsed::default();
    loop {
        s.skip_space();
        if s.eof() {
            return Ok(parsed);
        }
        let line = s.line();
        let name = s.name();
        if name.is_empty() {
            return Err(s.error(format!("« {} » n'est pas un bloc", s.snippet())));
        }
        let Some(kind) = kind(&name) else {
            s.skip_unknown(&name)?;
            parsed.skipped.push((line, name));
            continue;
        };
        let content = match kind {
            Kind::Pairs | Kind::Assert => {
                s.open(&name)?;
                Content::Pairs(s.pairs(kind == Kind::Assert)?)
            }
            Kind::Text => {
                s.open(&name)?;
                Content::Text(s.text_block()?)
            }
            Kind::Raw => {
                s.open(&name)?;
                Content::Raw(s.raw_block()?)
            }
        };
        parsed.blocks.push(Block { name, content });
    }
}

pub struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    dialect: Dialect,
}

impl<'a> Scanner<'a> {
    pub fn new(text: &'a str, dialect: Dialect) -> Self {
        Self { text, pos: 0, dialect }
    }

    pub fn eof(&self) -> bool {
        self.pos >= self.text.len()
    }

    pub fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    pub fn line(&self) -> usize {
        self.text[..self.pos].matches('\n').count() + 1
    }

    pub fn error(&self, message: impl Into<String>) -> ParseError {
        ParseError { line: self.line(), message: message.into() }
    }

    /// Le début de ce qui reste, pour un message d'erreur.
    pub fn snippet(&self) -> String {
        let line = self.rest().lines().next().unwrap_or_default();
        let shown: String = line.chars().take(40).collect();
        if shown.len() < line.len() {
            format!("{shown}…")
        } else {
            shown
        }
    }

    /// Avance de `n` octets (la fin d'un jeton déjà examiné par l'appelant).
    pub fn skip_bytes(&mut self, n: usize) {
        self.pos = (self.pos + n).min(self.text.len());
    }

    pub fn eat(&mut self, token: &str) -> bool {
        let found = self.rest().starts_with(token);
        if found {
            self.pos += token.len();
        }
        found
    }

    /// Espaces et tabulations.
    pub fn skip_st(&mut self) {
        let skipped = self.rest().len() - self.rest().trim_start_matches([' ', '\t']).len();
        self.pos += skipped;
    }

    /// Tout caractère de contrôle ou espace : le `space` d'Ohm, qui sépare les blocs.
    pub fn skip_space(&mut self) {
        let skipped = self.rest().len() - self.rest().trim_start_matches(|c: char| c <= ' ').len();
        self.pos += skipped;
    }

    fn newline_len(&self, at: usize) -> usize {
        let rest = &self.text[at..];
        if rest.starts_with("\r\n") {
            2
        } else {
            usize::from(rest.starts_with('\n'))
        }
    }

    /// Un saut de ligne suivi de `}` en début de ligne : la fin d'un bloc. Renvoie la position après `}`.
    fn tagend_at(&self, at: usize) -> Option<usize> {
        let nl = self.newline_len(at);
        (nl > 0 && self.text[at + nl..].starts_with('}')).then_some(at + nl + 1)
    }

    /// Le nom d'un bloc : jusqu'au premier blanc ou à `{`.
    pub fn name(&mut self) -> String {
        let rest = self.rest();
        let end = rest.find(|c: char| c <= ' ' || c == '{').unwrap_or(rest.len());
        self.pos += end;
        rest[..end].to_owned()
    }

    /// `st* "{"` après le nom du bloc.
    pub fn open(&mut self, name: &str) -> R<()> {
        self.skip_st();
        if self.eat("{") {
            Ok(())
        } else {
            Err(self.error(format!("« {{ » attendu après « {name} »")))
        }
    }

    /// Saute un bloc inconnu jusqu'à sa fin.
    pub fn skip_unknown(&mut self, name: &str) -> R<()> {
        self.open(name)?;
        let (_, after) = self.find_tagend(self.pos, &format!("le bloc « {name} » n'est pas fermé"))?;
        self.pos = after;
        Ok(())
    }

    // -----------------------------------------------------------------------------------------------------------
    // Blocs de texte

    /// Le contenu d'un bloc de texte, désindenté de deux espaces (`outdentString`).
    pub fn text_block(&mut self) -> R<String> {
        while self.newline_len(self.pos) > 0 {
            self.pos += self.newline_len(self.pos);
        }
        let start = self.pos;
        if self.eat("}") {
            return Ok(String::new());
        }
        let (end, after) = self.find_tagend(start, UNCLOSED)?;
        self.pos = after;
        Ok(outdent(&self.text[start..end], 2))
    }

    fn raw_block(&mut self) -> R<String> {
        while self.newline_len(self.pos) > 0 {
            self.pos += self.newline_len(self.pos);
        }
        let start = self.pos;
        if self.eat("}") {
            return Ok(String::new());
        }
        let (end, after) = self.find_tagend(start, UNCLOSED)?;
        self.pos = after;
        Ok(self.text[start..end].to_owned())
    }

    /// Position du saut de ligne qui précède la première ligne commençant par `}`, et celle qui suit ce `}`.
    fn find_tagend(&self, from: usize, unclosed: &str) -> R<(usize, usize)> {
        for (offset, byte) in self.text.as_bytes()[from..].iter().enumerate() {
            if matches!(byte, b'\r' | b'\n') {
                if let Some(after) = self.tagend_at(from + offset) {
                    return Ok((from + offset, after));
                }
            }
        }
        let mut at_start = Scanner::new(self.text, self.dialect);
        at_start.pos = from;
        Err(at_start.error(unclosed))
    }

    // -----------------------------------------------------------------------------------------------------------
    // Dictionnaires

    /// Les paires d'un dictionnaire, après `{` et jusqu'à la ligne `}`.
    pub fn pairs(&mut self, assert: bool) -> R<Vec<Pair>> {
        let mut pairs = Vec::new();
        loop {
            self.skip_st();
            if let Some(after) = self.tagend_at(self.pos) {
                self.pos = after;
                return Ok(pairs);
            }
            let nl = self.newline_len(self.pos);
            if nl > 0 {
                self.pos += nl;
                continue;
            }
            if self.eof() {
                return Err(self.error(UNCLOSED));
            }
            pairs.push(self.pair(assert)?);
        }
    }

    fn pair(&mut self, assert: bool) -> R<Pair> {
        let mut annotations = Vec::new();
        while let Some(annotation) = self.annotation_entry() {
            annotations.push(annotation);
        }
        self.skip_st();
        let key = if assert { self.assert_key() } else { self.key() };
        self.skip_st();
        if !self.eat(":") {
            let found = self.snippet();
            let found = if found.is_empty() { "fin de ligne".to_owned() } else { format!("« {found} »") };
            return Err(self.error(format!("« : » attendu après la clé « {key} », trouvé {found}")));
        }
        self.skip_st();
        let mut value = self.value();
        if let Value::Text(text) = &mut value {
            *text = trim(text).to_owned();
        }
        self.skip_st();
        Ok(Pair { key, value, annotations })
    }

    fn key(&mut self) -> String {
        if let Some(quoted) = self.quoted_key() {
            return quoted;
        }
        let rest = self.rest();
        let end = rest.find([' ', '\t', '\n', '\r', ':']).unwrap_or(rest.len());
        self.pos += end;
        trim(&rest[..end]).to_owned()
    }

    fn assert_key(&mut self) -> String {
        let rest = self.rest();
        let end = rest.find(['\n', '\r', ':']).unwrap_or(rest.len());
        self.pos += end;
        trim(&rest[..end]).to_owned()
    }

    /// `~"clé \" guillemet"` : la clé entre guillemets, le `~` de tête (désactivé) conservé.
    fn quoted_key(&mut self) -> Option<String> {
        let start = self.pos;
        let disabled = self.eat("~");
        if !self.eat("\"") {
            self.pos = start;
            return None;
        }
        let mut key = String::from(if disabled { "~" } else { "" });
        let mut chars = self.rest().char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            match c {
                '\\' if self.rest()[i + 1..].starts_with('"') => {
                    key.push('"');
                    chars.next();
                }
                '"' => {
                    self.pos += i + 1;
                    return Some(key);
                }
                '\n' | '\r' => break,
                other => key.push(other),
            }
        }
        self.pos = start;
        None
    }

    // -----------------------------------------------------------------------------------------------------------
    // Valeurs

    fn value(&mut self) -> Value {
        if self.dialect == Dialect::Request {
            if let Some(list) = self.list() {
                return Value::List(list);
            }
        }
        if let Some(text) = self.multiline() {
            return Value::Text(text);
        }
        Value::Text(self.single_line())
    }

    fn single_line(&mut self) -> String {
        let rest = self.rest();
        let mut end = rest.find('\n').unwrap_or(rest.len());
        if end > 0 && rest[..end].ends_with('\r') {
            end -= 1;
        }
        if self.dialect == Dialect::Environment {
            if let Some(start) = block_start(rest) {
                end = end.min(start);
            }
        }
        self.pos += end;
        trim(&rest[..end]).to_owned()
    }

    /// `[` suivi de sauts de ligne, d'éléments simples (lettres, chiffres, `_`, `-`) un par ligne, puis `]`.
    fn list(&mut self) -> Option<Vec<String>> {
        let start = self.pos;
        let list = self.try_list();
        if list.is_none() {
            self.pos = start;
        }
        list
    }

    fn try_list(&mut self) -> Option<Vec<String>> {
        self.skip_st();
        if !self.eat("[") {
            return None;
        }
        if !self.newlines() {
            return None;
        }
        let mut items = Vec::new();
        if let Some(first) = self.list_item() {
            items.push(first);
            loop {
                let save = self.pos;
                if !self.newlines() {
                    break;
                }
                match self.list_item() {
                    Some(item) => items.push(item),
                    None => {
                        self.pos = save;
                        break;
                    }
                }
            }
        }
        self.skip_st();
        if !self.newlines() {
            return None;
        }
        self.skip_st();
        self.eat("]").then_some(items)
    }

    /// `nl+`.
    fn newlines(&mut self) -> bool {
        let before = self.pos;
        while self.newline_len(self.pos) > 0 {
            self.pos += self.newline_len(self.pos);
        }
        self.pos > before
    }

    fn list_item(&mut self) -> Option<String> {
        let start = self.pos;
        self.skip_st();
        if self.pos == start {
            return None;
        }
        let rest = self.rest();
        let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).unwrap_or(rest.len());
        if end == 0 {
            self.pos = start;
            return None;
        }
        self.pos += end;
        self.skip_st();
        Some(rest[..end].to_owned())
    }

    /// `'''…'''` suivi, au besoin, de `@contentType(…)`.
    fn multiline(&mut self) -> Option<String> {
        match self.dialect {
            Dialect::Request => self.multiline_request(),
            Dialect::Environment => self.multiline_environment(),
        }
    }

    fn multiline_request(&mut self) -> Option<String> {
        let rest = self.rest();
        let body = rest.strip_prefix("'''")?;
        let close = body.find("'''")?;
        let content = &body[..close];
        self.pos += 3 + close + 3;
        let joined = content.split('\n').map(|line| drop_chars(line, 4)).collect::<Vec<_>>().join("\n");

        let after = self.rest();
        let blanks = after.len() - after.trim_start_matches([' ', '\t']).len();
        let annotation = after[blanks..]
            .strip_prefix("@contentType(")
            .and_then(|tail| tail.find(')').map(|end| &after[blanks..blanks + "@contentType(".len() + end + 1]));
        match annotation {
            Some(annotation) => {
                self.pos += blanks + annotation.len();
                Some(format!("{joined} {annotation}"))
            }
            None => Some(joined),
        }
    }

    /// Environnement : ''' seul sur sa ligne, contenu indenté de quatre espaces, ''' fermant en début de ligne.
    fn multiline_environment(&mut self) -> Option<String> {
        let after_open = self.rest().strip_prefix("'''")?;
        let open_nl = if after_open.starts_with("\r\n") {
            2
        } else if after_open.starts_with('\n') {
            1
        } else {
            return None;
        };
        let body = &after_open[open_nl..];
        let mut from = 0;
        let (content_end, consumed) = loop {
            let at = from + body[from..].find(['\r', '\n'])?;
            let nl = if body[at..].starts_with("\r\n") {
                2
            } else if body[at..].starts_with('\n') {
                1
            } else {
                from = at + 1;
                continue;
            };
            let tail = &body[at + nl..];
            let blanks = tail.len() - tail.trim_start_matches([' ', '\t']).len();
            if tail[blanks..].starts_with("'''") {
                break (at, at + nl + blanks + 3);
            }
            from = at + nl;
        };
        self.pos += 3 + open_nl + consumed;
        let joined = split_lines(&body[..content_end]).map(|line| drop_chars(line, 4)).collect::<Vec<_>>().join("\n");
        Some(trim(&joined).to_owned())
    }

    /// Le reste de la ligne, rogné (`color: red`, `extends: Base`).
    pub fn rest_of_line(&mut self) -> String {
        let rest = self.rest();
        let end = rest.find(['\r', '\n']).unwrap_or(rest.len());
        self.pos += end;
        trim(&rest[..end]).to_owned()
    }

    /// `[ nom, nom ]` d'un fichier d'environnement : des noms séparés par des virgules, chacun pouvant porter des
    /// annotations sur les lignes qui le précèdent.
    pub fn name_list(&mut self) -> R<Vec<(String, Vec<Annotation>)>> {
        self.skip_st();
        if !self.eat("[") {
            return Err(self.error("« [ » attendu"));
        }
        let mut names = Vec::new();
        loop {
            self.skip_space();
            let mut annotations = Vec::new();
            while let Some(annotation) = self.annotation_entry() {
                annotations.push(annotation);
            }
            self.skip_st();
            let rest = self.rest();
            let end = rest.find([' ', '\t', '\r', '\n', '[', ']', ',']).unwrap_or(rest.len());
            self.pos += end;
            names.push((rest[..end].to_owned(), annotations));
            self.skip_space();
            if self.eat(",") {
                continue;
            }
            if self.eat("]") {
                return Ok(names);
            }
            return Err(self.error(format!("« , » ou « ] » attendu (« {} »)", self.snippet())));
        }
    }

    // -----------------------------------------------------------------------------------------------------------
    // Annotations : `@description('…')` sur la ligne qui précède une paire.

    pub fn annotation_entry(&mut self) -> Option<Annotation> {
        let start = self.pos;
        let annotation = self.try_annotation();
        if annotation.is_none() {
            self.pos = start;
        }
        annotation
    }

    fn try_annotation(&mut self) -> Option<Annotation> {
        self.skip_st();
        if !self.eat("@") {
            return None;
        }
        let rest = self.rest();
        let end = rest.find(['(', ')', ' ', '\t', '\r', '\n', ':']).unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        let name = rest[..end].to_owned();
        self.pos += end;
        let value = if self.eat("(") {
            let value = self.annotation_argument()?;
            if !self.eat(")") {
                return None;
            }
            Some(value)
        } else {
            None
        };
        if self.rest().starts_with(':') {
            return None;
        }
        self.skip_st();
        let nl = self.newline_len(self.pos);
        if nl == 0 {
            return None;
        }
        self.pos += nl;
        Some(Annotation { name, value })
    }

    fn annotation_argument(&mut self) -> Option<String> {
        let rest = self.rest();
        if let Some(body) = rest.strip_prefix("'''") {
            let close = body.find("'''")?;
            self.pos += 3 + close + 3;
            return Some(unindent_block(&body[..close]));
        }
        if let Some(body) = rest.strip_prefix('\'') {
            let close = body.find('\'')?;
            self.pos += 1 + close + 1;
            return Some(body[..close].to_owned());
        }
        if rest.starts_with('"') {
            let mut escaped = false;
            for (i, c) in rest.char_indices().skip(1) {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    self.pos += i + 1;
                    return Some(unescape_double_quoted(&rest[1..i]));
                }
            }
            return None;
        }
        let close = rest.find(')')?;
        self.pos += close;
        Some(rest[..close].to_owned())
    }
}

/// Position de `'''` suivi d'un saut de ligne.
fn block_start(text: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(at) = text[from..].find("'''") {
        let after = &text[from + at + 3..];
        if after.starts_with('\n') || after.starts_with("\r\n") {
            return Some(from + at);
        }
        from += at + 3;
    }
    None
}

/// Les lignes d'un texte, séparées par `\r\n`, `\r` ou `\n`.
pub fn split_lines(text: &str) -> impl Iterator<Item = &str> {
    let mut rest = Some(text);
    std::iter::from_fn(move || {
        let current = rest?;
        match current.find(['\r', '\n']) {
            Some(at) => {
                let skip = if current[at..].starts_with("\r\n") { 2 } else { 1 };
                rest = Some(&current[at + skip..]);
                Some(&current[..at])
            }
            None => {
                rest = None;
                Some(current)
            }
        }
    })
}

/// `outdentString` : retire jusqu'à `spaces` espaces en tête de chaque ligne ; les fins de ligne deviennent `\n`.
pub fn outdent(text: &str, spaces: usize) -> String {
    split_lines(text)
        .map(|line| {
            let blanks = line.chars().take(spaces).take_while(|c| *c == ' ').count();
            &line[blanks..]
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `line.slice(n)` pour une ligne dont le début est ASCII.
fn drop_chars(line: &str, n: usize) -> &str {
    line.char_indices().nth(n).map_or("", |(i, _)| &line[i..])
}

/// `parseAnnotationMultilineTextBlock` : retire l'indentation commune et les lignes vides de tête et de queue.
fn unindent_block(content: &str) -> String {
    if !content.contains('\n') && !content.contains('\r') {
        return content.to_owned();
    }
    let ending = if content.contains("\r\n") {
        "\r\n"
    } else if content.contains('\r') {
        "\r"
    } else {
        "\n"
    };
    let mut lines: Vec<&str> = split_lines(content).collect();
    if lines.first().is_some_and(|l| l.is_empty()) {
        lines.remove(0);
    }
    if lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let indent = |l: &str| l.len() - l.trim_start_matches([' ', '\t']).len();
    let common = lines.iter().filter(|l| !l.trim().is_empty()).map(|l| indent(l)).min().unwrap_or(0);
    lines.iter().map(|l| if l.trim().is_empty() { "" } else { &l[common..] }).collect::<Vec<_>>().join(ending)
}

/// `unescapeAnnotationDoubleQuotedArg` : `\r`, `\n`, `\t`, `\"` et `\\`.
fn unescape_double_quoted(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(name: &str) -> Option<Kind> {
        Some(match name {
            "meta" | "headers" | "get" => Kind::Pairs,
            "assert" => Kind::Assert,
            "docs" | "body:json" => Kind::Text,
            _ => return None,
        })
    }

    fn pairs(parsed: &Parsed, block: usize) -> Vec<(String, String)> {
        match &parsed.blocks[block].content {
            Content::Pairs(p) => {
                p.iter().map(|p| (p.key.clone(), p.value.text().unwrap_or("[liste]").to_owned())).collect()
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_dictionary_keeps_each_value_to_the_end_of_its_line_and_trims_it() {
        let parsed = parse("meta {\n  name: Hello world  \n  url: http://x/a:b\n}\n", kinds).unwrap();
        assert_eq!(pairs(&parsed, 0), [("name".into(), "Hello world".into()), ("url".into(), "http://x/a:b".into())]);
    }

    #[test]
    fn text_blocks_end_at_the_first_line_starting_with_a_brace_and_are_outdented() {
        let parsed = parse("body:json {\n  {\n    \"a\": 1\n  }\n}\ndocs {\n  hi\n}\n", kinds).unwrap();
        let Content::Text(json) = &parsed.blocks[0].content else { panic!() };
        assert_eq!(json, "{\n  \"a\": 1\n}");
        let Content::Text(docs) = &parsed.blocks[1].content else { panic!() };
        assert_eq!(docs, "hi");
    }

    #[test]
    fn multiline_values_lose_four_spaces_of_indentation_per_line() {
        let parsed = parse("headers {\n  a: '''\n      one\n      two\n    '''\n  b: c\n}\n", kinds).unwrap();
        assert_eq!(pairs(&parsed, 0), [("a".into(), "one\n  two".into()), ("b".into(), "c".into())]);
    }

    #[test]
    fn quoted_keys_may_hold_spaces_and_a_disabled_marker() {
        let parsed = parse("headers {\n  ~\"X Odd: key\": v\n  \"a \\\" b\": w\n}\n", kinds).unwrap();
        assert_eq!(pairs(&parsed, 0), [("~X Odd: key".into(), "v".into()), ("a \" b".into(), "w".into())]);
    }

    #[test]
    fn assert_keys_may_contain_spaces() {
        let parsed = parse("assert {\n  res.body.items length: eq 3\n  ~res.status: eq 200\n}\n", kinds).unwrap();
        assert_eq!(
            pairs(&parsed, 0),
            [("res.body.items length".into(), "eq 3".into()), ("~res.status".into(), "eq 200".into())]
        );
    }

    #[test]
    fn lists_are_read_one_item_per_line_and_inline_brackets_stay_text() {
        let parsed = parse("meta {\n  tags: [\n    smoke\n    re-gression\n  ]\n  other: [a, b]\n}\n", kinds).unwrap();
        let Content::Pairs(p) = &parsed.blocks[0].content else { panic!() };
        assert_eq!(p[0].value, Value::List(vec!["smoke".into(), "re-gression".into()]));
        assert_eq!(p[1].value, Value::Text("[a, b]".into()));
    }

    #[test]
    fn annotations_before_a_pair_are_parsed_and_a_leading_at_sign_in_a_key_is_not_one() {
        let parsed = parse("headers {\n  @description('Pour le support')\n  a: b\n  @local: c\n}\n", kinds).unwrap();
        let Content::Pairs(p) = &parsed.blocks[0].content else { panic!() };
        assert_eq!(
            p[0].annotations,
            [Annotation { name: "description".into(), value: Some("Pour le support".into()) }]
        );
        assert_eq!((p[1].key.as_str(), p[1].annotations.len()), ("@local", 0));
    }

    #[test]
    fn unknown_blocks_are_skipped_and_reported() {
        let parsed = parse("future {\n  x: y\n}\nmeta {\n  name: a\n}\n", kinds).unwrap();
        assert_eq!(parsed.skipped, [(1, "future".to_owned())]);
        assert_eq!(parsed.blocks.len(), 1);
    }

    #[test]
    fn malformed_input_reports_the_line() {
        let error = parse("meta {\n  name: a\n}\nnot a block\n", kinds).unwrap_err();
        assert_eq!(error.line, 4);
        let error = parse("meta {\n  name a\n}\n", kinds).unwrap_err();
        assert!(error.message.contains("« : » attendu"), "{error}");
        let error = parse("meta {\n  name: a\n", kinds).unwrap_err();
        assert!(error.message.contains("non fermé"), "{error}");
    }

    #[test]
    fn crlf_line_endings_are_accepted() {
        let parsed = parse("meta {\r\n  name: a\r\n  seq: 2\r\n}\r\n", kinds).unwrap();
        assert_eq!(pairs(&parsed, 0), [("name".into(), "a".into()), ("seq".into(), "2".into())]);
    }
}
