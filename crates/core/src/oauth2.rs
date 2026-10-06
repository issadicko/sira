//! La configuration OAuth 2.0 d'une requête, d'un dossier ou de la collection, et sa forme dans OpenCollection YAML
//! (`type: oauth2`, `flow`, `credentials`, `tokenConfig`, `settings`…), comme Bruno l'écrit.

use serde::{Deserialize, Serialize};

use crate::request::{table, text};
use crate::yaml::{self, Map, Value};

pub const CLIENT_CREDENTIALS: &str = "client_credentials";
pub const PASSWORD: &str = "resource_owner_password_credentials";
pub const AUTHORIZATION_CODE: &str = "authorization_code";
pub const IMPLICIT: &str = "implicit";

/// Un paramètre ajouté par l'utilisateur à une des requêtes du flux.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthParam {
    /// `authorization`, `token` ou `refresh` : la requête du flux qui le reçoit.
    pub stage: String,
    pub name: String,
    pub value: String,
    /// `header`, `query` ou `body`.
    pub placement: String,
}

/// Un champ vide est absent du fichier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OAuth2 {
    pub flow: String,
    pub authorization_url: String,
    pub access_token_url: String,
    pub refresh_token_url: String,
    pub callback_url: String,
    pub client_id: String,
    pub client_secret: String,
    /// `basic_auth_header` ou `body`.
    pub credentials_placement: String,
    pub username: String,
    pub password: String,
    pub scope: String,
    pub state: String,
    pub pkce: bool,
    /// Nom sous lequel les jetons sont gardés et lus par les scripts (`$oauth2.<id>.access_token`).
    pub token_id: String,
    /// `header` ou `query`.
    pub token_placement: String,
    /// Avant le jeton dans l'en-tête `Authorization` (`Bearer`).
    pub token_prefix: String,
    pub token_query_key: String,
    /// `access_token` ou `id_token`.
    pub token_source: String,
    pub auto_fetch_token: bool,
    pub auto_refresh_token: bool,
    pub parameters: Vec<OAuthParam>,
}

impl Default for OAuth2 {
    fn default() -> Self {
        Self {
            flow: CLIENT_CREDENTIALS.into(),
            authorization_url: String::new(),
            access_token_url: String::new(),
            refresh_token_url: String::new(),
            callback_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            credentials_placement: "basic_auth_header".into(),
            username: String::new(),
            password: String::new(),
            scope: String::new(),
            state: String::new(),
            pkce: false,
            token_id: "credentials".into(),
            token_placement: "header".into(),
            token_prefix: "Bearer".into(),
            token_query_key: "access_token".into(),
            token_source: "access_token".into(),
            auto_fetch_token: true,
            auto_refresh_token: false,
            parameters: Vec::new(),
        }
    }
}

const STAGES: [(&str, &str); 3] =
    [("authorization", "authorizationRequest"), ("token", "accessTokenRequest"), ("refresh", "refreshTokenRequest")];

fn placement_of(map: Option<&Map>, key: &str) -> Option<(String, String)> {
    let placement = map?.map(key)?;
    if let Some(prefix) = placement.get("header") {
        return Some(("header".into(), text(Some(prefix))));
    }
    placement.get("query").map(|key| ("query".into(), text(Some(key))))
}

/// La configuration d'une table `auth` de type `oauth2`.
pub fn read(m: &Map) -> OAuth2 {
    let field = |k: &str| text(m.get(k));
    let credentials = m.map("credentials");
    let owner = m.map("resourceOwner");
    let token = m.map("tokenConfig");
    let settings = m.map("settings");
    let nested = |map: Option<&Map>, key: &str| map.map(|m| text(m.get(key))).unwrap_or_default();
    let defaults = OAuth2::default();
    let mut config = OAuth2 {
        flow: m.str("flow").unwrap_or(&defaults.flow).to_owned(),
        authorization_url: field("authorizationUrl"),
        access_token_url: field("accessTokenUrl"),
        refresh_token_url: field("refreshTokenUrl"),
        callback_url: field("callbackUrl"),
        client_id: nested(credentials, "clientId"),
        client_secret: nested(credentials, "clientSecret"),
        credentials_placement: credentials
            .and_then(|c| c.str("placement"))
            .unwrap_or(&defaults.credentials_placement)
            .to_owned(),
        username: nested(owner, "username"),
        password: nested(owner, "password"),
        scope: field("scope"),
        state: field("state"),
        pkce: m.map("pkce").is_some_and(|p| !p.get("disabled").is_some_and(Value::is_true)),
        token_id: token.and_then(|t| t.str("id")).filter(|id| !id.is_empty()).unwrap_or(&defaults.token_id).to_owned(),
        token_source: token
            .and_then(|t| t.str("source"))
            .filter(|s| !s.is_empty())
            .unwrap_or(&defaults.token_source)
            .to_owned(),
        auto_fetch_token: settings
            .and_then(|s| s.get("autoFetchToken"))
            .map_or(defaults.auto_fetch_token, Value::is_true),
        auto_refresh_token: settings.and_then(|s| s.get("autoRefreshToken")).is_some_and(Value::is_true),
        ..defaults
    };
    match placement_of(token, "placement") {
        Some((kind, value)) if kind == "query" => {
            config.token_placement = kind;
            config.token_query_key = value;
        }
        Some((_, prefix)) => config.token_prefix = prefix,
        None => {}
    }
    if let Some(extra) = m.map("additionalParameters") {
        for (stage, key) in STAGES {
            for item in extra.seq(key).iter().filter_map(Value::as_map) {
                config.parameters.push(OAuthParam {
                    stage: stage.into(),
                    name: text(item.get("name")),
                    value: text(item.get("value")),
                    placement: item.str("placement").unwrap_or("header").into(),
                });
            }
        }
    }
    config
}

fn pairs(entries: Vec<(&str, &str)>) -> Option<Map> {
    let kept: Vec<(&str, Value)> =
        entries.into_iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (k, Value::str(v))).collect();
    (!kept.is_empty()).then(|| table(kept))
}

/// `additionalParameters` : les paramètres de chaque requête du flux, ceux d'une requête que le flux n'envoie pas sont
/// écartés.
fn additional(c: &OAuth2) -> Option<Value> {
    let mut stages = Vec::new();
    for (stage, key) in STAGES {
        let sent = if stage == "authorization" {
            c.flow == AUTHORIZATION_CODE || c.flow == IMPLICIT
        } else {
            c.flow != IMPLICIT
        };
        let items: Vec<Value> = c
            .parameters
            .iter()
            .filter(|p| sent && p.stage == stage && !p.name.trim().is_empty())
            .map(|p| {
                let mut fields = vec![("name", Value::str(p.name.trim()))];
                if !p.value.is_empty() {
                    fields.push(("value", Value::str(&p.value)));
                }
                fields.push(("placement", Value::str(&p.placement)));
                Value::Map(table(fields))
            })
            .collect();
        if !items.is_empty() {
            stages.push((key, Value::Seq(items)));
        }
    }
    (!stages.is_empty()).then(|| Value::Map(table(stages)))
}

/// La table `auth` de `config`, dans l'ordre où Bruno l'écrit pour son flux ; les champs vides n'y figurent pas.
pub fn write(config: &OAuth2) -> Map {
    let c = config;
    let mut out: Vec<(&str, Value)> = vec![("type", Value::str("oauth2")), ("flow", Value::str(&c.flow))];
    let put = |out: &mut Vec<(&'static str, Value)>, key: &'static str, value: &str| {
        if !value.is_empty() {
            out.push((key, Value::str(value)));
        }
    };
    let interactive = c.flow == AUTHORIZATION_CODE || c.flow == IMPLICIT;
    if interactive {
        put(&mut out, "authorizationUrl", &c.authorization_url);
    }
    if c.flow != IMPLICIT {
        put(&mut out, "accessTokenUrl", &c.access_token_url);
        put(&mut out, "refreshTokenUrl", &c.refresh_token_url);
    }
    if interactive {
        put(&mut out, "callbackUrl", &c.callback_url);
    }
    let credentials = if c.flow == IMPLICIT {
        pairs(vec![("clientId", &c.client_id)])
    } else {
        pairs(vec![
            ("clientId", &c.client_id),
            ("clientSecret", &c.client_secret),
            ("placement", &c.credentials_placement),
        ])
    };
    if let Some(credentials) = credentials {
        out.push(("credentials", Value::Map(credentials)));
    }
    if c.flow == PASSWORD {
        if let Some(owner) = pairs(vec![("username", &c.username), ("password", &c.password)]) {
            out.push(("resourceOwner", Value::Map(owner)));
        }
    }
    let extra = additional(c);
    match c.flow.as_str() {
        AUTHORIZATION_CODE => {
            out.extend(extra.map(|e| ("additionalParameters", e)));
            put(&mut out, "scope", &c.scope);
            put(&mut out, "state", &c.state);
            let pkce = if c.pkce { Map::default() } else { table(vec![("disabled", Value::Bool(true))]) };
            out.push(("pkce", Value::Map(pkce)));
        }
        IMPLICIT => {
            put(&mut out, "scope", &c.scope);
            put(&mut out, "state", &c.state);
            out.extend(extra.map(|e| ("additionalParameters", e)));
        }
        _ => {
            put(&mut out, "scope", &c.scope);
            out.extend(extra.map(|e| ("additionalParameters", e)));
        }
    }
    let placement = if c.token_placement == "query" {
        table(vec![("query", Value::str(&c.token_query_key))])
    } else {
        table(vec![("header", Value::str(&c.token_prefix))])
    };
    let mut token = Vec::new();
    if !c.token_id.is_empty() {
        token.push(("id", Value::str(&c.token_id)));
    }
    token.push(("placement", Value::Map(placement)));
    token.push(("source", Value::str(&c.token_source)));
    out.push(("tokenConfig", Value::Map(table(token))));
    out.push((
        "settings",
        Value::Map(table(vec![
            ("autoFetchToken", Value::Bool(c.auto_fetch_token)),
            ("autoRefreshToken", Value::Bool(c.auto_refresh_token)),
        ])),
    ));
    table(out)
}

/// La configuration sans son `type`, en YAML trié : ce que la revue d'un conflit montre, et ce qui distingue deux
/// configurations différentes.
pub fn canonical(config: &OAuth2) -> String {
    let mut map = write(config);
    map.0.retain(|(k, _)| k != "type");
    yaml::emit(&Value::Map(map).sorted(), &[]).trim_end().to_owned()
}

impl OAuth2 {
    /// La configuration avec `fill` appliqué à chaque champ texte : les variables résolues.
    pub fn resolved(&self, mut fill: impl FnMut(&str) -> String) -> Self {
        let mut config = self.clone();
        for field in [
            &mut config.authorization_url,
            &mut config.access_token_url,
            &mut config.refresh_token_url,
            &mut config.callback_url,
            &mut config.client_id,
            &mut config.client_secret,
            &mut config.username,
            &mut config.password,
            &mut config.scope,
            &mut config.state,
            &mut config.token_id,
            &mut config.token_prefix,
            &mut config.token_query_key,
        ] {
            *field = fill(field);
        }
        for param in &mut config.parameters {
            param.name = fill(&param.name);
            param.value = fill(&param.value);
        }
        config
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml;

    fn auth(text: &str) -> Map {
        match yaml::parse(text).unwrap() {
            Value::Map(m) => m,
            _ => panic!("table attendue"),
        }
    }

    const PASSWORD_FLOW: &str = "type: oauth2\nflow: resource_owner_password_credentials\naccessTokenUrl: \"{{KLC_URL}}/protocol/openid-connect/token\"\ncredentials:\n  clientId: sentinel\n  placement: basic_auth_header\nresourceOwner:\n  username: \"{{KLC_USERNAME}}\"\n  password: \"{{KLC_PASSWORD}}\"\nscope: openid\ntokenConfig:\n  id: access_token\n  placement:\n    header: Bearer\n  source: access_token\nsettings:\n  autoFetchToken: true\n  autoRefreshToken: true\n";
    const CODE_FLOW: &str = "type: oauth2\nflow: authorization_code\nauthorizationUrl: https://auth.example/auth\naccessTokenUrl: https://auth.example/token\ncallbackUrl: http://localhost\ncredentials:\n  clientId: \"{{ID}}\"\n  clientSecret: \"{{SECRET}}\"\n  placement: body\nscope: openid read\npkce: {}\ntokenConfig:\n  id: credentials\n  placement:\n    header: Bearer\n  source: access_token\nsettings:\n  autoFetchToken: true\n  autoRefreshToken: true\n";
    const QUERY_FLOW: &str = "type: oauth2\nflow: client_credentials\naccessTokenUrl: \"{{baseUrl}}/oauth/token?trace=oauth-demo\"\ncredentials:\n  clientId: demo-client\n  clientSecret: demo-secret\n  placement: body\nadditionalParameters:\n  accessTokenRequest:\n    - name: X-Demo-Tenant\n      value: demo-tenant\n      placement: header\n    - name: audience\n      value: missio-demo\n      placement: body\ntokenConfig:\n  placement:\n    query: access_token\n";

    #[test]
    fn ef_aut_02_a_file_written_by_bruno_is_read_and_written_back_identically() {
        for text in [PASSWORD_FLOW, CODE_FLOW] {
            let config = read(&auth(text));
            assert_eq!(yaml::emit(&Value::Map(write(&config)), &[]), text, "{config:?}");
        }
    }

    #[test]
    fn ef_aut_02_the_password_flow_reads_its_owner_and_settings() {
        let c = read(&auth(PASSWORD_FLOW));
        assert_eq!(
            (c.flow.as_str(), c.client_id.as_str(), c.username.as_str(), c.scope.as_str()),
            (PASSWORD, "sentinel", "{{KLC_USERNAME}}", "openid")
        );
        assert!(c.auto_fetch_token && c.auto_refresh_token && !c.pkce);
        assert_eq!(
            (c.token_id.as_str(), c.token_prefix.as_str(), c.token_placement.as_str()),
            ("access_token", "Bearer", "header")
        );
    }

    #[test]
    fn ef_aut_02_the_authorization_code_flow_reads_pkce_and_urls() {
        let c = read(&auth(CODE_FLOW));
        assert!(c.pkce);
        assert_eq!(
            (c.authorization_url.as_str(), c.callback_url.as_str(), c.credentials_placement.as_str()),
            ("https://auth.example/auth", "http://localhost", "body")
        );
        let disabled = read(&auth("type: oauth2\nflow: authorization_code\npkce:\n  disabled: true\n"));
        assert!(!disabled.pkce);
        assert!(
            !read(&auth("type: oauth2\nflow: authorization_code\n")).pkce,
            "sans clé pkce, il est désactivé comme chez Bruno"
        );
    }

    #[test]
    fn ef_aut_02_a_token_in_the_query_and_additional_parameters_are_read() {
        let c = read(&auth(QUERY_FLOW));
        assert_eq!((c.token_placement.as_str(), c.token_query_key.as_str()), ("query", "access_token"));
        assert_eq!(c.parameters.len(), 2);
        assert_eq!(
            (c.parameters[1].stage.as_str(), c.parameters[1].name.as_str(), c.parameters[1].placement.as_str()),
            ("token", "audience", "body")
        );
        assert!(
            c.auto_fetch_token && !c.auto_refresh_token,
            "réglages absents : jeton obtenu à la demande, sans rafraîchissement"
        );
        let written = yaml::emit(&Value::Map(write(&c)), &[]);
        assert!(written.contains("additionalParameters:\n  accessTokenRequest:\n    - name: X-Demo-Tenant\n      value: demo-tenant\n      placement: header\n"), "{written}");
        assert!(
            written.contains(
                "tokenConfig:\n  id: credentials\n  placement:\n    query: access_token\n  source: access_token\n"
            ),
            "{written}"
        );
    }

    #[test]
    fn ef_aut_02_an_empty_header_prefix_stays_empty() {
        let c = read(&auth("type: oauth2\nflow: authorization_code\ntokenConfig:\n  placement:\n    header: \"\"\n"));
        assert_eq!(c.token_prefix, "");
        assert!(yaml::emit(&Value::Map(write(&c)), &[]).contains("    header: \"\"\n"));
    }

    #[test]
    fn ef_aut_02_a_new_config_writes_only_what_its_flow_uses() {
        let mut c = OAuth2 { access_token_url: "https://x/token".into(), client_id: "id".into(), ..OAuth2::default() };
        let text = yaml::emit(&Value::Map(write(&c)), &[]);
        assert_eq!(text, "type: oauth2\nflow: client_credentials\naccessTokenUrl: https://x/token\ncredentials:\n  clientId: id\n  placement: basic_auth_header\ntokenConfig:\n  id: credentials\n  placement:\n    header: Bearer\n  source: access_token\nsettings:\n  autoFetchToken: true\n  autoRefreshToken: false\n");
        c.flow = IMPLICIT.into();
        c.authorization_url = "https://x/auth".into();
        c.client_secret = "never".into();
        let implicit = yaml::emit(&Value::Map(write(&c)), &[]);
        assert!(
            !implicit.contains("accessTokenUrl") && !implicit.contains("clientSecret") && !implicit.contains("pkce"),
            "{implicit}"
        );
        assert!(implicit.contains("authorizationUrl: https://x/auth"), "{implicit}");
    }

    #[test]
    fn ef_aut_02_parameters_without_a_name_are_not_written() {
        let c = OAuth2 {
            parameters: vec![
                OAuthParam { stage: "token".into(), name: "  ".into(), value: "x".into(), placement: "body".into() },
                OAuthParam {
                    stage: "refresh".into(),
                    name: "tenant".into(),
                    value: String::new(),
                    placement: "header".into(),
                },
            ],
            ..OAuth2::default()
        };
        let text = yaml::emit(&Value::Map(write(&c)), &[]);
        assert!(
            !text.contains("accessTokenRequest")
                && text.contains("refreshTokenRequest:\n    - name: tenant\n      placement: header\n"),
            "{text}"
        );
    }

    #[test]
    fn ef_aut_02_the_json_form_uses_camel_case_and_fills_defaults() {
        let c: OAuth2 =
            serde_json::from_value(serde_json::json!({ "flow": "authorization_code", "clientId": "id" })).unwrap();
        assert_eq!((c.client_id.as_str(), c.token_prefix.as_str(), c.auto_fetch_token), ("id", "Bearer", true));
        assert_eq!(serde_json::to_value(&c).unwrap()["accessTokenUrl"], "");
    }
}
