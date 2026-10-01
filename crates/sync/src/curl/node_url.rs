//! Port de l'API historique `url.parse` / `url.format` de Node.js 22 (sans `parseQueryString` ni
//! `slashesDenoteHost`), utilisée par `parse-curl.js`.

use super::domain::to_ascii;
use crate::js::{decode_uri_component, is_js_space, utf16_len, JsError};

const SLASHED_PROTOCOLS: [&str; 14] = [
    "http", "http:", "https", "https:", "ftp", "ftp:", "gopher", "gopher:", "file", "file:", "ws", "ws:", "wss", "wss:",
];
const JAVASCRIPT_PROTOCOLS: [&str; 2] = ["javascript", "javascript:"];

#[derive(Debug, Default)]
pub(crate) struct Url {
    pub protocol: Option<String>,
    pub slashes: bool,
    pub auth: Option<String>,
    pub host: Option<String>,
    pub port: Option<String>,
    pub hostname: Option<String>,
    pub hash: Option<String>,
    pub search: Option<String>,
    pub query: Option<String>,
    pub pathname: Option<String>,
}

fn text(chars: &[char]) -> String {
    chars.iter().collect()
}

fn invalid_url(url: &str) -> JsError {
    JsError(format!("TypeError: Invalid URL: {url}"))
}

fn is_slashed(protocol: Option<&str>) -> bool {
    protocol.is_some_and(|p| SLASHED_PROTOCOLS.contains(&p))
}

fn is_javascript(protocol: Option<&str>) -> bool {
    protocol.is_some_and(|p| JAVASCRIPT_PROTOCOLS.contains(&p))
}

/// `url.parse(url)`.
pub(crate) fn parse(url: &str) -> Result<Url, JsError> {
    let chars: Vec<char> = url.chars().collect();
    let mut result = Url::default();
    let mut rest = trim_and_convert_backslashes(&chars);
    let (has_hash, has_at) = split_flags(&chars);

    if !has_hash && !has_at {
        if let Some((pathname, search)) = simple_path(&rest) {
            result.pathname = Some(pathname);
            if let Some(search) = search {
                result.query = Some(search[1..].to_owned());
                result.search = Some(search);
            }
            return Ok(result);
        }
    }

    let proto_len = protocol_len(&rest);
    let proto: Option<String> = proto_len.map(|len| text(&rest[..len]));
    let lower_proto = proto.as_ref().map(|p| p.to_lowercase());
    if let Some(len) = proto_len {
        result.protocol = lower_proto.clone();
        rest.drain(..len);
    }
    let lower = lower_proto.as_deref();

    let mut slashes = false;
    if proto.is_some() || host_pattern(&rest) {
        slashes = rest.starts_with(&['/', '/']);
        if slashes && !(proto.is_some() && is_javascript(lower)) {
            rest.drain(..2);
            result.slashes = true;
        }
    }

    if !is_javascript(lower) && (slashes || proto.as_deref().is_some_and(|p| !SLASHED_PROTOCOLS.contains(&p))) {
        rest = parse_host(&mut result, rest, url)?;
    }

    if !is_javascript(lower) {
        rest = auto_escape(&rest);
    }

    let hash_index = rest.iter().position(|&c| c == '#');
    let searchable = &rest[..hash_index.unwrap_or(rest.len())];
    let question_index = searchable.iter().position(|&c| c == '?');
    if let Some(h) = hash_index {
        result.hash = Some(text(&rest[h..]));
    }
    if let Some(q) = question_index {
        result.search = Some(text(&searchable[q..]));
        result.query = Some(text(&searchable[q + 1..]));
    }
    match question_index.or(hash_index) {
        None if !rest.is_empty() => result.pathname = Some(text(&rest)),
        Some(first) if first > 0 => result.pathname = Some(text(&rest[..first])),
        _ => {}
    }
    if is_slashed(lower) && result.hostname.as_deref().is_some_and(|h| !h.is_empty()) && result.pathname.is_none() {
        result.pathname = Some("/".to_owned());
    }
    Ok(result)
}

fn is_url_whitespace(c: char) -> bool {
    (c as u32) < 33 || c == '\u{a0}' || c == '\u{feff}'
}

fn split_flags(chars: &[char]) -> (bool, bool) {
    let mut has_hash = false;
    let mut has_at = false;
    let mut split = false;
    let mut started = false;
    for &c in chars {
        if !started {
            if is_url_whitespace(c) {
                continue;
            }
            started = true;
        }
        if !split {
            match c {
                '@' => has_at = true,
                '#' => {
                    has_hash = true;
                    split = true;
                }
                '?' => split = true,
                _ => {}
            }
        } else if !has_hash && c == '#' {
            has_hash = true;
        }
    }
    (has_hash, has_at)
}

fn trim_and_convert_backslashes(url: &[char]) -> Vec<char> {
    let mut rest: Vec<char> = Vec::new();
    let mut start: Option<usize> = None;
    let mut end: Option<usize> = None;
    let mut last_pos = 0;
    let mut in_ws = false;
    let mut split = false;
    for (i, &c) in url.iter().enumerate() {
        let is_ws = is_url_whitespace(c);
        if start.is_none() {
            if is_ws {
                continue;
            }
            start = Some(i);
            last_pos = i;
        } else if in_ws {
            if !is_ws {
                end = None;
                in_ws = false;
            }
        } else if is_ws {
            end = Some(i);
            in_ws = true;
        }
        if !split {
            match c {
                '#' | '?' => split = true,
                '\\' => {
                    rest.extend_from_slice(&url[last_pos..i]);
                    rest.push('/');
                    last_pos = i + 1;
                }
                _ => {}
            }
        }
    }
    let Some(start) = start else { return rest };
    if last_pos == start {
        return url[start..end.unwrap_or(url.len())].to_vec();
    }
    match end {
        None if last_pos < url.len() => rest.extend_from_slice(&url[last_pos..]),
        Some(end) if last_pos < end => rest.extend_from_slice(&url[last_pos..end]),
        _ => {}
    }
    rest
}

fn simple_path(rest: &[char]) -> Option<(String, Option<String>)> {
    if rest.first() != Some(&'/') || (rest.get(1) == Some(&'/') && rest.get(2) == Some(&'/')) {
        return None;
    }
    let stop = rest.iter().position(|&c| c == '?' || is_js_space(c));
    match stop {
        None => Some((text(rest), None)),
        Some(k) if rest[k] == '?' && !rest[k..].iter().any(|&c| is_js_space(c)) => {
            Some((text(&rest[..k]), Some(text(&rest[k..]))))
        }
        _ => None,
    }
}

fn protocol_len(rest: &[char]) -> Option<usize> {
    let len = rest.iter().take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-')).count();
    (len > 0 && rest.get(len) == Some(&':')).then_some(len + 1)
}

fn host_pattern(rest: &[char]) -> bool {
    if !rest.starts_with(&['/', '/']) {
        return false;
    }
    let user = rest[2..].iter().take_while(|&&c| c != '@' && c != '/').count();
    user > 0 && rest.get(2 + user) == Some(&'@') && rest.get(3 + user).is_some_and(|&c| c != '@' && c != '/')
}

fn parse_host(result: &mut Url, mut rest: Vec<char>, url: &str) -> Result<Vec<char>, JsError> {
    let mut host_end = None;
    let mut at_sign = None;
    let mut non_host = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            '\t' | '\n' | '\r' => {
                rest.remove(i);
                continue;
            }
            ' ' | '"' | '%' | '\'' | ';' | '<' | '>' | '\\' | '^' | '`' | '{' | '|' | '}' => {
                non_host.get_or_insert(i);
            }
            '#' | '/' | '?' => {
                non_host.get_or_insert(i);
                host_end = Some(i);
            }
            '@' => {
                at_sign = Some(i);
                non_host = None;
            }
            _ => {}
        }
        if host_end.is_some() {
            break;
        }
        i += 1;
    }
    let mut start = 0;
    if let Some(at) = at_sign {
        let auth = text(&rest[..at]);
        result.auth = Some(decode_uri_component(&auth).ok_or_else(|| JsError("URIError: URI malformed".into()))?);
        start = at + 1;
    }
    let host = match non_host {
        None => {
            let host = text(&rest[start..]);
            rest.clear();
            host
        }
        Some(n) => {
            let host = text(&rest[start..n]);
            rest.drain(..n);
            host
        }
    };
    let (hostname, port) = split_port(&host);
    result.port = port;
    let mut hostname = hostname.unwrap_or_default();
    let ipv6 = hostname.starts_with('[') && hostname.ends_with(']') && !hostname.is_empty();
    if !ipv6 {
        if let Some(k) = hostname.find(['/', '\\', '#', '?', ':']) {
            let mut moved: Vec<char> = vec!['/'];
            moved.extend(hostname[k..].chars());
            moved.extend(rest);
            rest = moved;
            hostname.truncate(k);
        }
    }
    hostname = if utf16_len(&hostname) > 255 { String::new() } else { hostname.to_lowercase() };
    if !hostname.is_empty() {
        if ipv6 {
            if hostname.contains(['\0', '\t', '\n', '\r', ' ', '#', '%', '/', '<', '>', '?', '@', '\\', '^', '|']) {
                return Err(invalid_url(url));
            }
        } else {
            hostname = to_ascii(&hostname);
            let forbidden =
                ['\0', '\t', '\n', '\r', ' ', '#', '%', '/', ':', '<', '>', '?', '@', '[', '\\', ']', '^', '|'];
            if hostname.is_empty() || hostname.contains(forbidden) {
                return Err(invalid_url(url));
            }
        }
    }
    let port_suffix = result.port.as_ref().map(|p| format!(":{p}")).unwrap_or_default();
    result.host = Some(format!("{hostname}{port_suffix}"));
    if ipv6 {
        hostname = if hostname.is_empty() { hostname } else { hostname[1..hostname.len() - 1].to_owned() };
        if rest.first() != Some(&'/') {
            rest.insert(0, '/');
        }
    }
    result.hostname = Some(hostname);
    Ok(rest)
}

fn split_port(host: &str) -> (Option<String>, Option<String>) {
    let mut host = host.to_owned();
    let mut port = None;
    if let Some(colon) = host.rfind(':') {
        if host[colon + 1..].bytes().all(|b| b.is_ascii_digit()) {
            if colon + 1 < host.len() {
                port = Some(host[colon + 1..].to_owned());
            }
            host.truncate(colon);
        }
    }
    ((!host.is_empty()).then_some(host), port)
}

fn auto_escape(rest: &[char]) -> Vec<char> {
    let mut out = Vec::with_capacity(rest.len());
    for &c in rest {
        let escaped = match c {
            '\t' => "%09",
            '\n' => "%0A",
            '\r' => "%0D",
            ' ' => "%20",
            '"' => "%22",
            '\'' => "%27",
            '<' => "%3C",
            '>' => "%3E",
            '\\' => "%5C",
            '^' => "%5E",
            '`' => "%60",
            '{' => "%7B",
            '|' => "%7C",
            '}' => "%7D",
            _ => {
                out.push(c);
                continue;
            }
        };
        out.extend(escaped.chars());
    }
    out
}

/// `url.format(urlObject)` pour un objet issu de [`parse`].
pub(crate) fn format(url: &Url) -> String {
    let auth = match url.auth.as_deref() {
        Some(auth) if !auth.is_empty() => format!("{}@", encode_auth(auth)),
        _ => String::new(),
    };
    let mut protocol = url.protocol.clone().unwrap_or_default();
    if !protocol.is_empty() && !protocol.ends_with(':') {
        protocol.push(':');
    }
    let mut pathname = url.pathname.clone().unwrap_or_default();
    let mut hash = url.hash.clone().unwrap_or_default();
    let mut host = String::new();
    if let Some(h) = url.host.as_deref().filter(|h| !h.is_empty()) {
        host = format!("{auth}{h}");
    } else if let Some(hostname) = url.hostname.as_deref().filter(|h| !h.is_empty()) {
        let bracketed = hostname.contains(':') && !(hostname.starts_with('[') && hostname.ends_with(']'));
        host = if bracketed { format!("{auth}[{hostname}]") } else { format!("{auth}{hostname}") };
        if let Some(port) = url.port.as_deref().filter(|p| !p.is_empty()) {
            host = format!("{host}:{port}");
        }
    }
    let mut search = url.search.clone().unwrap_or_default();
    pathname = pathname.replace('#', "%23").replace('?', "%3F");
    if url.slashes || is_slashed(Some(&protocol)) {
        if url.slashes || !host.is_empty() {
            if !pathname.is_empty() && !pathname.starts_with('/') {
                pathname.insert(0, '/');
            }
            host = format!("//{host}");
        } else if protocol.len() >= 4 && protocol.starts_with("file") {
            host = "//".to_owned();
        }
    }
    search = search.replace('#', "%23");
    if !hash.is_empty() && !hash.starts_with('#') {
        hash.insert(0, '#');
    }
    if !search.is_empty() && !search.starts_with('?') {
        search.insert(0, '?');
    }
    format!("{protocol}{host}{pathname}{search}{hash}")
}

fn encode_auth(auth: &str) -> String {
    let mut out = String::with_capacity(auth.len());
    for c in auth.chars() {
        if c.is_ascii_alphanumeric() || "!'()*-._~:".contains(c) {
            out.push(c);
        } else {
            let mut buf = [0u8; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                out.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    out
}
