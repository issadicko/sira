//! OAuth 2.0 : obtenir le jeton d'accès (client credentials, mot de passe, code d'autorisation avec PKCE, implicite), le
//! garder pour les requêtes suivantes, le rafraîchir à l'expiration, et le poser sur la requête.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde::Serialize;
use serde_json::{Map, Value};
use url::Url;
use xc_core::oauth2::{OAuth2, AUTHORIZATION_CODE, CLIENT_CREDENTIALS, IMPLICIT, PASSWORD};
use xc_engine::digest::client_nonce;
use xc_engine::{aws::sha256, HttpRequest, HttpResponse};

/// Un jeton est tenu pour expiré peu avant sa vraie expiration, pour ne pas partir avec un jeton qui meurt en route.
const SKEW: Duration = Duration::from_secs(10);
const TOKEN_TIMEOUT: Duration = Duration::from_secs(30);

/// Un jeton obtenu auprès du serveur d'autorisation.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    /// Nom sous lequel les scripts le lisent (`$oauth2.<id>.access_token`).
    pub id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    /// Instant d'expiration, en millisecondes Unix ; `None` : le serveur n'en a pas annoncé.
    pub expires_at: Option<u64>,
    /// La réponse du serveur en entier.
    pub raw: Map<String, Value>,
}

impl Token {
    pub fn expired(&self, now_ms: u64) -> bool {
        self.expires_at.is_some_and(|at| now_ms + u64::try_from(SKEW.as_millis()).unwrap_or(0) >= at)
    }

    /// La valeur que la requête porte : le jeton d'accès, ou l'`id_token` quand la configuration le demande.
    fn value(&self, source: &str) -> Option<&str> {
        if source == "id_token" {
            self.id_token.as_deref()
        } else {
            Some(self.access_token.as_str())
        }
    }
}

/// Une page où l'utilisateur se connecte : l'adresse à ouvrir et celle où le serveur le renvoie.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorizationRequest {
    pub url: String,
    pub callback_url: String,
}

/// Ce que l'hôte (l'application) fait de l'interaction qu'exigent le code d'autorisation et le flux implicite : ouvrir
/// `request.url`, attendre la redirection vers `callback_url` et rendre les paramètres de la requête et du fragment de
/// l'adresse où elle a abouti.
pub trait Authorizer: Send + Sync + std::fmt::Debug {
    fn authorize(&self, request: AuthorizationRequest) -> Authorization<'_>;
}

/// La réponse attendue de l'hôte : les paramètres de l'adresse de retour, ou la raison de l'échec.
pub type Authorization<'a> = Pin<Box<dyn Future<Output = Result<HashMap<String, String>, String>> + Send + 'a>>;

pub type SharedAuthorizer = Arc<dyn Authorizer>;

/// Les paramètres (requête, puis fragment) de `landed` quand cette adresse est celle où le serveur d'autorisation renvoie
/// l'utilisateur (`callback`) : même schéma, même hôte, même port et même chemin. `None` pour toute autre adresse.
pub fn redirect_params(landed: &str, callback: &str) -> Option<HashMap<String, String>> {
    let (landed, callback) = (Url::parse(landed).ok()?, Url::parse(callback).ok()?);
    let same = landed.scheme() == callback.scheme()
        && landed.host_str() == callback.host_str()
        && landed.port_or_known_default() == callback.port_or_known_default()
        && landed.path().trim_end_matches('/') == callback.path().trim_end_matches('/');
    if !same {
        return None;
    }
    let mut params: HashMap<String, String> =
        landed.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
    if let Some(fragment) = landed.fragment() {
        params.extend(form_urlencoded::parse(fragment.as_bytes()).map(|(k, v)| (k.into_owned(), v.into_owned())));
    }
    Some(params)
}

fn now_ms() -> u64 {
    u64::try_from(SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()).unwrap_or(u64::MAX)
}

/// La clé d'un jeton : le même serveur, le même client et le même nom partagent le jeton.
pub fn token_key(config: &OAuth2) -> String {
    let url = if config.flow == IMPLICIT { &config.authorization_url } else { &config.access_token_url };
    format!("{url}|{}|{}", config.client_id, config.token_id)
}

fn form(pairs: &[(String, String)]) -> Vec<u8> {
    let mut out = form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        out.append_pair(k, v);
    }
    out.finish().into_bytes()
}

/// Ajoute `name=value` à la requête de `url`.
fn with_query(url: &str, name: &str, value: &str) -> String {
    match Url::parse(url) {
        Ok(mut parsed) => {
            parsed.query_pairs_mut().append_pair(name, value);
            parsed.to_string()
        }
        Err(_) => url.to_owned(),
    }
}

/// La requête vers le point d'accès aux jetons : le corps `grant`, l'identification du client et les paramètres que
/// l'utilisateur ajoute à l'étape `stage`.
fn token_request(config: &OAuth2, url: &str, stage: &str, mut grant: Vec<(String, String)>) -> HttpRequest {
    let mut headers = vec![
        ("Content-Type".to_owned(), "application/x-www-form-urlencoded".to_owned()),
        ("Accept".to_owned(), "application/json".to_owned()),
    ];
    let mut url = url.to_owned();
    if config.credentials_placement == "body" {
        if !config.client_id.is_empty() {
            grant.push(("client_id".into(), config.client_id.clone()));
        }
        if !config.client_secret.is_empty() {
            grant.push(("client_secret".into(), config.client_secret.clone()));
        }
    } else if !config.client_id.is_empty() || !config.client_secret.is_empty() {
        let raw = format!("{}:{}", config.client_id, config.client_secret);
        headers
            .push(("Authorization".into(), format!("Basic {}", base64::engine::general_purpose::STANDARD.encode(raw))));
    }
    for param in config.parameters.iter().filter(|p| p.stage == stage && !p.name.is_empty()) {
        match param.placement.as_str() {
            "header" => headers.push((param.name.clone(), param.value.clone())),
            "query" => url = with_query(&url, &param.name, &param.value),
            _ => grant.push((param.name.clone(), param.value.clone())),
        }
    }
    HttpRequest {
        method: "POST".into(),
        url,
        headers,
        body: Some(form(&grant)),
        timeout: TOKEN_TIMEOUT,
        max_response_body: Some(1 << 20),
    }
}

fn describe(response: &HttpResponse, body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let field = |name: &str| parsed.as_ref().and_then(|v| v.get(name)).and_then(Value::as_str).map(str::to_owned);
    match (field("error"), field("error_description")) {
        (Some(error), Some(description)) => format!("{error} : {description}"),
        (Some(error), None) => error,
        _ => format!("{} {}", response.status, response.reason).trim().to_owned(),
    }
}

fn json_or_form(body: &str) -> Map<String, Value> {
    if let Ok(Value::Object(map)) = serde_json::from_str(body) {
        return map;
    }
    form_urlencoded::parse(body.as_bytes()).map(|(k, v)| (k.into_owned(), Value::String(v.into_owned()))).collect()
}

fn text(map: &Map<String, Value>, key: &str) -> Option<String> {
    match map.get(key)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Le jeton de la réponse du serveur ; `previous` fournit le jeton de rafraîchissement quand le serveur n'en renvoie pas.
fn token_of(config: &OAuth2, body: &str, previous: Option<&Token>, at: u64) -> Result<Token, String> {
    let raw = json_or_form(body);
    let access_token = text(&raw, "access_token")
        .ok_or_else(|| "OAuth 2 : la réponse du serveur ne contient pas d'access_token".to_owned())?;
    let seconds = text(&raw, "expires_in").and_then(|s| s.parse::<f64>().ok()).filter(|s| *s > 0.0);
    let token = Token {
        id: config.token_id.clone(),
        access_token,
        refresh_token: text(&raw, "refresh_token").or_else(|| previous.and_then(|p| p.refresh_token.clone())),
        id_token: text(&raw, "id_token"),
        token_type: text(&raw, "token_type"),
        scope: text(&raw, "scope"),
        expires_at: seconds.map(|s| at + (s * 1000.0) as u64),
        raw,
    };
    if config.token_source == "id_token" && token.id_token.is_none() {
        return Err("OAuth 2 : la réponse du serveur ne contient pas d'id_token".into());
    }
    Ok(token)
}

async fn exchange(config: &OAuth2, request: HttpRequest, previous: Option<&Token>) -> Result<Token, String> {
    let response = xc_engine::send(request).await.map_err(|e| format!("OAuth 2 : {e}"))?;
    let body = xc_engine::lossy_text(response.body.clone());
    if !(200..300).contains(&response.status) {
        return Err(format!("OAuth 2 : le serveur a refusé la demande de jeton ({})", describe(&response, &body)));
    }
    token_of(config, &body, previous, now_ms())
}

/// 64 caractères du jeu autorisé pour un `code_verifier` (RFC 7636).
fn code_verifier() -> String {
    format!("{}{}", client_nonce(), client_nonce())
}

fn code_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes()))
}

async fn interact(
    config: &OAuth2,
    authorizer: Option<&SharedAuthorizer>,
    response_type: &str,
    challenge: Option<&str>,
) -> Result<(HashMap<String, String>, String), String> {
    let authorizer = authorizer.ok_or_else(|| {
        format!(
            "OAuth 2 : le flux « {} » demande une fenêtre de connexion ; lance la requête depuis l'application",
            config.flow
        )
    })?;
    if config.authorization_url.is_empty() {
        return Err("OAuth 2 : l'adresse d'autorisation est vide".into());
    }
    if config.callback_url.is_empty() {
        return Err("OAuth 2 : l'adresse de rappel (Callback URL) est vide".into());
    }
    let state = if config.state.is_empty() { client_nonce() } else { config.state.clone() };
    let mut url = Url::parse(&config.authorization_url)
        .map_err(|e| format!("OAuth 2 : adresse d'autorisation invalide ({e})"))?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", response_type);
        query.append_pair("client_id", &config.client_id);
        if !config.callback_url.is_empty() {
            query.append_pair("redirect_uri", &config.callback_url);
        }
        if !config.scope.is_empty() {
            query.append_pair("scope", &config.scope);
        }
        query.append_pair("state", &state);
        if let Some(challenge) = challenge {
            query.append_pair("code_challenge", challenge);
            query.append_pair("code_challenge_method", "S256");
        }
        for param in config
            .parameters
            .iter()
            .filter(|p| p.stage == "authorization" && p.placement == "query" && !p.name.is_empty())
        {
            query.append_pair(&param.name, &param.value);
        }
    }
    let params = authorizer
        .authorize(AuthorizationRequest { url: url.to_string(), callback_url: config.callback_url.clone() })
        .await
        .map_err(|e| format!("OAuth 2 : {e}"))?;
    if let Some(error) = params.get("error") {
        let description = params.get("error_description").map(|d| format!(" : {d}")).unwrap_or_default();
        return Err(format!("OAuth 2 : l'autorisation a été refusée ({error}{description})"));
    }
    if params.get("state").is_some_and(|returned| *returned != state) {
        return Err("OAuth 2 : le paramètre state de la réponse ne correspond pas à celui de la demande".into());
    }
    Ok((params, state))
}

/// Demande un jeton neuf au serveur d'autorisation.
pub async fn fetch(config: &OAuth2, authorizer: Option<&SharedAuthorizer>) -> Result<Token, String> {
    let pair = |k: &str, v: &str| (k.to_owned(), v.to_owned());
    let scope = (!config.scope.is_empty()).then(|| pair("scope", &config.scope));
    if config.flow != IMPLICIT && config.access_token_url.is_empty() {
        return Err("OAuth 2 : l'adresse du jeton (Access Token URL) est vide".into());
    }
    match config.flow.as_str() {
        CLIENT_CREDENTIALS => {
            let grant = [Some(pair("grant_type", "client_credentials")), scope].into_iter().flatten().collect();
            exchange(config, token_request(config, &config.access_token_url, "token", grant), None).await
        }
        PASSWORD => {
            let grant = [
                Some(pair("grant_type", "password")),
                Some(pair("username", &config.username)),
                Some(pair("password", &config.password)),
                scope,
            ]
            .into_iter()
            .flatten()
            .collect();
            exchange(config, token_request(config, &config.access_token_url, "token", grant), None).await
        }
        AUTHORIZATION_CODE => {
            let verifier = config.pkce.then(code_verifier);
            let challenge = verifier.as_deref().map(code_challenge);
            let (params, _) = interact(config, authorizer, "code", challenge.as_deref()).await?;
            let code = params
                .get("code")
                .ok_or_else(|| "OAuth 2 : la redirection ne contient pas de code d'autorisation".to_owned())?;
            let mut grant = vec![pair("grant_type", "authorization_code"), pair("code", code)];
            if !config.callback_url.is_empty() {
                grant.push(pair("redirect_uri", &config.callback_url));
            }
            if let Some(verifier) = &verifier {
                grant.push(pair("code_verifier", verifier));
            }
            exchange(config, token_request(config, &config.access_token_url, "token", grant), None).await
        }
        IMPLICIT => {
            let (params, _) = interact(config, authorizer, "token", None).await?;
            let raw: Map<String, Value> = params.into_iter().map(|(k, v)| (k, Value::String(v))).collect();
            token_of(config, &serde_json::to_string(&raw).unwrap_or_default(), None, now_ms())
        }
        other => Err(format!("OAuth 2 : flux inconnu « {other} »")),
    }
}

/// Échange le jeton de rafraîchissement de `token` contre un nouveau jeton.
pub async fn refresh(config: &OAuth2, token: &Token) -> Result<Token, String> {
    let refresh_token = token.refresh_token.as_deref().ok_or("OAuth 2 : pas de jeton de rafraîchissement")?;
    let url = if config.refresh_token_url.is_empty() { &config.access_token_url } else { &config.refresh_token_url };
    let grant = vec![
        ("grant_type".to_owned(), "refresh_token".to_owned()),
        ("refresh_token".to_owned(), refresh_token.to_owned()),
    ];
    exchange(config, token_request(config, url, "refresh", grant), Some(token)).await
}

/// Le jeton à poser sur la requête : celui que la session garde s'il est valide, sinon le jeton rafraîchi ou obtenu de
/// nouveau selon les réglages ; `None` quand aucun jeton n'est à poser (`autoFetchToken` désactivé, ou aucun jeton).
pub async fn token_for(
    config: &OAuth2,
    tokens: &mut HashMap<String, Token>,
    authorizer: Option<&SharedAuthorizer>,
) -> Result<Option<Token>, String> {
    let key = token_key(config);
    if let Some(token) = tokens.get(&key).cloned() {
        if !token.expired(now_ms()) {
            return Ok(Some(token));
        }
        if config.auto_refresh_token && token.refresh_token.is_some() {
            if let Ok(fresh) = refresh(config, &token).await {
                tokens.insert(key, fresh.clone());
                return Ok(Some(fresh));
            }
        }
        tokens.remove(&key);
    }
    if !config.auto_fetch_token {
        return Ok(None);
    }
    let token = fetch(config, authorizer).await?;
    tokens.insert(key, token.clone());
    Ok(Some(token))
}

/// Pose le jeton sur la requête : en-tête `Authorization` ou paramètre d'adresse, selon la configuration.
pub fn apply(request: &mut HttpRequest, config: &OAuth2, token: &Token) -> Result<(), String> {
    let value = token
        .value(&config.token_source)
        .ok_or_else(|| "OAuth 2 : le jeton demandé (id_token) est absent de la réponse".to_owned())?;
    if config.token_placement == "query" {
        let mut url = Url::parse(&request.url).map_err(|e| e.to_string())?;
        let others: Vec<(String, String)> = url
            .query_pairs()
            .filter(|(k, _)| *k != config.token_query_key)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        url.query_pairs_mut().clear().extend_pairs(others).append_pair(&config.token_query_key, value);
        request.url = url.to_string();
    } else {
        request.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
        request.headers.push(("Authorization".into(), format!("{} {value}", config.token_prefix).trim().to_owned()));
    }
    Ok(())
}

/// Ce que l'interface montre d'un jeton : jamais sa valeur.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub id: String,
    pub token_type: Option<String>,
    pub scope: Option<String>,
    pub expires_at: Option<u64>,
    pub expired: bool,
    pub has_refresh_token: bool,
}

impl Token {
    pub fn info(&self) -> TokenInfo {
        TokenInfo {
            id: self.id.clone(),
            token_type: self.token_type.clone(),
            scope: self.scope.clone(),
            expires_at: self.expires_at,
            expired: self.expired(now_ms()),
            has_refresh_token: self.refresh_token.is_some(),
        }
    }
}

/// Les variables que les scripts lisent : `$oauth2.<id>.<champ>` pour chaque champ de la réponse du serveur.
pub fn script_vars(tokens: &HashMap<String, Token>) -> Map<String, Value> {
    let mut out = Map::new();
    for token in tokens.values() {
        for (key, value) in &token.raw {
            out.insert(format!("$oauth2.{}.{key}", token.id), value.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ef_aut_02_the_redirect_is_recognised_by_scheme_host_port_and_path() {
        let cb = "http://localhost:8080/callback";
        let query = redirect_params("http://localhost:8080/callback?code=abc&state=xyz", cb).unwrap();
        assert_eq!((query["code"].as_str(), query["state"].as_str()), ("abc", "xyz"));
        assert!(
            redirect_params("http://localhost:8080/callback/?code=1", cb).is_some(),
            "la barre finale ne compte pas"
        );
        for other in [
            "https://localhost:8080/callback?code=1",
            "http://localhost:9090/callback?code=1",
            "http://127.0.0.1:8080/callback",
            "http://localhost:8080/other",
        ] {
            assert!(redirect_params(other, cb).is_none(), "{other}");
        }
        assert!(redirect_params("pas une adresse", cb).is_none() && redirect_params(cb, "pas une adresse").is_none());
    }

    #[test]
    fn ef_aut_02_the_implicit_flow_reads_the_fragment_and_a_missing_port_means_the_default() {
        let params = redirect_params(
            "http://localhost/cb#access_token=t%20k&token_type=bearer&state=s",
            "http://localhost:80/cb",
        )
        .unwrap();
        assert_eq!((params["access_token"].as_str(), params["token_type"].as_str()), ("t k", "bearer"));
        let both = redirect_params("http://localhost/cb?error=access_denied#state=s", "http://localhost/cb").unwrap();
        assert_eq!((both["error"].as_str(), both["state"].as_str()), ("access_denied", "s"));
    }

    fn config(url: &str) -> OAuth2 {
        OAuth2 { access_token_url: url.into(), client_id: "c".into(), ..OAuth2::default() }
    }

    #[test]
    fn ef_aut_02_a_token_is_shared_by_server_client_and_name_only() {
        let a = token_key(&config("https://a/token"));
        assert_eq!(a, token_key(&config("https://a/token")));
        assert_ne!(a, token_key(&config("https://b/token")));
        assert_ne!(a, token_key(&OAuth2 { token_id: "other".into(), ..config("https://a/token") }));
    }

    #[test]
    fn ef_aut_02_a_token_expires_ten_seconds_early_and_never_without_an_announced_lifetime() {
        let at = |expires_at| Token {
            id: "t".into(),
            access_token: "a".into(),
            refresh_token: None,
            id_token: None,
            token_type: None,
            scope: None,
            expires_at,
            raw: Map::new(),
        };
        assert!(!at(None).expired(u64::MAX / 2));
        assert!(!at(Some(100_000)).expired(80_000));
        assert!(at(Some(100_000)).expired(90_000));
    }

    #[test]
    fn ef_aut_02_a_token_response_may_be_json_or_a_form_and_keeps_the_old_refresh_token() {
        let config = config("https://a/token");
        let json = token_of(&config, r#"{"access_token":"a","expires_in":"60","scope":"x y"}"#, None, 1_000).unwrap();
        assert_eq!((json.expires_at, json.scope.as_deref()), (Some(61_000), Some("x y")));
        let form = token_of(&config, "access_token=b&token_type=bearer", None, 1_000).unwrap();
        assert_eq!((form.access_token.as_str(), form.expires_at), ("b", None));
        let previous = Token { refresh_token: Some("r".into()), ..json };
        assert_eq!(
            token_of(&config, r#"{"access_token":"c"}"#, Some(&previous), 0).unwrap().refresh_token.as_deref(),
            Some("r")
        );
        assert!(token_of(&config, r#"{"expires_in":5}"#, None, 0).is_err());
    }

    #[test]
    fn ef_aut_02_the_interface_never_receives_the_token_value() {
        let token = Token {
            id: "t".into(),
            access_token: "SECRET".into(),
            refresh_token: Some("R".into()),
            id_token: None,
            token_type: Some("Bearer".into()),
            scope: None,
            expires_at: None,
            raw: Map::new(),
        };
        let json = serde_json::to_string(&token.info()).unwrap();
        assert!(!json.contains("SECRET") && !json.contains("\"R\""), "{json}");
        assert!(json.contains("\"hasRefreshToken\":true"), "{json}");
    }

    #[test]
    fn ef_aut_02_scripts_read_every_field_of_the_response_by_token_name() {
        let raw: Map<String, Value> = serde_json::from_str(r#"{"access_token":"a","custom":7}"#).unwrap();
        let token = Token {
            id: "creds".into(),
            access_token: "a".into(),
            refresh_token: None,
            id_token: None,
            token_type: None,
            scope: None,
            expires_at: None,
            raw,
        };
        let vars = script_vars(&HashMap::from([("k".to_owned(), token)]));
        assert_eq!(
            (vars["$oauth2.creds.access_token"].as_str(), vars["$oauth2.creds.custom"].as_i64()),
            (Some("a"), Some(7))
        );
    }
}
