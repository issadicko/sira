use serde::de::IgnoredAny;

/// Ré-indente un JSON valide sur deux espaces, comme `JSON.stringify(v, null, 2)`, en recopiant
/// chaque jeton à l'identique (nombres, échappements). `None` si le texte n'est pas du JSON.
pub fn pretty_json(text: &str) -> Option<String> {
    serde_json::from_str::<IgnoredAny>(text).ok()?;
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len() + text.len() / 2);
    let mut depth = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let end = string_end(bytes, i);
                out.push_str(&text[i..end]);
                i = end;
                continue;
            }
            open @ (b'{' | b'[') => {
                let next = skip_whitespace(bytes, i + 1);
                out.push(open as char);
                if matches!(bytes[next], b'}' | b']') {
                    out.push(bytes[next] as char);
                    i = next;
                } else {
                    depth += 1;
                    newline(&mut out, depth);
                }
            }
            close @ (b'}' | b']') => {
                depth -= 1;
                newline(&mut out, depth);
                out.push(close as char);
            }
            b',' => {
                out.push(',');
                newline(&mut out, depth);
            }
            b':' => out.push_str(": "),
            b' ' | b'\t' | b'\n' | b'\r' => {}
            _ => {
                let end = atom_end(bytes, i);
                out.push_str(&text[i..end]);
                i = end;
                continue;
            }
        }
        i += 1;
    }
    Some(out)
}

fn string_end(bytes: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while bytes[i] != b'"' {
        i += if bytes[i] == b'\\' { 2 } else { 1 };
    }
    i + 1
}

fn atom_end(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .position(|b| matches!(b, b',' | b':' | b'{' | b'}' | b'[' | b']' | b'"' | b' ' | b'\t' | b'\n' | b'\r'))
        .map_or(bytes.len(), |n| start + n)
}

fn skip_whitespace(bytes: &[u8], start: usize) -> usize {
    bytes[start..].iter().position(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\r')).map_or(bytes.len(), |n| start + n)
}

fn newline(out: &mut String, depth: usize) {
    out.push('\n');
    for _ in 0..depth {
        out.push_str("  ");
    }
}
