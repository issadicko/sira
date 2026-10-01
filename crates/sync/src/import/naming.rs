//! Noms de fichiers et de dossiers : `sanitizeName` et `validateName` (`bruno-common/src/utils/naming.ts`),
//! `getSafePathToWrite` et `nextSuffixedName` (`bruno-electron/src/utils/filesystem.js`).

use std::collections::HashSet;

use xc_core::yaml::is_js_space;

const MAX_NAME: usize = 255;

fn is_invalid(c: char) -> bool {
    matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c <= '\u{1f}'
}

/// Remplace les caractères interdits par `-`, retire les espaces et tirets de tête, les points et espaces de queue.
pub fn sanitize_name(name: &str) -> String {
    let replaced: String = name.chars().map(|c| if is_invalid(c) { '-' } else { c }).collect();
    replaced
        .trim_start_matches(|c| is_js_space(c) || c == '-')
        .trim_end_matches(|c| c == '.' || is_js_space(c))
        .to_owned()
}

fn is_reserved(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    let numbered =
        |prefix: &str| upper.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit());
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered("COM") || numbered("LPT")
}

/// `validateNameError` : la première raison pour laquelle `name` ne peut pas servir de nom de fichier.
pub fn validate_name(name: &str) -> Result<(), String> {
    let forbidden = |c: char| format!("caractère interdit « {c} » dans le nom");
    let (Some(first), Some(last)) = (name.chars().next(), name.chars().next_back()) else {
        return Err("le nom ne peut pas être vide".into());
    };
    if name.encode_utf16().count() > MAX_NAME {
        return Err(format!("le nom ne peut pas dépasser {MAX_NAME} caractères"));
    }
    if is_reserved(name) {
        return Err("le nom ne peut pas être un nom de périphérique réservé".into());
    }
    if is_js_space(first) || first == '-' || is_invalid(first) {
        return Err(forbidden(first));
    }
    if let Some(c) = name.chars().find(|c| is_invalid(*c)) {
        return Err(forbidden(c));
    }
    if last == '.' || is_js_space(last) {
        return Err(forbidden(last));
    }
    Ok(())
}

/// Radical du fichier d'un élément : `sanitizeName("<nom><ext>")` privé de son extension.
pub fn stem(name: &str, ext: &str, fallback: &str) -> String {
    let sanitized = sanitize_name(&format!("{name}{ext}"));
    match sanitized.strip_suffix(ext) {
        Some(stem) if !stem.is_empty() => stem.to_owned(),
        _ => fallback.to_owned(),
    }
}

/// Nom du dossier d'une collection : celui de `sanitizeName`, borné à la taille d'un nom de fichier.
pub fn folder_name(title: &str) -> String {
    fit(&sanitize_name(title), "", "")
}

/// `getSafePathToWrite` : tronque le radical pour que `radical + suffixe + extension` tienne dans 255 unités UTF-16
/// et 255 octets, le suffixe d'unicité étant toujours conservé.
pub fn fit(stem: &str, suffix: &str, ext: &str) -> String {
    let tail = format!("{suffix}{ext}");
    let fits = |s: &str| s.encode_utf16().count() <= MAX_NAME && s.len() <= MAX_NAME;
    if fits(&format!("{stem}{tail}")) {
        return format!("{stem}{tail}");
    }
    let base = sanitize_name(stem);
    let (room_units, room_bytes) =
        (MAX_NAME.saturating_sub(tail.encode_utf16().count()), MAX_NAME.saturating_sub(tail.len()));
    let (mut units, mut bytes) = (0, 0);
    let end = base.char_indices().find_map(|(i, c)| {
        units += c.len_utf16();
        bytes += c.len_utf8();
        (units > room_units || bytes > room_bytes).then_some(i)
    });
    format!("{}{tail}", &base[..end.unwrap_or(base.len())])
}

/// Noms déjà pris dans un dossier, casse ignorée : sur un disque insensible à la casse, deux noms qui ne
/// diffèrent que par elle seraient le même fichier.
pub struct Directory {
    taken: HashSet<String>,
}

impl Directory {
    pub fn new(reserved: &[&str]) -> Self {
        Self { taken: reserved.iter().map(|name| name.to_lowercase()).collect() }
    }

    /// Premier nom libre parmi `radical`, `radical 1`, `radical 2`… avec l'extension `ext`.
    pub fn claim(&mut self, stem: &str, ext: &str) -> String {
        let mut name = fit(stem, "", ext);
        let mut counter = 0;
        while !self.taken.insert(name.to_lowercase()) {
            counter += 1;
            name = fit(stem, &format!(" {counter}"), ext);
        }
        name
    }
}
