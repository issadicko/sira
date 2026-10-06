//! L'auth au moment de l'envoi : signature AWS, défi Digest. Ce que `prepare` ne peut pas faire seul, parce que cela
//! dépend de la requête finale ou de la réponse du serveur.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use url::Url;
use xc_core::{AwsSettings, SendAuth};
use xc_engine::aws::{self, AwsCredentials};
use xc_engine::digest;
use xc_engine::{HttpRequest, HttpResponse};

use crate::oauth2;
use crate::session::Session;

/// La réponse, et les en-têtes de la requête telle qu'elle est partie (signature et défi compris).
pub(crate) struct Sent {
    pub response: HttpResponse,
    pub headers: Vec<(String, String)>,
}

pub(crate) async fn send(mut request: HttpRequest, auth: &SendAuth, session: &mut Session) -> Result<Sent, String> {
    match auth {
        SendAuth::None => {}
        SendAuth::Aws(settings) => sign(&mut request, settings)?,
        SendAuth::Digest { username, password } => return digest_send(request, username, password).await,
        SendAuth::Oauth2(config) => {
            let token = oauth2::token_for(config, &mut session.tokens, session.authorizer.as_ref()).await?;
            if let Some(token) = token {
                oauth2::apply(&mut request, config, &token)?;
            }
        }
    }
    let headers = request.headers.clone();
    let response = xc_engine::send(request).await.map_err(|e| e.to_string())?;
    Ok(Sent { response, headers })
}

fn now_millis() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
}

fn sign(request: &mut HttpRequest, settings: &AwsSettings) -> Result<(), String> {
    let mut creds = AwsCredentials {
        access_key_id: settings.access_key_id.clone(),
        secret_access_key: settings.secret_access_key.clone(),
        session_token: Some(settings.session_token.clone()).filter(|t| !t.is_empty()),
        region: settings.region.clone(),
        service: settings.service.clone(),
    };
    if !settings.profile_name.is_empty() {
        let profile = read_profile(&settings.profile_name).ok_or_else(|| {
            format!("profil AWS « {} » introuvable dans ~/.aws/credentials ni ~/.aws/config", settings.profile_name)
        })?;
        creds.access_key_id = profile.access_key_id;
        creds.secret_access_key = profile.secret_access_key;
        creds.session_token = profile.session_token;
    }
    if creds.access_key_id.is_empty() || creds.secret_access_key.is_empty() {
        return Err("AWS Signature V4 : l'identifiant et la clé secrète d'accès sont requis".into());
    }
    aws::sign(request, &creds, now_millis()).map_err(|e| e.to_string())
}

/// Envoie sans identifiants ; sur un 401 qui porte un défi Digest, renvoie une fois avec la réponse au défi.
async fn digest_send(request: HttpRequest, username: &str, password: &str) -> Result<Sent, String> {
    let first = xc_engine::send(request.clone()).await.map_err(|e| e.to_string())?;
    let challenge = (first.status == 401)
        .then(|| {
            first
                .headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("www-authenticate"))
                .find_map(|(_, v)| digest::parse_challenge(v))
        })
        .flatten();
    let Some(challenge) = challenge else { return Ok(Sent { response: first, headers: request.headers }) };
    let url = Url::parse(&request.url).map_err(|e| e.to_string())?;
    let uri = url.query().map_or_else(|| url.path().to_owned(), |q| format!("{}?{q}", url.path()));
    let header =
        digest::authorization(&challenge, username, password, &request.method, &uri, 1, &digest::client_nonce());
    let mut retry = request;
    retry.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
    retry.headers.push(("Authorization".into(), header));
    let headers = retry.headers.clone();
    let response = xc_engine::send(retry).await.map_err(|e| e.to_string())?;
    Ok(Sent { response, headers })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Profile {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Les clés d'une section `[name]` d'un fichier INI d'AWS (`[profile name]` accepté dans `config`).
fn parse_profile(text: &str, name: &str) -> Option<Profile> {
    let mut inside = false;
    let (mut id, mut secret, mut token) = (None, None, None);
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with(['#', ';'])) {
        if let Some(section) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let section = section.trim();
            inside = section == name || section.strip_prefix("profile").is_some_and(|rest| rest.trim() == name);
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().to_owned();
            match key.trim() {
                "aws_access_key_id" => id = Some(value),
                "aws_secret_access_key" => secret = Some(value),
                "aws_session_token" => token = Some(value),
                _ => {}
            }
        }
    }
    Some(Profile { access_key_id: id?, secret_access_key: secret?, session_token: token })
}

fn read_profile(name: &str) -> Option<Profile> {
    let aws = home()?.join(".aws");
    let credentials =
        std::env::var_os("AWS_SHARED_CREDENTIALS_FILE").map_or_else(|| aws.join("credentials"), PathBuf::from);
    let config = std::env::var_os("AWS_CONFIG_FILE").map_or_else(|| aws.join("config"), PathBuf::from);
    [credentials, config]
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .find_map(|text| parse_profile(&text, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "# commentaire\n[default]\naws_access_key_id = AKDEFAULT\naws_secret_access_key = sdefault\n\n[profile dev]\naws_access_key_id=AKDEV\naws_secret_access_key=sdev\naws_session_token = tdev\n\n[other]\nregion = eu-west-1\n";

    #[test]
    fn ef_aut_01_an_aws_profile_is_read_from_credentials_or_config_sections() {
        let dev = parse_profile(INI, "dev").unwrap();
        assert_eq!(
            (dev.access_key_id.as_str(), dev.secret_access_key.as_str(), dev.session_token.as_deref()),
            ("AKDEV", "sdev", Some("tdev"))
        );
        assert_eq!(parse_profile(INI, "default").unwrap().session_token, None);
        assert_eq!(
            parse_profile("[ci]\naws_access_key_id = A\naws_secret_access_key = S\n", "ci").unwrap().access_key_id,
            "A"
        );
    }

    #[test]
    fn ef_aut_01_a_missing_or_incomplete_aws_profile_is_not_found() {
        assert_eq!(parse_profile(INI, "prod"), None);
        assert_eq!(parse_profile(INI, "other"), None, "sans clés d'accès");
    }
}
