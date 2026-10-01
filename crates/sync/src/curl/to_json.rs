//! Port de `curl-to-json.js` : mise en forme de la requête analysée (URL, en-têtes, corps, authentification).

use serde_json::{json, Map, Value};

use super::content_type::is_structured_content_type;
use super::js::{JsError, JsObject};
use super::parse::{parse_curl_command, Auth, ParsedCurl};
use super::query::{build_query_string, query_string_parse, QsValue};
use super::shell::Token;

/// Objet renvoyé par `curlToJson`.
#[derive(Clone, Debug)]
pub struct CurlJson {
    pub(crate) url: String,
    pub(crate) raw_url: String,
    pub(crate) method: String,
    pub(crate) is_data_binary: bool,
    pub(crate) cookies: Option<JsObject<String>>,
    pub(crate) headers: Option<JsObject<Option<Token>>>,
    pub(crate) data: Option<Value>,
    pub(crate) insecure: bool,
    pub(crate) auth: Option<Auth>,
}

impl CurlJson {
    /// Vue JSON (`JSON.stringify`) de l'objet renvoyé par `curlToJson` ; seul l'ordre des clés peut différer
    /// (Bruno ajoute `headers` après `data` pour un formulaire multipart sans en-tête).
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert("url".into(), json!(self.url));
        map.insert("raw_url".into(), json!(self.raw_url));
        map.insert("method".into(), json!(self.method));
        if self.is_data_binary {
            map.insert("isDataBinary".into(), json!(true));
        }
        if let Some(cookies) = &self.cookies {
            map.insert("cookies".into(), cookies.to_json(|v| Some(json!(v))));
        }
        if let Some(headers) = &self.headers {
            map.insert("headers".into(), headers.to_json(|v| v.as_ref().map(Token::to_json)));
        }
        if let Some(data) = &self.data {
            map.insert("data".into(), data.clone());
        }
        if self.insecure {
            map.insert("insecure".into(), json!(false));
        }
        if let Some(auth) = &self.auth {
            map.insert("auth".into(), auth.to_json());
        }
        Value::Object(map)
    }
}

fn with_http_scheme(url: String) -> String {
    if url.contains("http:") || url.contains("https:") {
        url
    } else {
        format!("http://{url}")
    }
}

/// `curlToJson(curlCommand)`.
pub fn curl_to_json(command: &str) -> Result<Option<CurlJson>, JsError> {
    let request = parse_curl_command(command)?;
    let Some(raw_url) = request.url.clone().filter(|u| !u.is_empty()) else { return Ok(None) };
    let raw_url = with_http_scheme(raw_url);
    let mut url = with_http_scheme(request.url_without_query.clone().unwrap_or_default());
    let mut headers = (!request.headers.is_empty()).then(|| request.headers.clone());
    if !request.queries.is_empty() {
        url = format!("{url}?{}", build_query_string(&request.queries));
    }
    let data = match (&request.multipart_uploads, request.data.as_ref().and_then(Token::as_word)) {
        (Some(uploads), _) => {
            headers
                .get_or_insert_with(JsObject::new)
                .set("Content-Type", Some(Token::Word("multipart/form-data".into())));
            Some(uploads.iter().map(|u| u.to_json()).collect())
        }
        (None, Some(data)) if request.is_data_binary && data.starts_with('@') => Some(files_data(&request, data)?),
        (None, Some(data)) => Some(data_string(&request, data)),
        (None, None) => None,
    };
    Ok(Some(CurlJson {
        url,
        raw_url,
        method: request.method.clone().unwrap_or_default(),
        is_data_binary: request.is_data_binary,
        cookies: request.cookies.clone(),
        headers,
        data,
        insecure: request.insecure,
        auth: request.auth.clone(),
    }))
}

fn files_data(request: &ParsedCurl, data: &str) -> Result<Value, JsError> {
    if request.headers.is_empty() {
        return Err(JsError::type_error("Cannot read properties of undefined (reading 'Content-Type')"));
    }
    let mut file = Map::new();
    file.insert("filePath".into(), json!(&data[1..]));
    if let Some(Some(content_type)) = request.headers.get("Content-Type") {
        file.insert("contentType".into(), content_type.to_json());
    }
    file.insert("selected".into(), json!(true));
    Ok(Value::Array(vec![Value::Object(file)]))
}

fn content_type(headers: &JsObject<Option<Token>>) -> Option<&str> {
    let (_, value) = headers.iter().find(|(name, _)| name.to_lowercase() == "content-type")?;
    value.as_ref().and_then(Token::as_word)
}

fn data_string(request: &ParsedCurl, data: &str) -> Value {
    if is_structured_content_type(content_type(&request.headers)) {
        return json!(data);
    }
    let mut parsed = query_string_parse(data);
    for value in parsed.values_mut() {
        if *value == QsValue::Null {
            *value = QsValue::Str(String::new());
        }
    }
    let single_key_only =
        parsed.len() == 1 && parsed.iter().next().is_some_and(|(_, v)| matches!(v, QsValue::Str(s) if s.is_empty()));
    let mut object: JsObject<Value> = JsObject::new();
    if request.is_data_binary || single_key_only {
        object.set(data, json!(""));
    } else {
        for (key, value) in parsed.iter() {
            object.set(key, value.to_json());
        }
    }
    object.to_json(|v| Some(v.clone()))
}
