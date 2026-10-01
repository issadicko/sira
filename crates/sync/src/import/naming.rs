//! Noms de fichiers et de dossiers : `sanitizeName` et `validateName` (`bruno-common/src/utils/naming.ts`),
//! `getSafePathToWrite` et `nextSuffixedName` (`bruno-electron/src/utils/filesystem.js`).

use std::collections::HashSet;

use xc_core::collection::is_hidden;
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

/// Windows réserve aussi ces noms suivis d'une extension (`con.txt`) : le radical est ce qui précède le premier point.
pub fn is_device_name(name: &str) -> bool {
    is_reserved(name.split('.').next().unwrap_or(name))
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

/// Radical du fichier d'un élément : `sanitizeName("<nom><ext>")` privé de son extension et de ses points de tête,
/// que l'arbre des collections cacherait.
pub fn stem(name: &str, ext: &str, fallback: &str) -> String {
    let sanitized = sanitize_name(&format!("{name}{ext}"));
    let visible = sanitized
        .strip_suffix(ext)
        .map(|stem| stem.trim_start_matches(|c| c == '.' || c == '-' || is_js_space(c)))
        .filter(|stem| !stem.is_empty());
    visible.unwrap_or(fallback).to_owned()
}

/// Dossier de la collection nommée `title`, avec le nom qu'elle porte : `title`, puis `title - 1`, `title - 2`…
/// tant que `taken` refuse le dossier, qui reste sous la limite de taille avec son suffixe. Un titre réduit à des
/// caractères interdits devient `Untitled Collection` ; un nom de périphérique réservé reçoit aussi un suffixe.
pub fn collection_folder(title: &str, taken: impl Fn(&str) -> bool) -> (String, String) {
    let sanitized = sanitize_name(title);
    let base = if sanitized.is_empty() { "Untitled Collection" } else { &sanitized };
    let mut counter = 0;
    loop {
        let (suffix, name) = match counter {
            0 => (String::new(), title.to_owned()),
            n => (format!(" - {n}"), format!("{title} - {n}")),
        };
        let folder = with_suffix(base, &suffix, "");
        if !is_device_name(&folder) && !taken(&folder) {
            return (folder, name);
        }
        counter += 1;
    }
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

/// [`fit`] avec le suffixe placé après le radical quand celui-ci est un nom de périphérique réservé
/// (`con.txt` devient `con 1.txt`) : à la fin du nom, il laisserait le radical réservé.
fn with_suffix(stem: &str, suffix: &str, ext: &str) -> String {
    match stem.split_once('.') {
        Some((radical, rest)) if is_reserved(radical) => fit(&format!("{radical}{suffix}.{rest}"), "", ext),
        _ => fit(stem, suffix, ext),
    }
}

/// Ce que le disque contient sous un nom candidat.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    Free,
    Same,
    Other,
}

/// Noms déjà pris dans un dossier, casse ignorée : sur un disque insensible à la casse, deux noms qui ne
/// diffèrent que par elle seraient le même fichier.
pub struct Directory {
    taken: HashSet<String>,
    listed_at_root: Option<bool>,
}

impl Directory {
    pub fn new(reserved: &[&str]) -> Self {
        Self { taken: reserved.iter().map(|name| name.to_lowercase()).collect(), listed_at_root: None }
    }

    /// Les noms attribués doivent rester visibles de `open_collection` ; `at_root` : le dossier est la racine.
    pub fn listed(mut self, at_root: bool) -> Self {
        self.listed_at_root = Some(at_root);
        self
    }

    /// Premier nom libre parmi `radical`, `radical 1`, `radical 2`… avec l'extension `ext`.
    /// Écart avec Bruno : un nom de périphérique Windows ou un nom que l'arbre cacherait (`node_modules`, `mocks` à la
    /// racine, `opencollection.yml`, `folder.yml`) reçoit aussi un suffixe.
    pub fn claim(&mut self, stem: &str, ext: &str) -> String {
        self.claim_reusing(stem, ext, |_| Slot::Free)
    }

    /// Comme [`Directory::claim`], mais un nom déjà pris sur le disque est réutilisé quand `slot` le dit identique.
    pub fn claim_reusing(&mut self, stem: &str, ext: &str, slot: impl Fn(&str) -> Slot) -> String {
        let mut counter = 0;
        loop {
            let suffix = if counter == 0 { String::new() } else { format!(" {counter}") };
            let name = with_suffix(stem, &suffix, ext);
            let hidden = self.listed_at_root.is_some_and(|at_root| is_hidden(&name, at_root));
            let lower = name.to_lowercase();
            if !is_device_name(&name) && !hidden && !self.taken.contains(&lower) && slot(&name) != Slot::Other {
                self.taken.insert(lower);
                return name;
            }
            counter += 1;
        }
    }
}
