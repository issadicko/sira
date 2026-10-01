//! Port de `parse-curl.js` : nettoyage de la commande, découpage `shell-quote` puis machine à états sur les
//! options de cURL.

use serde_json::{json, Map, Value};

use super::node_url;
use super::query::{parse_query_params, QueryParam};
use super::shell::{self, Token};
use crate::js::{decode_uri_component, is_js_space, is_line_terminator, push_char, trim, JsError, JsObject};

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    UserAgent,
    Header,
    Data,
    Json,
    User,
    Method,
    Cookie,
    Form,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AuthMode {
    Basic,
    Digest,
    Ntlm,
}

impl AuthMode {
    fn name(self) -> &'static str {
        match self {
            AuthMode::Basic => "basic",
            AuthMode::Digest => "digest",
            AuthMode::Ntlm => "ntlm",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Auth {
    pub mode: AuthMode,
    pub username: String,
    pub password: String,
}

impl Auth {
    pub(crate) fn to_json(&self) -> Value {
        let mode = self.mode.name();
        let mut map = Map::new();
        map.insert("mode".into(), json!(mode));
        map.insert(mode.into(), json!({ "username": self.username, "password": self.password }));
        Value::Object(map)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FormField {
    pub name: String,
    pub value: String,
    pub is_file: bool,
}

impl FormField {
    pub(crate) fn to_json(&self) -> Value {
        let kind = if self.is_file { "file" } else { "text" };
        json!({ "name": self.name, "value": self.value, "type": kind, "enabled": true })
    }
}

/// Requête produite par `parseCurlCommand` ; une valeur `None` d'en-tête vaut `undefined`.
#[derive(Clone, Debug, Default)]
pub struct ParsedCurl {
    pub(crate) url: Option<String>,
    pub(crate) url_without_query: Option<String>,
    pub(crate) queries: Vec<QueryParam>,
    pub(crate) method: Option<String>,
    pub(crate) headers: JsObject<Option<Token>>,
    pub(crate) data: Option<Token>,
    pub(crate) is_data_raw: bool,
    pub(crate) is_data_binary: bool,
    pub(crate) is_query: bool,
    pub(crate) insecure: bool,
    is_digest_auth: bool,
    is_ntlm_auth: bool,
    auth_credentials: Option<(String, String)>,
    pub(crate) cookies: Option<JsObject<String>>,
    pub(crate) cookie_string: Option<String>,
    pub(crate) multipart_uploads: Option<Vec<FormField>>,
    pub(crate) auth: Option<Auth>,
}

impl ParsedCurl {
    /// Vue JSON (`JSON.stringify`) de l'objet renvoyé par `parseCurlCommand`.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        let mut put = |key: &str, value: Value| {
            map.insert(key.to_owned(), value);
        };
        if let Some(method) = &self.method {
            put("method", json!(method));
        }
        if !self.headers.is_empty() {
            put("headers", self.headers.to_json(|v| v.as_ref().map(Token::to_json)));
        }
        if let Some(data) = &self.data {
            put("data", data.to_json());
        }
        for (key, flag) in [("isDataRaw", self.is_data_raw), ("isDataBinary", self.is_data_binary)] {
            if flag {
                put(key, json!(true));
            }
        }
        if self.is_query {
            put("isQuery", json!(true));
        }
        if self.insecure {
            put("insecure", json!(true));
        }
        if let Some(cookies) = &self.cookies {
            put("cookies", cookies.to_json(|v| Some(json!(v))));
        }
        if let Some(cookie_string) = &self.cookie_string {
            put("cookieString", json!(cookie_string));
        }
        if let Some(uploads) = &self.multipart_uploads {
            put("multipartUploads", uploads.iter().map(FormField::to_json).collect());
        }
        if let Some(auth) = &self.auth {
            put("auth", auth.to_json());
        }
        if !self.queries.is_empty() {
            put("queries", self.queries.iter().map(QueryParam::to_json).collect());
        }
        if let Some(url) = &self.url {
            put("url", json!(url));
        }
        if let Some(url) = &self.url_without_query {
            put("urlWithoutQuery", json!(url));
        }
        Value::Object(map)
    }
}

/// `parseCurlCommand(curl)`.
pub fn parse_curl_command(curl: &str) -> Result<ParsedCurl, JsError> {
    let cleaned = clean_curl_command(curl);
    let args = shell::parse(&cleaned)?;
    let mut request = build_request(&args)?;
    post_build_process_request(&mut request)?;
    Ok(request)
}

fn build_request(args: &[Token]) -> Result<ParsedCurl, JsError> {
    let mut request = ParsedCurl::default();
    let mut state: Option<State> = None;
    for arg in args {
        let new_state = process_argument(arg, state, &mut request)?;
        if state.is_some() && new_state.is_none() {
            state = None;
        } else if new_state.is_some() {
            state = new_state;
        }
    }
    Ok(request)
}

fn process_argument(arg: &Token, state: Option<State>, request: &mut ParsedCurl) -> Result<Option<State>, JsError> {
    if let Some(flag_state) = arg.as_word().and_then(|flag| handle_flag(flag, request)) {
        return Ok(Some(flag_state));
    }
    let truthy = arg.as_word() != Some("");
    if let (true, Some(state)) = (truthy, state) {
        handle_value(arg, state, request)?;
        return Ok(None);
    }
    if state.is_none() && is_url_or_fragment(arg)? {
        set_url(request, arg)?;
    }
    Ok(None)
}

fn handle_flag(arg: &str, request: &mut ParsedCurl) -> Option<State> {
    let state = match arg {
        "-A" | "--user-agent" => State::UserAgent,
        "-H" | "--header" => State::Header,
        "-d" | "--data" | "--data-ascii" | "--data-urlencode" => State::Data,
        "--json" => State::Json,
        "-u" | "--user" => State::User,
        "-X" | "--request" => State::Method,
        "-b" | "--cookie" => State::Cookie,
        "-F" | "--form" => State::Form,
        "--data-raw" => {
            request.is_data_raw = true;
            State::Data
        }
        "--data-binary" => {
            request.is_data_binary = true;
            State::Data
        }
        "-I" | "--head" => {
            request.method = Some("HEAD".into());
            return None;
        }
        "--compressed" => {
            if request.headers.get("Accept-Encoding").is_none_or(|v| !token_truthy(v.as_ref())) {
                request.headers.set("Accept-Encoding", Some(Token::Word("deflate, gzip".into())));
            }
            return None;
        }
        "-k" | "--insecure" => {
            request.insecure = true;
            return None;
        }
        "--digest" => {
            request.is_digest_auth = true;
            return None;
        }
        "--ntlm" => {
            request.is_ntlm_auth = true;
            return None;
        }
        "-G" | "--get" => {
            request.is_query = true;
            return None;
        }
        _ => return None,
    };
    Some(state)
}

fn token_truthy(token: Option<&Token>) -> bool {
    token.is_some_and(|t| t.as_word() != Some(""))
}

fn not_a_function(what: &str) -> JsError {
    JsError::type_error(&format!("{what} is not a function"))
}

fn handle_value(value: &Token, state: State, request: &mut ParsedCurl) -> Result<(), JsError> {
    match state {
        State::Header => {
            let value = value.as_word().ok_or_else(|| not_a_function("value.split"))?;
            let (name, header_value) = split_header(value);
            request.headers.set(&name, header_value.map(Token::Word));
        }
        State::UserAgent => request.headers.set("User-Agent", Some(value.clone())),
        State::Data => {
            request.data = Some(match &request.data {
                Some(data) => Token::Word(format!("{}&{}", data.js_string(), value.js_string())),
                None => value.clone(),
            });
        }
        State::Json => {
            if matches!(request.method.as_deref(), Some("GET" | "HEAD")) {
                request.method = Some("POST".into());
            }
            request.headers.set("Content-Type", Some(Token::Word("application/json".into())));
            request.data = Some(value.clone());
        }
        State::Form => {
            let field = value.as_word().ok_or_else(|| not_a_function("field.match"))?;
            let uploads = request.multipart_uploads.get_or_insert_with(Vec::new);
            uploads.extend(parse_form_field(field));
            request.method = Some("POST".into());
        }
        State::User => {
            if let Some(value) = value.as_word() {
                let mut parts = value.split(':');
                let username = parts.next().unwrap_or_default().to_owned();
                let password = parts.next().unwrap_or_default().to_owned();
                request.auth_credentials = Some((username, password));
            }
        }
        State::Method => {
            let method = value.as_word().ok_or_else(|| not_a_function("value.toUpperCase"))?;
            request.method = Some(method.to_uppercase());
        }
        State::Cookie => {
            if let Some(value) = value.as_word() {
                set_cookie(request, value)?;
            }
        }
    }
    Ok(())
}

/// `value.split(/:\s*(.+)/)` réduit à ses deux premiers éléments.
pub(crate) fn split_header(value: &str) -> (String, Option<String>) {
    let chars: Vec<char> = value.chars().collect();
    for (colon, _) in chars.iter().enumerate().filter(|(_, &c)| c == ':') {
        let spaces_end = colon + 1 + chars[colon + 1..].iter().take_while(|&&c| is_js_space(c)).count();
        let start = (colon + 1..=spaces_end).rev().find(|&k| chars.get(k).is_some_and(|&c| !is_line_terminator(c)));
        if let Some(start) = start {
            let end = start + chars[start..].iter().take_while(|&&c| !is_line_terminator(c)).count();
            return (chars[..colon].iter().collect(), Some(chars[start..end].iter().collect()));
        }
    }
    (value.to_owned(), None)
}

fn set_cookie(request: &mut ParsedCurl, value: &str) -> Result<(), JsError> {
    let parsed = parse_cookie(value)?;
    let mut cookies = request.cookies.take().unwrap_or_default();
    for (key, cookie) in parsed.iter() {
        cookies.set(key, cookie.clone());
    }
    request.cookies = Some(cookies);
    let cookie_string = match request.cookie_string.take() {
        Some(previous) if !previous.is_empty() => format!("{previous}; {value}"),
        _ => value.to_owned(),
    };
    request.headers.set("Cookie", Some(Token::Word(cookie_string.clone())));
    request.cookie_string = Some(cookie_string);
    Ok(())
}

/// `cookie.parse` de la bibliothèque `cookie` 0.7.
pub(crate) fn parse_cookie(text: &str) -> Result<JsObject<String>, JsError> {
    let s: Vec<char> = text.chars().collect();
    let mut cookies = JsObject::new();
    let len = s.len();
    if len < 2 {
        return Ok(cookies);
    }
    let find = |c: char, from: usize| s[from.min(len)..].iter().position(|&x| x == c).map(|i| i + from);
    let mut index = 0;
    while index < len {
        let Some(eq) = find('=', index) else { break };
        let end = match find(';', index) {
            None => len,
            Some(end) if eq > end => {
                index = s[..eq].iter().rposition(|&c| c == ';').map_or(0, |i| i + 1);
                continue;
            }
            Some(end) => end,
        };
        let key_start = skip_spaces_forward(&s, index, eq);
        let key_end = skip_spaces_backward(&s, eq, key_start);
        let key: String = s[key_start..key_end].iter().collect();
        if cookies.contains("hasOwnProperty") {
            return Err(not_a_function("obj.hasOwnProperty"));
        }
        if !cookies.contains(&key) {
            let mut value_start = skip_spaces_forward(&s, eq + 1, end);
            let mut value_end = skip_spaces_backward(&s, end, value_start);
            if s.get(value_start) == Some(&'"') && value_end > 0 && s.get(value_end - 1) == Some(&'"') {
                value_start += 1;
                value_end -= 1;
            }
            let raw: String = s[value_start.min(value_end)..value_end].iter().collect();
            let value = if raw.contains('%') { decode_uri_component(&raw).unwrap_or(raw) } else { raw };
            cookies.set(&key, value);
        }
        index = end + 1;
    }
    Ok(cookies)
}

fn is_cookie_space(c: char) -> bool {
    c == ' ' || c == '\t'
}

fn skip_spaces_forward(s: &[char], mut index: usize, max: usize) -> usize {
    loop {
        if s.get(index).is_none_or(|&c| !is_cookie_space(c)) {
            return index;
        }
        index += 1;
        if index >= max {
            return max;
        }
    }
}

fn skip_spaces_backward(s: &[char], mut index: usize, min: usize) -> usize {
    while index > min {
        index -= 1;
        if !is_cookie_space(s[index]) {
            return index + 1;
        }
    }
    min
}

fn double_quoted(text: &str) -> Option<&str> {
    text.strip_prefix('"').and_then(|t| t.strip_suffix('"')).filter(|inner| !inner.contains('"'))
}

/// `parseFormField(field)` : `name=valeur`, `name=@fichier` ou `name="valeur"`.
pub(crate) fn parse_form_field(field: &str) -> Option<FormField> {
    let (name, rest) = field.split_once('=')?;
    if name.is_empty() {
        return None;
    }
    let value = if let Some(inner) = rest.strip_prefix('@').and_then(double_quoted).or_else(|| double_quoted(rest)) {
        inner
    } else if let Some(path) = rest.strip_prefix('@').filter(|path| !path.contains('@')) {
        path
    } else if !rest.contains('@') {
        rest
    } else {
        return None;
    };
    Some(FormField { name: name.to_owned(), value: value.to_owned(), is_file: field.contains('@') })
}

fn is_url_or_fragment(arg: &Token) -> Result<bool, JsError> {
    if let Token::Word(s) = arg {
        if is_url(s)? {
            return Ok(true);
        }
    }
    match arg {
        Token::Glob(pattern) => is_url(pattern),
        Token::Op(op) => Ok(op == "&"),
        Token::Word(s) => Ok(is_query_fragment(s)),
        Token::Comment(_) => Ok(false),
    }
}

/// `/^[^=]+=[^&]*$/`.
fn is_query_fragment(s: &str) -> bool {
    s.split_once('=').is_some_and(|(name, value)| !name.is_empty() && !value.contains('&'))
}

pub(crate) fn is_url(arg: &str) -> Result<bool, JsError> {
    if node_url::parse(arg)?.host.is_some_and(|h| !h.is_empty()) {
        return Ok(true);
    }
    Ok(matches_domain_pattern(arg))
}

fn matches_domain_pattern(arg: &str) -> bool {
    let host_len = arg.find(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '-')).unwrap_or(arg.len());
    let (host, rest) = arg.split_at(host_len);
    let rest_ok = rest.is_empty() || (rest.starts_with(['/', '?']) && !rest.chars().any(is_js_space));
    let labels: Vec<&str> = host.split('.').collect();
    let label_ok = |l: &&str| {
        (1..=63).contains(&l.len())
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    rest_ok && labels.len() >= 2 && labels.iter().all(label_ok)
}

fn set_url(request: &mut ParsedCurl, url: &Token) -> Result<(), JsError> {
    let url_string = match url {
        Token::Word(s) => s.as_str(),
        Token::Glob(pattern) => pattern.as_str(),
        Token::Op(op) if op == "&" => "&",
        _ => return Ok(()),
    };
    if url_string.is_empty() {
        return Ok(());
    }
    let current = request.url.clone().filter(|u| !u.is_empty());
    let processed = if current.is_none() && !has_scheme(url_string) {
        format!("https://{url_string}")
    } else {
        url_string.to_owned()
    };
    let new_url = match current {
        Some(current) => current + &processed,
        None => processed,
    };
    let (formatted, without_query, queries) = parse_url(&new_url)?;
    request.url = Some(formatted);
    request.url_without_query = Some(without_query);
    request.queries = queries;
    Ok(())
}

/// `/^[a-zA-Z]+:\/\//`.
fn has_scheme(url: &str) -> bool {
    let letters = url.bytes().take_while(u8::is_ascii_alphabetic).count();
    letters > 0 && url[letters..].starts_with("://")
}

fn parse_url(url: &str) -> Result<(String, String, Vec<QueryParam>), JsError> {
    let parsed = node_url::parse(url)?;
    let queries = parse_query_params(parsed.query.as_deref());
    let mut formatted = node_url::format(&parsed);
    if !url.ends_with('/') && formatted.ends_with('/') {
        formatted.pop();
    }
    let without_query = formatted.split('?').next().unwrap_or_default().to_owned();
    Ok((formatted, without_query, queries))
}

fn convert_data_to_query_string(request: &mut ParsedCurl) -> Result<(), JsError> {
    let mut url = request.url.clone().ok_or_else(|| JsError::type_error("Cannot read properties of undefined"))?;
    if !url.contains('?') {
        url.push('?');
    } else if !url.ends_with('&') {
        url.push('&');
    }
    url.push_str(&request.data.as_ref().map(Token::js_string).unwrap_or_default());
    let (formatted, _, queries) = parse_url(&url)?;
    request.url = Some(formatted);
    request.queries = queries;
    Ok(())
}

fn post_build_process_request(request: &mut ParsedCurl) -> Result<(), JsError> {
    let has_data = token_truthy(request.data.as_ref());
    if request.is_query && has_data {
        convert_data_to_query_string(request)?;
        request.data = None;
        request.is_query = false;
    } else if has_data && matches!(request.method.as_deref(), None | Some("" | "HEAD")) {
        request.method = Some("POST".into());
    }
    normalize_auth_properties(request);
    let method = request.method.take().filter(|m| !m.is_empty()).unwrap_or_else(|| "GET".into());
    request.method = Some(method.to_lowercase());
    Ok(())
}

fn normalize_auth_properties(request: &mut ParsedCurl) {
    if let Some((username, password)) = request.auth_credentials.take() {
        let mode = if request.is_digest_auth {
            AuthMode::Digest
        } else if request.is_ntlm_auth {
            AuthMode::Ntlm
        } else {
            AuthMode::Basic
        };
        request.auth = Some(Auth { mode, username, password });
    }
    request.is_digest_auth = false;
    request.is_ntlm_auth = false;
}

/// `cleanCurlCommand` : décodage des `$'…'`, échappement des `\'`, séparation de `-XPOST`, `trim`.
pub(crate) fn clean_curl_command(command: &str) -> String {
    let command = replace_ansi_c_quotes(command);
    let command = escape_single_quotes(&command);
    let command = fix_concatenated_methods(command);
    trim(&command).to_owned()
}

fn replace_ansi_c_quotes(command: &str) -> String {
    let s: Vec<char> = command.chars().collect();
    let len = s.len();
    let mut ends: Vec<Option<usize>> = vec![None; len + 2];
    for q in (0..len).rev() {
        let escaped = s[q] == '\\' && q + 1 < len && !is_line_terminator(s[q + 1]);
        ends[q] = escaped
            .then(|| ends[q + 2])
            .flatten()
            .or_else(|| if s[q] != '\'' { ends[q + 1] } else { None })
            .or_else(|| (s[q] == '\'').then_some(q + 1));
    }
    let mut out = String::with_capacity(command.len());
    let mut i = 0;
    while i < len {
        if s[i] == '$' && s.get(i + 1) == Some(&'\'') {
            if let Some(end) = ends[i + 2] {
                let body: String = s[i + 2..end - 1].iter().collect();
                out.push_str(&quote_for_shell(&decode_ansi_escapes(&body)));
                i = end;
                continue;
            }
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn decode_ansi_escapes(value: &str) -> String {
    let s: Vec<char> = value.chars().collect();
    let mut out: Vec<u16> = Vec::with_capacity(s.len());
    let hex = |from: usize, count: usize| -> Option<u16> {
        let digits: String = s.get(from..from + count)?.iter().collect();
        digits.chars().all(|c| c.is_ascii_hexdigit()).then(|| u16::from_str_radix(&digits, 16).ok()).flatten()
    };
    let mut i = 0;
    while i < s.len() {
        if s[i] == '\\' {
            let decoded = match s.get(i + 1) {
                Some('\\') => Some((u16::from(b'\\'), 2)),
                Some('\'') => Some((u16::from(b'\''), 2)),
                Some('n') => Some((0x0A, 2)),
                Some('r') => Some((0x0D, 2)),
                Some('t') => Some((0x09, 2)),
                Some('v') => Some((0x0B, 2)),
                Some('f') => Some((0x0C, 2)),
                Some('a') => Some((0x07, 2)),
                Some('b') => Some((0x08, 2)),
                Some('x') => hex(i + 2, 2).map(|u| (u, 4)),
                Some('u') => hex(i + 2, 4).map(|u| (u, 6)),
                _ => None,
            };
            if let Some((unit, len)) = decoded {
                out.push(unit);
                i += len;
                continue;
            }
        }
        push_char(&mut out, s[i]);
        i += 1;
    }
    String::from_utf16_lossy(&out)
}

fn quote_for_shell(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn escape_single_quotes(command: &str) -> String {
    let s: Vec<char> = command.chars().collect();
    let mut out = String::with_capacity(command.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == '\\' && s.get(i + 1) == Some(&'\'') && s.get(i + 2) != Some(&'\'') {
            out.push_str("'\\''");
            i += 2;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

fn fix_concatenated_methods(mut command: String) -> String {
    const FIXES: [(&str, &str); 8] = [
        (" -XPOST", " -X POST"),
        (" -XGET", " -X GET"),
        (" -XPUT", " -X PUT"),
        (" -XPATCH", " -X PATCH"),
        (" -XDELETE", " -X DELETE"),
        (" -XOPTIONS", " -X OPTIONS"),
        (" -XHEAD", " -X HEAD"),
        (" -Xnull", " "),
    ];
    for (from, to) in FIXES {
        command = command.replacen(from, to, 1);
    }
    command
}
