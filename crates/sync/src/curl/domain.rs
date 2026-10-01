//! `toASCII` de Node 22 (bibliothèque ada) appliqué par `url.parse` aux noms d'hôte : correspondance UTS #46
//! non transitionnelle, NFC, Punycode, validité des étiquettes (marque combinatoire initiale, ContextJ, règles
//! bidirectionnelles telles qu'ada les applique). Renvoie une chaîne vide en cas d'erreur.

use idna_adapter::{Adapter, LAST_RTL_MASK, LEFT_OR_DUAL_JOINING_MASK, MIDDLE_LTR_MASK, MIDDLE_RTL_MASK};
use idna_adapter::{RIGHT_OR_DUAL_JOINING_MASK, RTL_MASK};

const ZWNJ: char = '\u{200C}';
const ZWJ: char = '\u{200D}';

pub(crate) fn to_ascii(hostname: &str) -> String {
    let adapter = Adapter::new();
    let mapped: String = if hostname.is_ascii() {
        hostname.to_ascii_lowercase()
    } else {
        let mapped: String = adapter.map_normalize(hostname.chars()).collect();
        if mapped.contains('\u{FFFD}') {
            return String::new();
        }
        mapped
    };
    let mut labels = Vec::new();
    for label in mapped.split('.') {
        match ascii_label(&adapter, label) {
            Some(label) => labels.push(label),
            None => return String::new(),
        }
    }
    labels.join(".")
}

fn ascii_label(adapter: &Adapter, label: &str) -> Option<String> {
    if let Some(punycode) = label.strip_prefix("xn--") {
        if !label.is_ascii() {
            return None;
        }
        let decoded = punycode_decode(punycode)?;
        let remapped: Vec<char> = adapter.map_normalize(decoded.iter().copied()).collect();
        let valid = remapped == decoded && !decoded.is_empty() && is_label_valid(adapter, &decoded);
        return valid.then(|| label.to_owned());
    }
    if label.is_ascii() {
        return Some(label.to_owned());
    }
    let chars: Vec<char> = label.chars().collect();
    if !is_label_valid(adapter, &chars) {
        return None;
    }
    idna::punycode::encode(&chars).map(|encoded| format!("xn--{encoded}"))
}

fn is_label_valid(adapter: &Adapter, label: &[char]) -> bool {
    let Some(&first) = label.first() else { return true };
    if adapter.is_mark(first) {
        return false;
    }
    if let Some(joiners) = check_joiners(adapter, label) {
        return joiners;
    }
    let Some(last) = label.iter().rposition(|&c| !adapter.bidi_class(c).is_nonspacing_mark()) else {
        return false;
    };
    let is_rtl = label.iter().any(|&c| adapter.bidi_class(c).to_mask().intersects(RTL_MASK));
    if !is_rtl {
        return true;
    }
    if !adapter.bidi_class(label[last]).to_mask().intersects(LAST_RTL_MASK) {
        return false;
    }
    if adapter.bidi_class(first).is_ltr() {
        return label
            .get(1..last)
            .unwrap_or_default()
            .iter()
            .all(|&c| adapter.bidi_class(c).to_mask().intersects(MIDDLE_LTR_MASK));
    }
    let has = |test: fn(&idna_adapter::BidiClass) -> bool| label.iter().any(|&c| test(&adapter.bidi_class(c)));
    label.iter().all(|&c| adapter.bidi_class(c).to_mask().intersects(MIDDLE_RTL_MASK))
        && !(has(|bc| bc.is_european_number()) && has(|bc| bc.is_arabic_number()))
}

/// Règles ContextJ telles qu'ada les applique : le premier ZWNJ/ZWJ rencontré décide de la validité de
/// toute l'étiquette.
fn check_joiners(adapter: &Adapter, label: &[char]) -> Option<bool> {
    let i = label.iter().position(|&c| c == ZWNJ || c == ZWJ)?;
    if i > 0 && adapter.is_virama(label[i - 1]) {
        return Some(true);
    }
    if label[i] == ZWJ || i + 1 >= label.len() {
        return Some(false);
    }
    let joins = |c: char, mask| adapter.joining_type(c).to_mask().intersects(mask);
    Some(
        label[..i].iter().any(|&c| joins(c, LEFT_OR_DUAL_JOINING_MASK))
            && label[i + 1..].iter().any(|&c| joins(c, RIGHT_OR_DUAL_JOINING_MASK)),
    )
}

/// Décodeur Punycode (RFC 3492) d'ada : tout ce qui précède le dernier `-` est recopié tel quel.
fn punycode_decode(input: &str) -> Option<Vec<char>> {
    const BASE: u32 = 36;
    let (basic, mut extended) = match input.rfind('-') {
        Some(p) => (&input[..p], &input[p + 1..]),
        None => ("", input),
    };
    let mut out: Vec<char> = basic.chars().collect();
    let (mut n, mut i, mut bias) = (128u32, 0u32, 72u32);
    while !extended.is_empty() {
        let old_i = i;
        let mut w = 1u32;
        let mut k = BASE;
        loop {
            let byte = *extended.as_bytes().first()?;
            extended = &extended[1..];
            let digit = match byte {
                b'a'..=b'z' => u32::from(byte - b'a'),
                b'A'..=b'Z' => u32::from(byte - b'A'),
                b'0'..=b'9' => u32::from(byte - b'0') + 26,
                _ => return None,
            };
            if digit > (0x7fff_ffff - i) / w {
                return None;
            }
            i += digit * w;
            let t = if k <= bias {
                1
            } else if k >= bias + 26 {
                26
            } else {
                k - bias
            };
            if digit < t {
                break;
            }
            if w > 0x7fff_ffff / (BASE - t) {
                return None;
            }
            w *= BASE - t;
            k += BASE;
        }
        let length = out.len() as u32 + 1;
        bias = adapt(i - old_i, length, old_i == 0);
        if i / length > 0x7fff_ffff - n {
            return None;
        }
        n += i / length;
        i %= length;
        if n < 0x80 {
            return None;
        }
        out.insert(i as usize, char::from_u32(n)?);
        i += 1;
    }
    Some(out)
}

fn adapt(delta: u32, length: u32, first: bool) -> u32 {
    let mut delta = if first { delta / 700 } else { delta / 2 };
    delta += delta / length;
    let mut k = 0;
    while delta > ((36 - 1) * 26) / 2 {
        delta /= 36 - 1;
        k += 36;
    }
    k + (36 * delta) / (delta + 38)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ef_imp_01_to_ascii_matches_node() {
        for (host, expected) in [
            ("example.com", "example.com"),
            ("münchen.de", "xn--mnchen-3ya.de"),
            ("faß.de", "xn--fa-hia.de"),
            ("ⅷ.com", "viii.com"),
            ("0\u{640}", "xn--0-foc"),
            ("a\u{5d0}", "xn--a-0hc"),
            ("\u{5d0}a", ""),
            ("a1\u{661}", "xn--a1-cyd"),
            ("a\u{661}1", ""),
            ("xn--zzxn--", "xn--zzxn--"),
            ("xn---0hc", "xn---0hc"),
            ("xn--a", ""),
            ("a\u{200c}b", ""),
            ("\u{628}\u{200c}\u{628}", "xn--ngba799q"),
            ("\u{300}a", ""),
            ("é_!$&", "xn--_!$&-9oa"),
        ] {
            assert_eq!(to_ascii(host), expected, "{host}");
        }
    }
}
