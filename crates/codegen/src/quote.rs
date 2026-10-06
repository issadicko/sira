//! Littéraux de chaîne : chaque langage échappe un peu différemment.

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    JavaScript,
    Python,
    Go,
    Java,
    Kotlin,
    Dart,
    CSharp,
}

/// `texte` entre guillemets. Dart utilise des apostrophes (comme le veut son style) ; les autres, des guillemets.
pub fn string(text: &str, dialect: Dialect) -> String {
    let quote = if dialect == Dialect::Dart { '\'' } else { '"' };
    let mut out = String::with_capacity(text.len() + 2);
    out.push(quote);
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            '$' if matches!(dialect, Dialect::Kotlin | Dialect::Dart) => out.push_str("\\$"),
            '\u{2028}' | '\u{2029}' if dialect == Dialect::JavaScript => out.push_str(&format!("\\u{:04x}", c as u32)),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => match dialect {
                // Java traite `\uXXXX` avant l'analyse : `\u000a` couperait la ligne. L'octal n'a pas ce défaut.
                Dialect::Java => out.push_str(&format!("\\{:03o}", c as u32)),
                _ => out.push_str(&format!("\\u{:04x}", c as u32)),
            },
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `texte` en littéral PHP entre apostrophes : seuls `\` et `'` s'échappent.
pub fn php(text: &str) -> String {
    format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'"))
}

/// `texte` pour un shell POSIX : entre apostrophes, une apostrophe devient `'\''`.
pub fn shell(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// Un littéral Go : en backticks quand le texte tient sur plusieurs lignes sans rien qui l'interdise, sinon entre guillemets.
pub fn go(text: &str) -> String {
    let raw_ok = text.contains('\n') && !text.contains(['`', '\r', '\0']) && !text.contains('\u{feff}');
    if raw_ok {
        format!("`{text}`")
    } else {
        string(text, Dialect::Go)
    }
}
