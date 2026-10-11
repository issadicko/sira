//! OAuth 1.0 (RFC 5849) : la signature d'une requête et l'en-tête `Authorization: OAuth …` (ou les paramètres d'adresse, ou
//! du corps). Les jetons s'obtiennent hors de Sira : on signe avec ceux que la requête donne.

use base64::Engine as _;
use ring::hmac;
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};
use rsa::RsaPrivateKey;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha512};
use url::Url;

use crate::{EngineError, HttpRequest};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// La configuration d'une requête OAuth 1.0, variables résolues ; un champ vide est absent.
#[derive(Clone, PartialEq, Eq, Default)]
pub struct Oauth1 {
    pub consumer_key: String,
    pub consumer_secret: String,
    pub access_token: String,
    pub access_token_secret: String,
    pub callback_url: String,
    pub verifier: String,
    /// `HMAC-SHA1` (par défaut), `HMAC-SHA256`, `HMAC-SHA512`, `RSA-SHA1`, `RSA-SHA256`, `RSA-SHA512` ou `PLAINTEXT`.
    pub signature_method: String,
    /// La clé privée en PEM (PKCS#8 ou PKCS#1), pour les méthodes `RSA-*`.
    pub private_key: String,
    pub timestamp: String,
    pub nonce: String,
    pub version: String,
    pub realm: String,
    /// `header` (par défaut), `query` ou `body`.
    pub placement: String,
    pub include_body_hash: bool,
}

impl std::fmt::Debug for Oauth1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Oauth1")
            .field("consumer_key", &self.consumer_key)
            .field("signature_method", &self.signature_method)
            .field("placement", &self.placement)
            .finish_non_exhaustive()
    }
}

fn error(message: impl Into<String>) -> EngineError {
    EngineError::Http(format!("OAuth 1.0 : {}", message.into()))
}

const fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

/// L'encodage en pourcentage de la RFC 5849 §3.6 : seuls les caractères non réservés restent tels quels.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if unreserved(byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decode_pairs(text: &str) -> Vec<(String, String)> {
    url::form_urlencoded::parse(text.as_bytes()).map(|(k, v)| (k.into_owned(), v.into_owned())).collect()
}

/// L'URL de base de la RFC 5849 §3.4.1.2 : schéma et hôte en minuscules, port omis quand c'est celui du schéma, ni requête ni
/// fragment.
pub fn base_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let port = url.port().map(|p| format!(":{p}")).unwrap_or_default();
    format!("{}://{host}{port}{}", url.scheme().to_ascii_lowercase(), url.path())
}

/// Les paramètres normalisés (§3.4.1.3.2) : encodés, triés par nom puis par valeur, joints par `&`.
pub fn normalized(params: &[(String, String)]) -> String {
    let mut encoded: Vec<(String, String)> =
        params.iter().map(|(k, v)| (percent_encode(k), percent_encode(v))).collect();
    encoded.sort();
    encoded.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

pub fn base_string(method: &str, url: &Url, params: &[(String, String)]) -> String {
    format!(
        "{}&{}&{}",
        method.to_ascii_uppercase(),
        percent_encode(&base_url(url)),
        percent_encode(&normalized(params))
    )
}

fn method_of(config: &Oauth1) -> &str {
    if config.signature_method.trim().is_empty() {
        "HMAC-SHA1"
    } else {
        config.signature_method.trim()
    }
}

fn digest_of(method: &str, data: &[u8]) -> Result<Vec<u8>, EngineError> {
    match method {
        "HMAC-SHA1" | "RSA-SHA1" => Ok(Sha1::digest(data).to_vec()),
        "HMAC-SHA256" | "RSA-SHA256" => Ok(Sha256::digest(data).to_vec()),
        "HMAC-SHA512" | "RSA-SHA512" => Ok(Sha512::digest(data).to_vec()),
        other => Err(error(format!("méthode de signature inconnue : {other}"))),
    }
}

fn rsa_key(pem: &str) -> Result<RsaPrivateKey, EngineError> {
    if pem.trim().is_empty() {
        return Err(error("une méthode RSA demande une clé privée (PEM)"));
    }
    use rsa::pkcs1::DecodeRsaPrivateKey;
    RsaPrivateKey::from_pkcs8_pem(pem)
        .or_else(|_| RsaPrivateKey::from_pkcs1_pem(pem))
        .map_err(|_| error("clé privée illisible : attendu un PEM PKCS#8 ou PKCS#1 sans mot de passe"))
}

fn sign_base(config: &Oauth1, base: &str) -> Result<String, EngineError> {
    let key = format!("{}&{}", percent_encode(&config.consumer_secret), percent_encode(&config.access_token_secret));
    let method = method_of(config);
    let raw = match method {
        "PLAINTEXT" => return Ok(key),
        "HMAC-SHA1" => {
            hmac::sign(&hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key.as_bytes()), base.as_bytes())
                .as_ref()
                .to_vec()
        }
        "HMAC-SHA256" => {
            hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()), base.as_bytes()).as_ref().to_vec()
        }
        "HMAC-SHA512" => {
            hmac::sign(&hmac::Key::new(hmac::HMAC_SHA512, key.as_bytes()), base.as_bytes()).as_ref().to_vec()
        }
        "RSA-SHA1" => SigningKey::<Sha1>::new(rsa_key(&config.private_key)?).sign(base.as_bytes()).to_vec(),
        "RSA-SHA256" => SigningKey::<Sha256>::new(rsa_key(&config.private_key)?).sign(base.as_bytes()).to_vec(),
        "RSA-SHA512" => SigningKey::<Sha512>::new(rsa_key(&config.private_key)?).sign(base.as_bytes()).to_vec(),
        other => return Err(error(format!("méthode de signature inconnue : {other}"))),
    };
    Ok(B64.encode(raw))
}

fn header_value<'a>(request: &'a HttpRequest, name: &str) -> Option<&'a str> {
    request.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

fn is_form(request: &HttpRequest) -> bool {
    header_value(request, "content-type")
        .is_some_and(|t| t.to_ascii_lowercase().starts_with("application/x-www-form-urlencoded"))
}

/// Les paramètres `oauth_*` de la requête, sans la signature.
fn oauth_params(
    config: &Oauth1,
    request: &HttpRequest,
    now_secs: u64,
    nonce: &str,
) -> Result<Vec<(String, String)>, EngineError> {
    let pick = |own: &str, fallback: String| if own.trim().is_empty() { fallback } else { own.trim().to_owned() };
    let method = method_of(config);
    let mut params = Vec::new();
    let mut add = |name: &str, value: String| {
        if !value.is_empty() {
            params.push((name.to_owned(), value));
        }
    };
    add("oauth_consumer_key", config.consumer_key.trim().to_owned());
    add("oauth_token", config.access_token.trim().to_owned());
    add("oauth_signature_method", method.to_owned());
    add("oauth_timestamp", pick(&config.timestamp, now_secs.to_string()));
    add("oauth_nonce", pick(&config.nonce, nonce.to_owned()));
    add("oauth_version", pick(&config.version, "1.0".into()));
    add("oauth_callback", config.callback_url.trim().to_owned());
    add("oauth_verifier", config.verifier.trim().to_owned());
    if config.include_body_hash && method != "PLAINTEXT" && !is_form(request) {
        let body = request.body.as_deref().unwrap_or_default();
        add("oauth_body_hash", B64.encode(digest_of(method, body)?));
    }
    Ok(params)
}

/// Signe `request` : ajoute `Authorization: OAuth …`, ou les paramètres `oauth_*` à l'adresse (`query`) ou au corps `body`
/// (qui doit alors être un formulaire). Les paramètres de l'adresse et d'un corps de formulaire entrent dans la signature.
pub fn sign(request: &mut HttpRequest, config: &Oauth1, now_secs: u64, nonce: &str) -> Result<(), EngineError> {
    let mut url = Url::parse(&request.url).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    let oauth = oauth_params(config, request, now_secs, nonce)?;
    let mut signed: Vec<(String, String)> = url.query().map(decode_pairs).unwrap_or_default();
    if is_form(request) {
        signed.extend(request.body.as_deref().map(|b| decode_pairs(&String::from_utf8_lossy(b))).unwrap_or_default());
    }
    signed.extend(oauth.iter().cloned());
    let signature = sign_base(config, &base_string(&request.method, &url, &signed))?;
    let mut all = oauth;
    all.push(("oauth_signature".into(), signature));

    match config.placement.trim() {
        "" | "header" => {
            let mut parts: Vec<String> = Vec::new();
            if !config.realm.trim().is_empty() {
                parts.push(format!("realm=\"{}\"", config.realm.trim().replace('"', "")));
            }
            parts.extend(all.iter().map(|(k, v)| format!("{}=\"{}\"", percent_encode(k), percent_encode(v))));
            request.headers.retain(|(k, _)| !k.eq_ignore_ascii_case("authorization"));
            request.headers.push(("Authorization".into(), format!("OAuth {}", parts.join(", "))));
        }
        "query" => {
            let mut text = url.query().unwrap_or_default().to_owned();
            for (k, v) in &all {
                if !text.is_empty() {
                    text.push('&');
                }
                text.push_str(&format!("{}={}", percent_encode(k), percent_encode(v)));
            }
            url.set_query(Some(&text));
            request.url = url.to_string();
        }
        "body" => {
            if !is_form(request) {
                return Err(error(
                    "placer les paramètres dans le corps demande un corps application/x-www-form-urlencoded",
                ));
            }
            let mut text = String::from_utf8_lossy(request.body.as_deref().unwrap_or_default()).into_owned();
            for (k, v) in &all {
                if !text.is_empty() {
                    text.push('&');
                }
                text.push_str(&format!("{}={}", percent_encode(k), percent_encode(v)));
            }
            request.body = Some(text.into_bytes());
        }
        other => return Err(error(format!("emplacement inconnu : {other} (header, query ou body)"))),
    }
    Ok(())
}

/// Un nonce : 16 octets aléatoires en hexadécimal.
pub fn nonce() -> String {
    crate::digest::client_nonce()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::Network;

    fn request(method: &str, url: &str) -> HttpRequest {
        HttpRequest {
            method: method.into(),
            url: url.into(),
            headers: vec![],
            body: None,
            timeout: Duration::from_secs(5),
            max_response_body: None,
            network: Network::default(),
        }
    }

    fn core_spec_config() -> Oauth1 {
        Oauth1 {
            consumer_key: "dpf43f3p2l4k3l03".into(),
            consumer_secret: "kd94hf93k423kf44".into(),
            access_token: "nnch734d00sl2jdk".into(),
            access_token_secret: "pfkkdhi9sl3r4s00".into(),
            timestamp: "1191242096".into(),
            nonce: "kllo9940pd9333jh".into(),
            ..Oauth1::default()
        }
    }

    fn decoded(encoded: &str) -> Vec<u8> {
        let text = url::form_urlencoded::parse(format!("v={encoded}").as_bytes()).next().unwrap().1.into_owned();
        B64.decode(text).unwrap()
    }

    fn param(header: &str, name: &str) -> String {
        let at = header.find(&format!("{name}=\"")).unwrap() + name.len() + 2;
        header[at..].split('"').next().unwrap().to_owned()
    }

    #[test]
    fn ef_aut_03_the_hmac_sha1_signature_matches_the_oauth_core_1_0_example() {
        let mut req = request("GET", "http://photos.example.net/photos?file=vacation.jpg&size=original");

        sign(&mut req, &core_spec_config(), 0, "ignoré").unwrap();

        let header = &req.headers.iter().find(|(k, _)| k == "Authorization").unwrap().1;
        assert!(header.starts_with("OAuth "), "{header}");
        assert_eq!(param(header, "oauth_signature"), "tR3%2BTy81lMeYAr%2FFid0kMTYa%2FWM%3D");
        assert_eq!(param(header, "oauth_consumer_key"), "dpf43f3p2l4k3l03");
        assert_eq!(param(header, "oauth_version"), "1.0");
    }

    #[test]
    fn ef_aut_03_the_base_string_follows_rfc_5849_normalisation() {
        let url = Url::parse("HTTP://Example.COM:80/r%20v/X?id=123").unwrap();
        let params: Vec<(String, String)> =
            [("b5", "=%3D"), ("a3", "a"), ("c@", ""), ("a2", "r b"), ("oauth_nonce", "7d8f3e4a"), ("a3", "2 q")]
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect();

        assert_eq!(base_url(&url), "http://example.com/r%20v/X");
        assert_eq!(normalized(&params), "a2=r%20b&a3=2%20q&a3=a&b5=%3D%253D&c%40=&oauth_nonce=7d8f3e4a");
        assert_eq!(percent_encode("a b/é~-._*"), "a%20b%2F%C3%A9~-._%2A");
    }

    #[test]
    fn ef_aut_03_a_non_default_port_stays_in_the_base_url() {
        assert_eq!(base_url(&Url::parse("https://api.test:8443/x").unwrap()), "https://api.test:8443/x");
        assert_eq!(base_url(&Url::parse("https://api.test:443/x").unwrap()), "https://api.test/x");
    }

    fn signature_of(config: &Oauth1) -> String {
        let mut req = request("GET", "http://photos.example.net/photos?file=vacation.jpg");
        sign(&mut req, config, 0, "x").unwrap();
        let header = &req.headers.iter().find(|(k, _)| k == "Authorization").unwrap().1;
        param(header, "oauth_signature")
    }

    #[test]
    fn ef_aut_03_hmac_sha256_and_sha512_sign_the_same_base_string_with_the_same_key() {
        let base = {
            let config = Oauth1 { signature_method: "HMAC-SHA256".into(), ..core_spec_config() };
            let url = Url::parse("http://photos.example.net/photos?file=vacation.jpg").unwrap();
            let mut params = vec![("file".to_owned(), "vacation.jpg".to_owned())];
            for (k, v) in oauth_params(&config, &request("GET", url.as_str()), 0, "x").unwrap() {
                params.push((k, v));
            }
            (base_string("GET", &url, &params), config)
        };
        let key = "kd94hf93k423kf44&pfkkdhi9sl3r4s00";
        let expected256 =
            B64.encode(hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()), base.0.as_bytes()).as_ref());
        assert_eq!(percent_encode(&expected256), signature_of(&base.1));
        let config512 = Oauth1 { signature_method: "HMAC-SHA512".into(), ..core_spec_config() };
        let sig512 = signature_of(&config512);
        assert_eq!(decoded(&sig512).len(), 64);
        assert_ne!(sig512, signature_of(&Oauth1 { signature_method: "HMAC-SHA256".into(), ..core_spec_config() }));
    }

    #[test]
    fn ef_aut_03_plaintext_is_the_encoded_secrets_joined_by_an_ampersand() {
        let config = Oauth1 {
            signature_method: "PLAINTEXT".into(),
            consumer_secret: "a b".into(),
            access_token_secret: "c&d".into(),
            ..core_spec_config()
        };
        assert_eq!(signature_of(&config), percent_encode("a%20b&c%26d"));
    }

    fn verifies(public_pem: &str, base: &str, signature_b64: &str, sha: &str) -> bool {
        use rsa::pkcs1v15::{Signature, VerifyingKey};
        use rsa::pkcs8::DecodePublicKey;
        use rsa::signature::Verifier;
        let key = rsa::RsaPublicKey::from_public_key_pem(public_pem).unwrap();
        let signature = Signature::try_from(B64.decode(signature_b64).unwrap().as_slice()).unwrap();
        match sha {
            "1" => VerifyingKey::<Sha1>::new(key).verify(base.as_bytes(), &signature).is_ok(),
            "256" => VerifyingKey::<Sha256>::new(key).verify(base.as_bytes(), &signature).is_ok(),
            _ => VerifyingKey::<Sha512>::new(key).verify(base.as_bytes(), &signature).is_ok(),
        }
    }

    #[test]
    fn ef_aut_03_rsa_signatures_verify_with_the_public_key_for_pkcs8_and_pkcs1_keys() {
        let public = include_str!("../tests/fixtures/oauth1-public.pem");
        for (pem, label) in [
            (include_str!("../tests/fixtures/oauth1-pkcs8.pem"), "PKCS#8"),
            (include_str!("../tests/fixtures/oauth1-pkcs1.pem"), "PKCS#1"),
        ] {
            for (method, sha) in [("RSA-SHA1", "1"), ("RSA-SHA256", "256"), ("RSA-SHA512", "512")] {
                let config = Oauth1 { signature_method: method.into(), private_key: pem.into(), ..core_spec_config() };
                let mut req = request("GET", "http://photos.example.net/photos?file=vacation.jpg");
                sign(&mut req, &config, 0, "x").unwrap();
                let header = &req.headers.iter().find(|(k, _)| k == "Authorization").unwrap().1;
                let signature =
                    url::form_urlencoded::parse(format!("v={}", param(header, "oauth_signature")).as_bytes())
                        .next()
                        .unwrap()
                        .1
                        .into_owned();
                let url = Url::parse("http://photos.example.net/photos?file=vacation.jpg").unwrap();
                let mut params = vec![("file".to_owned(), "vacation.jpg".to_owned())];
                params.extend(oauth_params(&config, &req, 0, "x").unwrap());
                assert!(verifies(public, &base_string("GET", &url, &params), &signature, sha), "{label} {method}");
            }
        }
    }

    #[test]
    fn ef_aut_03_an_rsa_method_without_a_readable_key_says_so() {
        let missing = Oauth1 { signature_method: "RSA-SHA1".into(), ..core_spec_config() };
        assert!(sign(&mut request("GET", "http://x.test/"), &missing, 0, "n")
            .unwrap_err()
            .to_string()
            .contains("clé privée"));
        let bad =
            Oauth1 { signature_method: "RSA-SHA1".into(), private_key: "pas un PEM".into(), ..core_spec_config() };
        assert!(sign(&mut request("GET", "http://x.test/"), &bad, 0, "n")
            .unwrap_err()
            .to_string()
            .contains("illisible"));
        let unknown = Oauth1 { signature_method: "MD5".into(), ..core_spec_config() };
        assert!(sign(&mut request("GET", "http://x.test/"), &unknown, 0, "n")
            .unwrap_err()
            .to_string()
            .contains("inconnue"));
    }

    #[test]
    fn ef_aut_03_missing_timestamp_and_nonce_are_filled_and_empty_optional_fields_are_left_out() {
        let config = Oauth1 { consumer_key: "ck".into(), consumer_secret: "cs".into(), ..Oauth1::default() };
        let mut req = request("GET", "http://x.test/");

        sign(&mut req, &config, 1_700_000_000, "abc123").unwrap();

        let header = &req.headers[0].1;
        assert_eq!(param(header, "oauth_timestamp"), "1700000000");
        assert_eq!(param(header, "oauth_nonce"), "abc123");
        assert_eq!(param(header, "oauth_signature_method"), "HMAC-SHA1");
        assert!(
            !header.contains("oauth_token")
                && !header.contains("oauth_callback")
                && !header.contains("oauth_verifier")
                && !header.contains("realm"),
            "{header}"
        );
    }

    #[test]
    fn ef_aut_03_callback_verifier_and_realm_go_in_the_header_with_the_realm_out_of_the_signature() {
        let config =
            Oauth1 { callback_url: "oob".into(), verifier: "v-1".into(), realm: "Photos".into(), ..core_spec_config() };
        let mut with_realm = request("GET", "http://x.test/");
        sign(&mut with_realm, &config, 0, "n").unwrap();
        let mut without = request("GET", "http://x.test/");
        sign(&mut without, &Oauth1 { realm: String::new(), ..config.clone() }, 0, "n").unwrap();

        let header = &with_realm.headers[0].1;
        assert!(header.starts_with("OAuth realm=\"Photos\", "), "{header}");
        assert_eq!(
            (param(header, "oauth_callback"), param(header, "oauth_verifier")),
            ("oob".to_owned(), "v-1".to_owned())
        );
        assert_eq!(param(header, "oauth_signature"), param(&without.headers[0].1, "oauth_signature"));
    }

    #[test]
    fn ef_aut_03_an_existing_authorization_header_is_replaced() {
        let mut req = request("GET", "http://x.test/");
        req.headers.push(("authorization".into(), "Bearer vieux".into()));

        sign(&mut req, &core_spec_config(), 0, "n").unwrap();

        assert_eq!(req.headers.iter().filter(|(k, _)| k.eq_ignore_ascii_case("authorization")).count(), 1);
        assert!(req.headers[0].1.starts_with("OAuth "));
    }

    #[test]
    fn ef_aut_03_query_parameters_and_a_form_body_are_part_of_the_signature() {
        let signed = |url: &str, body: Option<&str>, ctype: &str| {
            let mut req = request("POST", url);
            req.headers.push(("Content-Type".into(), ctype.into()));
            req.body = body.map(|b| b.as_bytes().to_vec());
            sign(&mut req, &core_spec_config(), 0, "n").unwrap();
            param(&req.headers.iter().find(|(k, _)| k == "Authorization").unwrap().1, "oauth_signature")
        };
        let form = "application/x-www-form-urlencoded";
        let plain = signed("http://x.test/p", None, form);
        assert_ne!(plain, signed("http://x.test/p?a=1", None, form));
        assert_ne!(plain, signed("http://x.test/p", Some("a=1&b=2"), form));
        assert_eq!(
            signed("http://x.test/p?a=1&b=2", None, form),
            signed("http://x.test/p?b=2", Some("a=1"), form),
            "l'ordre et la provenance ne comptent pas"
        );
        assert_eq!(
            plain,
            signed("http://x.test/p", Some("a=1"), "application/json"),
            "un corps JSON n'entre pas dans la signature"
        );
    }

    #[test]
    fn ef_aut_03_the_query_placement_adds_the_oauth_parameters_to_the_url() {
        let config = Oauth1 { placement: "query".into(), ..core_spec_config() };
        let mut req = request("GET", "http://x.test/p?a=1");

        sign(&mut req, &config, 0, "n").unwrap();

        assert!(req.headers.is_empty());
        let url = Url::parse(&req.url).unwrap();
        let names: Vec<String> = url.query_pairs().map(|(k, _)| k.into_owned()).collect();
        assert_eq!(names[0], "a");
        assert!(names.contains(&"oauth_signature".to_owned()) && names.contains(&"oauth_consumer_key".to_owned()));
        let signature = url.query_pairs().find(|(k, _)| k == "oauth_signature").unwrap().1.into_owned();
        assert!(!signature.is_empty() && signature.ends_with('='), "{signature}");
    }

    #[test]
    fn ef_aut_03_the_body_placement_appends_to_a_form_body_and_refuses_anything_else() {
        let config = Oauth1 { placement: "body".into(), ..core_spec_config() };
        let mut form = request("POST", "http://x.test/p");
        form.headers.push(("Content-Type".into(), "application/x-www-form-urlencoded".into()));
        form.body = Some(b"a=1".to_vec());

        sign(&mut form, &config, 0, "n").unwrap();

        let body = String::from_utf8(form.body.unwrap()).unwrap();
        assert!(body.starts_with("a=1&oauth_consumer_key=") && body.contains("&oauth_signature="), "{body}");
        let mut json = request("POST", "http://x.test/p");
        json.headers.push(("Content-Type".into(), "application/json".into()));
        assert!(sign(&mut json, &config, 0, "n")
            .unwrap_err()
            .to_string()
            .contains("application/x-www-form-urlencoded"));
        let bad = Oauth1 { placement: "cookie".into(), ..core_spec_config() };
        assert!(sign(&mut request("GET", "http://x.test/"), &bad, 0, "n")
            .unwrap_err()
            .to_string()
            .contains("emplacement inconnu"));
    }

    #[test]
    fn ef_aut_03_the_body_hash_is_added_for_a_non_form_body_and_matches_the_method_family() {
        let config = Oauth1 { include_body_hash: true, ..core_spec_config() };
        let hash_of = |config: &Oauth1, ctype: &str, body: &str| {
            let mut req = request("POST", "http://x.test/p");
            req.headers.push(("Content-Type".into(), ctype.into()));
            req.body = Some(body.as_bytes().to_vec());
            sign(&mut req, config, 0, "n").unwrap();
            let header = req.headers.iter().find(|(k, _)| k == "Authorization").unwrap().1.clone();
            header.contains("oauth_body_hash").then(|| param(&header, "oauth_body_hash"))
        };
        assert_eq!(hash_of(&config, "application/json", ""), Some("2jmj7l5rSw0yVb%2FvlWAYkK%2FYBwk%3D".into()));
        assert_eq!(hash_of(&config, "application/x-www-form-urlencoded", "a=1"), None);
        assert_eq!(hash_of(&Oauth1 { include_body_hash: false, ..config.clone() }, "application/json", "{}"), None);
        let sha256 = Oauth1 { signature_method: "HMAC-SHA256".into(), ..config };
        assert_eq!(decoded(&hash_of(&sha256, "application/json", "").unwrap()).len(), 32);
    }

    #[test]
    fn ef_aut_03_the_debug_output_hides_the_secrets() {
        let shown = format!(
            "{:?}",
            Oauth1 {
                consumer_secret: "très-secret".into(),
                access_token_secret: "aussi-secret".into(),
                ..core_spec_config()
            }
        );
        assert!(!shown.contains("secret"), "{shown}");
    }
}
