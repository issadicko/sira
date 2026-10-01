//! Noms de fichiers et de dossiers : `sanitizeName` et `validateName` (`bruno-common/src/utils/naming.ts`),
//! `getSafePathToWrite` et `nextSuffixedName` (`bruno-electron/src/utils/filesystem.js`).

use std::collections::HashSet;

use unicode_normalization::UnicodeNormalization;
use xc_core::collection::{is_hidden, REQUEST_EXT};
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

/// Validation du formulaire de Bruno : nom obligatoire de 255 caractères au plus, nom de fichier `sanitizeName(nom)`
/// valide et hors des noms réservés `collection` et `folder`. Écart avec Bruno : le fichier final ne peut être ni
/// caché ni réservé (`folder.yml`, `opencollection.yml`, point de tête, nom de périphérique Windows, sans tenir compte
/// de la casse), car l'arbre de la collection ne le montrerait pas. Renvoie le nom du fichier avec son extension.
pub fn request_file_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim_matches(is_js_space);
    if trimmed.is_empty() {
        return Err("le nom est obligatoire".into());
    }
    if trimmed.encode_utf16().count() > MAX_NAME {
        return Err(format!("le nom ne peut pas dépasser {MAX_NAME} caractères"));
    }
    let sanitized = sanitize_name(name);
    let filename = sanitized.trim_matches(is_js_space);
    if matches!(filename, "collection" | "folder") {
        return Err(format!("les noms de fichier « collection » et « folder » sont réservés : {filename}"));
    }
    validate_name(filename)?;
    let base = filename.replacen(REQUEST_EXT, "", 1);
    validate_name(&base)?;
    let file = fit(&base, "", REQUEST_EXT);
    if is_hidden(&file.to_lowercase(), false) || is_device_name(&file) {
        return Err(format!("le nom de fichier « {file} » est réservé ou caché"));
    }
    Ok(file)
}

/// Validation du nom d'un dossier (`renderer:new-folder` de Bruno) : `sanitizeName(nom)` valide. Écart avec Bruno : le
/// dossier ne peut pas porter un nom que l'arbre cacherait (`.git`, `.oc-sync`, `node_modules`, et à la racine
/// `environments` et `mocks`, sans tenir compte de la casse). Renvoie le nom du dossier.
pub fn folder_dir_name(name: &str, at_root: bool) -> Result<String, String> {
    let sanitized = sanitize_name(name);
    validate_name(&sanitized)?;
    let dir = fit(&sanitized, "", "");
    if is_hidden(&dir.to_lowercase(), at_root) {
        return Err(format!("le nom de dossier « {dir} » est réservé ou caché"));
    }
    Ok(dir)
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

/// Clé de comparaison de deux noms de fichier : normalisation NFC puis repli de casse. Sur un disque insensible à la
/// casse ou à la normalisation Unicode (APFS, HFS+, NTFS), deux noms qui ne diffèrent que par elles désignent le même
/// fichier : `été` écrit NFD est `été` écrit NFC.
pub fn fold(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase().nfc().collect()
}

/// Noms déjà pris dans un dossier, comparés par [`fold`].
pub struct Directory {
    taken: HashSet<String>,
    listed_at_root: Option<bool>,
    ignored: Vec<String>,
}

impl Directory {
    pub fn new(reserved: &[&str]) -> Self {
        Self { taken: reserved.iter().map(|name| fold(name)).collect(), listed_at_root: None, ignored: Vec::new() }
    }

    /// Les noms attribués doivent rester visibles de `open_collection` ; `at_root` : le dossier est la racine.
    pub fn listed(mut self, at_root: bool) -> Self {
        self.listed_at_root = Some(at_root);
        self
    }

    /// Les noms attribués ne doivent pas figurer dans `extensions.bruno.ignore`, que l'arbre n'affiche pas.
    pub fn ignoring(mut self, ignored: &[String]) -> Self {
        self.ignored = ignored.to_vec();
        self
    }

    fn hidden(&self, name: &str) -> bool {
        self.listed_at_root.is_some_and(|at_root| is_hidden(name, at_root)) || self.ignored.iter().any(|i| i == name)
    }

    /// `name` tel quel, s'il est libre et que l'arbre le montrerait ; sinon `None`. Il n'est pas assaini ni renommé :
    /// un nom de périphérique Windows déjà porté par l'élément qu'on déplace reste le sien.
    pub fn claim_exact(&mut self, name: &str) -> Option<String> {
        let folded = fold(name);
        let free = !self.hidden(name) && !self.taken.contains(&folded);
        free.then(|| {
            self.taken.insert(folded);
            name.to_owned()
        })
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
            let folded = fold(&name);
            if !is_device_name(&name)
                && !self.hidden(&name)
                && !self.taken.contains(&folded)
                && slot(&name) != Slot::Other
            {
                self.taken.insert(folded);
                return name;
            }
            counter += 1;
        }
    }
}
