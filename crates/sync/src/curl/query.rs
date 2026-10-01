//! Chaînes de requête : `parseQueryParams` / `buildQueryString` de `@usebruno/common`, `parse` de
//! `query-string` 7 (`sort: false`) et `decode-uri-component` 0.2.

use serde_json::{json, Value};

use crate::js::{decode_uri_component_utf16, replace_all_utf16, trim, utf16, JsObject};

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct QueryParam {
    pub name: String,
    pub value: Option<String>,
}

impl QueryParam {
    pub(crate) fn to_json(&self) -> Value {
        match &self.value {
            Some(value) => json!({ "name": self.name, "value": value }),
            None => json!({ "name": self.name }),
        }
    }
}

/// `parseQueryParams(query, { decode: false })`.
pub(crate) fn parse_query_params(query: Option<&str>) -> Vec<QueryParam> {
    let Some(query) = query.filter(|q| !q.is_empty()) else { return Vec::new() };
    let query = query.split('#').next().unwrap_or_default();
    query
        .split('&')
        .filter_map(|pair| {
            let (name, value) = match pair.split_once('=') {
                Some((name, value)) => (name, Some(value.to_owned())),
                None => (pair, None),
            };
            (!name.is_empty()).then(|| QueryParam { name: name.to_owned(), value })
        })
        .collect()
}

/// `buildQueryString(params, { encode: false })`.
pub(crate) fn build_query_string(params: &[QueryParam]) -> String {
    params
        .iter()
        .filter(|p| !trim(&p.name).is_empty())
        .map(|p| match &p.value {
            Some(value) => format!("{}={value}", p.name),
            None => p.name.clone(),
        })
        .collect::<Vec<_>>()
        .join("&")
}

/// Valeur d'une clé après `query-string.parse` : `null` (pas de `=`), texte, ou liste si la clé se répète.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum QsValue {
    Null,
    Str(String),
    List(Vec<Option<String>>),
}

impl QsValue {
    pub(crate) fn to_json(&self) -> Value {
        match self {
            QsValue::Null => Value::Null,
            QsValue::Str(s) => json!(s),
            QsValue::List(items) => json!(items),
        }
    }

    fn into_items(self) -> Vec<Option<String>> {
        match self {
            QsValue::Null => vec![None],
            QsValue::Str(s) => vec![Some(s)],
            QsValue::List(items) => items,
        }
    }

    /// Ajoute une occurrence de la clé : la valeur devient (ou reste) une liste.
    fn append(&mut self, value: QsValue) {
        if let QsValue::List(items) = self {
            items.extend(value.into_items());
        } else {
            let mut items = std::mem::replace(self, QsValue::Null).into_items();
            items.extend(value.into_items());
            *self = QsValue::List(items);
        }
    }
}

/// `queryString.parse(query, { sort: false })`.
pub(crate) fn query_string_parse(query: &str) -> JsObject<QsValue> {
    let mut result = JsObject::null_proto();
    let mut budget = FALLBACK_BUDGET;
    let query = trim(query);
    let query = query.strip_prefix(['?', '#', '&']).unwrap_or(query);
    if query.is_empty() {
        return result;
    }
    for param in query.split('&').filter(|p| !p.is_empty()) {
        let param = param.replace('+', " ");
        let (key, value) = match param.split_once('=') {
            Some((key, value)) => (key, QsValue::Str(decode_component(value, &mut budget))),
            None => (param.as_str(), QsValue::Null),
        };
        let key = decode_component(key, &mut budget);
        match result.get_mut(&key) {
            Some(existing) => existing.append(value),
            None => result.set(&key, value),
        }
    }
    result
}

/// Travail total, en jetons réécrits, que le repli de `decode-uri-component` s'autorise pour une chaîne de requête :
/// au-delà il s'arrête et laisse le reste tel quel (l'original est cubique et gèle sur des entrées hostiles).
const FALLBACK_BUDGET: usize = 5_000_000;

/// `decode-uri-component` : `decodeURIComponent` puis, en cas d'échec, décodage morceau par morceau.
pub(crate) fn decode_component(input: &str, budget: &mut usize) -> String {
    let units = utf16(&input.replace('+', " "));
    let decoded = decode_uri_component_utf16(&units).unwrap_or_else(|| custom_decode(units, budget));
    String::from_utf16_lossy(&decoded)
}

fn custom_decode(mut input: Vec<u16>, budget: &mut usize) -> Vec<u16> {
    let replacement_char = utf16("\u{FFFD}\u{FFFD}");
    let mut replacements: Vec<(Vec<u16>, Vec<u16>)> =
        vec![(utf16("%FE%FF"), replacement_char.clone()), (utf16("%FF%FE"), replacement_char)];
    for run in percent_runs(&input) {
        match decode_uri_component_utf16(&run) {
            Some(decoded) => set(run, decoded, &mut replacements),
            None => {
                let result = decode_fallback(run.clone(), budget);
                if result != run {
                    set(run, result, &mut replacements);
                }
            }
        }
    }
    set(utf16("%C2"), utf16("\u{FFFD}"), &mut replacements);
    for (key, value) in &replacements {
        input = replace_all_utf16(&input, key, value);
    }
    input
}

fn set(key: Vec<u16>, value: Vec<u16>, map: &mut Vec<(Vec<u16>, Vec<u16>)>) {
    match map.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => map.push((key, value)),
    }
}

fn is_percent_token(input: &[u16], i: usize) -> bool {
    let hex = |k: usize| input.get(k).is_some_and(|&u| u < 128 && (u as u8).is_ascii_hexdigit());
    input.get(i) == Some(&u16::from(b'%')) && hex(i + 1) && hex(i + 2)
}

fn percent_runs(input: &[u16]) -> Vec<Vec<u16>> {
    let mut runs = Vec::new();
    let mut i = 0;
    while i < input.len() {
        let start = i;
        while is_percent_token(input, i) {
            i += 3;
        }
        if i > start {
            runs.push(input[start..i].to_vec());
        } else {
            i += 1;
        }
    }
    runs
}

/// `decode(input)` de `decode-uri-component` : le morcelage de la chaîne en jetons (`%XX` ou une unité, un `%` isolé
/// disparaît) puis, pour chaque point de coupe `i`, le décodage séparé des jetons avant et après `i`.
///
/// `decodeComponents` décode d'un seul tenant le plus long suffixe de chaque moitié que `decodeURIComponent` accepte
/// et laisse les jetons qui le précèdent un à un (décodés s'ils sont ASCII, sinon tels quels). Le suffixe d'une moitié
/// est calculé sans rien décoder, sur les caractères que forment les jetons ; un point de coupe qui ne change rien est
/// passé en temps constant, seuls ceux qui réécrivent la chaîne la reparcourent.
fn decode_fallback(mut input: Vec<u16>, budget: &mut usize) -> Vec<u16> {
    if let Some(decoded) = decode_uri_component_utf16(&input) {
        return decoded;
    }
    let mut tokens = Tokens::new(&input);
    let mut split = 1;
    while split < tokens.len() {
        if tokens.rewrites(split) {
            match budget.checked_sub(tokens.len()) {
                Some(left) => *budget = left,
                None => break,
            }
            input = tokens.rewrite(&input, split);
            tokens = Tokens::new(&input);
        }
        split += 1;
    }
    input
}

const NONE: usize = usize::MAX;

struct Span {
    start: usize,
    len: usize,
    byte: Option<u8>,
}

/// Jetons d'une chaîne avec, pour chaque indice, ce qui permet de décoder sans parcourir : fin du caractère qui
/// commence au jeton (`next`), premier jeton dont le suffixe entier se décode (`first_good`), plus petit jeton d'où
/// l'on atteint exactement un indice en suivant les caractères (`origin`), et les comptes cumulés de jetons `%XX`.
struct Tokens {
    spans: Vec<Span>,
    first_good: Vec<usize>,
    origin: Vec<usize>,
    percents: Vec<usize>,
    ascii_percents: Vec<usize>,
    dropped: bool,
}

impl Tokens {
    fn new(input: &[u16]) -> Self {
        let mut spans = Vec::new();
        let mut i = 0;
        while i < input.len() {
            if is_percent_token(input, i) {
                let byte = ((digit(input[i + 1]) << 4) | digit(input[i + 2])) as u8;
                spans.push(Span { start: i, len: 3, byte: Some(byte) });
                i += 3;
            } else {
                if input[i] != u16::from(b'%') {
                    spans.push(Span { start: i, len: 1, byte: None });
                }
                i += 1;
            }
        }
        let count = spans.len();
        let covered: usize = spans.iter().map(|s| s.len).sum();
        let next: Vec<usize> = (0..count).map(|at| char_end(&spans, at)).collect();
        let mut good = vec![false; count + 1];
        let mut first_good = vec![count; count + 1];
        good[count] = true;
        for at in (0..count).rev() {
            good[at] = next[at] != NONE && good[next[at]];
            first_good[at] = if good[at] { at } else { first_good[at + 1] };
        }
        let mut origin: Vec<usize> = (0..=count).collect();
        for at in 0..count {
            if next[at] != NONE {
                origin[next[at]] = origin[next[at]].min(origin[at]);
            }
        }
        let (mut percents, mut ascii_percents) = (vec![0; count + 1], vec![0; count + 1]);
        for (at, span) in spans.iter().enumerate() {
            percents[at + 1] = percents[at] + usize::from(span.byte.is_some());
            ascii_percents[at + 1] = ascii_percents[at] + usize::from(span.byte.is_some_and(|b| b < 0x80));
        }
        Self { spans, first_good, origin, percents, ascii_percents, dropped: covered != input.len() }
    }

    fn len(&self) -> usize {
        self.spans.len()
    }

    /// Le point de coupe `split` change-t-il la chaîne ? Un `%` isolé, absent des jetons, disparaît à la première
    /// réécriture.
    fn rewrites(&self, split: usize) -> bool {
        if self.dropped {
            return true;
        }
        let end = self.len();
        if self.first_good[0] == 0 {
            return self.percents[end] > 0;
        }
        let (left, right) = (self.origin[split], self.first_good[split]);
        self.ascii_percents[left] > 0
            || self.percents[split] > self.percents[left]
            || self.ascii_percents[right] > self.ascii_percents[split]
            || self.percents[end] > self.percents[right]
    }

    /// `decodeComponents(tokens, split).join('')`.
    fn rewrite(&self, input: &[u16], split: usize) -> Vec<u16> {
        let end = self.len();
        let mut out = Vec::with_capacity(input.len());
        if self.first_good[0] == 0 {
            self.decoded(input, 0..end, &mut out);
            return out;
        }
        let (left, right) = (self.origin[split], self.first_good[split]);
        self.alone(input, 0..left, &mut out);
        self.decoded(input, left..split, &mut out);
        self.alone(input, split..right, &mut out);
        self.decoded(input, right..end, &mut out);
        out
    }

    fn raw<'a>(&self, input: &'a [u16], at: usize) -> &'a [u16] {
        let span = &self.spans[at];
        &input[span.start..span.start + span.len]
    }

    /// Jetons pris un à un : décodés quand c'est un octet ASCII, sinon tels quels.
    fn alone(&self, input: &[u16], range: std::ops::Range<usize>, out: &mut Vec<u16>) {
        for at in range {
            match self.spans[at].byte {
                Some(byte) if byte < 0x80 => out.push(u16::from(byte)),
                _ => out.extend_from_slice(self.raw(input, at)),
            }
        }
    }

    /// Jetons décodés d'un seul tenant, comme `decodeURIComponent` sur leur concaténation.
    fn decoded(&self, input: &[u16], range: std::ops::Range<usize>, out: &mut Vec<u16>) {
        let joined: Vec<u16> = range.flat_map(|at| self.raw(input, at).iter().copied()).collect();
        match decode_uri_component_utf16(&joined) {
            Some(decoded) => out.extend(decoded),
            None => out.extend(joined),
        }
    }
}

fn digit(unit: u16) -> u16 {
    char::from_u32(u32::from(unit)).and_then(|c| c.to_digit(16)).map_or(0, |d| d as u16)
}

/// Indice qui suit le caractère commençant au jeton `at`, ou `NONE` quand `decodeURIComponent` le refuserait
/// (octet isolé, séquence tronquée ou invalide).
fn char_end(spans: &[Span], at: usize) -> usize {
    let Some(first) = spans[at].byte.filter(|byte| *byte >= 0x80) else { return at + 1 };
    let count = match first.leading_ones() {
        n @ 2..=4 => n as usize,
        _ => return NONE,
    };
    let bytes: Option<Vec<u8>> = spans.get(at..at + count).and_then(|span| span.iter().map(|s| s.byte).collect());
    let valid = bytes.is_some_and(|b| b[1..].iter().all(|c| c & 0xC0 == 0x80) && std::str::from_utf8(&b).is_ok());
    if valid {
        at + count
    } else {
        NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_tokens(input: &[u16]) -> Vec<Vec<u16>> {
        let mut tokens = Vec::new();
        let mut i = 0;
        while i < input.len() {
            if is_percent_token(input, i) {
                tokens.push(input[i..i + 3].to_vec());
                i += 3;
            } else {
                if input[i] != u16::from(b'%') {
                    tokens.push(vec![input[i]]);
                }
                i += 1;
            }
        }
        tokens
    }

    fn decode_components(components: &[Vec<u16>], split: usize) -> Vec<Vec<u16>> {
        if let Some(decoded) = decode_uri_component_utf16(&components.concat()) {
            return vec![decoded];
        }
        if components.len() == 1 {
            return components.to_vec();
        }
        let (left, right) = components.split_at(split.min(components.len()));
        let mut out = decode_components(left, 1);
        out.extend(decode_components(right, 1));
        out
    }

    /// Port littéral de `decode` de `decode-uri-component` : cubique, il sert de référence.
    fn reference_fallback(mut input: Vec<u16>) -> Vec<u16> {
        if let Some(decoded) = decode_uri_component_utf16(&input) {
            return decoded;
        }
        let mut tokens = single_tokens(&input);
        let mut i = 1;
        while i < tokens.len() {
            input = decode_components(&tokens, i).concat();
            tokens = single_tokens(&input);
            i += 1;
        }
        input
    }

    fn unlimited_fallback(input: &[u16]) -> Vec<u16> {
        let mut budget = usize::MAX;
        decode_fallback(input.to_vec(), &mut budget)
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self, bound: usize) -> usize {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 % bound as u64) as usize
        }
    }

    const BYTES: [u8; 26] = [
        0x00, 0x25, 0x32, 0x35, 0x41, 0x7F, 0x80, 0x81, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xC2, 0xDF, 0xE0, 0xE1,
        0xE3, 0xED, 0xEF, 0xF0, 0xF4, 0xF5, 0xFE, 0xFF,
    ];
    const LITERALS: [&str; 8] = ["a", "4", "1", "%", "5", "2", "é", "%4"];

    fn random_run(rng: &mut Rng) -> String {
        let mut text = String::new();
        for _ in 0..1 + rng.next(32) {
            if rng.next(6) == 0 {
                text.push_str(LITERALS[rng.next(LITERALS.len())]);
            } else {
                text.push_str(&format!("%{:02X}", BYTES[rng.next(BYTES.len())]));
            }
        }
        text
    }

    #[test]
    fn ef_imp_01_decode_fallback_gives_what_decode_uri_component_gives() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for _ in 0..1_500 {
            let input = utf16(&random_run(&mut rng));
            let expected = reference_fallback(input.clone());
            assert_eq!(unlimited_fallback(&input), expected, "{}", String::from_utf16_lossy(&input));
        }
    }

    fn timed<T>(f: impl FnOnce() -> T) -> (T, std::time::Duration) {
        let start = std::time::Instant::now();
        (f(), start.elapsed())
    }

    fn decoded(text: &str) -> (String, std::time::Duration) {
        let mut budget = FALLBACK_BUDGET;
        timed(|| decode_component(text, &mut budget))
    }

    #[test]
    fn ef_imp_01_decode_fallback_is_fast_on_long_invalid_runs() {
        let truncated = "%E3%81".repeat(3_000);
        let (text, elapsed) = decoded(&truncated);
        assert_eq!(text, truncated);
        assert!(elapsed.as_millis() < 500, "{elapsed:?}");

        let (text, elapsed) = decoded(&"%83%65%83%58%83%67".repeat(900));
        assert_eq!(text, "%83e%83X%83g".repeat(900));
        assert!(elapsed.as_millis() < 500, "{elapsed:?}");
    }

    #[test]
    fn ef_imp_01_decode_fallback_stops_when_its_budget_is_spent() {
        let stray = utf16(&"%FF%C3%A9".repeat(40));
        let complete = reference_fallback(stray.clone());
        assert_eq!(unlimited_fallback(&stray), complete);
        let mut budget = 300;
        assert_ne!(decode_fallback(stray, &mut budget), complete);
        assert!(budget < 300);
    }

    #[test]
    fn ef_imp_01_repeated_query_keys_are_collected_in_linear_time() {
        let query = "a=b&".repeat(100_000);
        let (parsed, elapsed) = timed(|| query_string_parse(&query));
        assert!(matches!(parsed.get("a"), Some(QsValue::List(items)) if items.len() == 100_000));
        assert!(elapsed.as_millis() < 500, "{elapsed:?}");
    }
}
