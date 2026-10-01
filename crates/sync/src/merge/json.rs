//! Fusion à 3 voies de documents JSON. Les `{{variables}}` hors des chaînes ne sont pas du JSON : elles sont masquées
//! par des chaînes avant l'analyse, puis restaurées dans le résultat (comme le fait Bruno).

use serde_json::{Map, Value};

const OPEN: char = '\u{e000}';
const CLOSE: char = '\u{e001}';

pub(super) struct Merged {
    pub text: String,
    pub conflicts: usize,
}

#[derive(Default)]
struct Masks(Vec<String>);

impl Masks {
    fn placeholder(index: usize) -> String {
        format!("\"{OPEN}{index}{CLOSE}\"")
    }

    fn mask(&mut self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        let mut in_string = false;
        while let Some(c) = rest.chars().next() {
            let len = c.len_utf8();
            if in_string {
                let escaped = if c == '\\' { rest[len..].chars().next().map_or(0, char::len_utf8) } else { 0 };
                in_string = c != '"';
                out.push_str(&rest[..len + escaped]);
                rest = &rest[len + escaped..];
                continue;
            }
            match rest.strip_prefix("{{").and_then(|inner| inner.find("}}")) {
                Some(end) => {
                    let variable = &rest[..end + 4];
                    let index = self.0.iter().position(|known| known == variable).unwrap_or_else(|| {
                        self.0.push(variable.to_owned());
                        self.0.len() - 1
                    });
                    out.push_str(&Self::placeholder(index));
                    rest = &rest[end + 4..];
                }
                None => {
                    in_string = c == '"';
                    out.push(c);
                    rest = &rest[len..];
                }
            }
        }
        out
    }

    fn restore(&self, text: String) -> String {
        self.0.iter().enumerate().fold(text, |text, (i, variable)| text.replace(&Self::placeholder(i), variable))
    }
}

/// Fusion de `base`, `ours` et `theirs`, ou `None` si l'un des trois n'est pas du JSON. En cas de conflit, le texte
/// donne la valeur de l'équipe sur le chemin en conflit et `conflicts` les compte.
pub(super) fn merge(base: &str, ours: &str, theirs: &str) -> Option<Merged> {
    let mut masks = Masks::default();
    let parse = |masks: &mut Masks, text: &str| serde_json::from_str::<Value>(&masks.mask(text)).ok();
    let (base, ours, theirs) = (parse(&mut masks, base)?, parse(&mut masks, ours)?, parse(&mut masks, theirs)?);
    let mut conflicts = 0;
    let merged = value(Some(&base), &ours, &theirs, &mut conflicts);
    let text = masks.restore(serde_json::to_string_pretty(&merged).ok()?);
    Some(Merged { text, conflicts })
}

fn value(base: Option<&Value>, ours: &Value, theirs: &Value, conflicts: &mut usize) -> Value {
    if let (Value::Object(o), Value::Object(t)) = (ours, theirs) {
        return Value::Object(object(base.and_then(Value::as_object), o, t, conflicts));
    }
    if ours == theirs || base == Some(theirs) {
        ours.clone()
    } else if base == Some(ours) {
        theirs.clone()
    } else {
        *conflicts += 1;
        ours.clone()
    }
}

fn object(
    base: Option<&Map<String, Value>>,
    ours: &Map<String, Value>,
    theirs: &Map<String, Value>,
    conflicts: &mut usize,
) -> Map<String, Value> {
    let before = |key: &str| base.and_then(|b| b.get(key));
    let mut out = Map::new();
    for (key, own) in ours {
        match (before(key), theirs.get(key)) {
            (before, Some(spec)) => {
                out.insert(key.clone(), value(before, own, spec, conflicts));
            }
            (Some(before), None) if own == before => {}
            (Some(_), None) => {
                *conflicts += 1;
                out.insert(key.clone(), own.clone());
            }
            (None, None) => {
                out.insert(key.clone(), own.clone());
            }
        }
    }
    for (key, spec) in theirs.iter().filter(|(key, _)| !ours.contains_key(*key)) {
        match before(key) {
            None => {
                out.insert(key.clone(), spec.clone());
            }
            Some(before) if spec != before => *conflicts += 1,
            Some(_) => {}
        }
    }
    out
}
