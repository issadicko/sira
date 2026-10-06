//! Signature AWS Signature Version 4 d'une requête HTTP, comme le fait la bibliothèque `aws4` dont Bruno se sert.

use std::collections::BTreeMap;

use ring::{digest, hmac};
use url::Url;

use crate::time::amz_date;
use crate::{EngineError, HttpRequest};

const ALGORITHM: &str = "AWS4-HMAC-SHA256";

/// Les en-têtes que la signature ignore, comme `aws4`.
const UNSIGNED: &[&str] = &[
    "authorization",
    "connection",
    "x-amzn-trace-id",
    "user-agent",
    "expect",
    "presigned-expires",
    "range",
    "content-length",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AwsCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: Option<String>,
    /// Vide : déduite de l'hôte (`s3.eu-west-1.amazonaws.com`), comme `aws4`.
    pub region: String,
    /// Vide : déduit de l'hôte.
    pub service: String,
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256(data: &[u8]) -> Vec<u8> {
    digest::digest(&digest::SHA256, data).as_ref().to_vec()
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex(&sha256(data))
}

fn hmac_sha256(key: &[u8], data: &str) -> Vec<u8> {
    hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data.as_bytes()).as_ref().to_vec()
}

/// Encodage des URI de la signature : tout sauf `A-Za-z0-9-_.~` est encodé, sur l'octet.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(char::from(b)),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Région et service déduits d'un hôte `amazonaws.com` ; `None` pour tout autre hôte.
fn infer(host: &str) -> Option<(String, String)> {
    let name =
        host.rsplit_once(':').filter(|(_, port)| port.chars().all(|c| c.is_ascii_digit())).map_or(host, |(h, _)| h);
    let labels: Vec<&str> = name.trim_end_matches(".cn").strip_suffix(".amazonaws.com")?.split('.').collect();
    let is_region =
        |l: &str| l.len() > 6 && l.split('-').count() >= 3 && l.chars().last().is_some_and(|c| c.is_ascii_digit());
    match labels.as_slice() {
        [.., service, region] if is_region(region) => Some((region.to_string(), service.to_string())),
        [.., service] => Some(("us-east-1".into(), service.to_string())),
        [] => None,
    }
}

fn canonical_uri(url: &Url, s3: bool) -> String {
    let raw = url.path();
    if s3 {
        return if raw.is_empty() { "/".into() } else { raw.to_owned() };
    }
    let segments: Vec<&str> = raw.split('/').collect();
    let last = segments.len().saturating_sub(1);
    let kept: Vec<String> = segments
        .iter()
        .enumerate()
        .filter(|(i, s)| !s.is_empty() || *i == 0 || *i == last)
        .map(|(_, s)| encode(s))
        .collect();
    let joined = kept.join("/");
    if joined.is_empty() {
        "/".into()
    } else {
        joined
    }
}

fn canonical_query(url: &Url) -> String {
    let mut pairs: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (encode(&k), encode(&v))).collect();
    pairs.sort();
    pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

fn collapse(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn set(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
    headers.push((name.to_owned(), value));
}

/// Signe `request` pour l'instant `at_millis` (millisecondes Unix) : ajoute `X-Amz-Date`, `X-Amz-Security-Token` si
/// besoin, `X-Amz-Content-Sha256` pour S3, et `Authorization`. Tout autre en-tête déjà posé est signé aussi.
pub fn sign(request: &mut HttpRequest, creds: &AwsCredentials, at_millis: u64) -> Result<(), EngineError> {
    let url = Url::parse(&request.url).map_err(|e| EngineError::InvalidUrl(e.to_string()))?;
    let host = match (url.host_str(), url.port()) {
        (Some(h), Some(p)) => format!("{h}:{p}"),
        (Some(h), None) => h.to_owned(),
        (None, _) => return Err(EngineError::InvalidUrl("hôte manquant".into())),
    };
    let inferred = infer(&host);
    let service = if creds.service.is_empty() {
        inferred.as_ref().map(|(_, s)| s.clone()).unwrap_or_default()
    } else {
        creds.service.clone()
    };
    let region = if creds.region.is_empty() {
        inferred.as_ref().map(|(r, _)| r.clone()).unwrap_or_else(|| "us-east-1".into())
    } else {
        creds.region.clone()
    };
    let s3 = service == "s3";
    let stamp = amz_date(at_millis);
    let (date, _) = stamp.split_at(8);
    let payload = request.body.as_deref().unwrap_or_default();
    let payload_hash = sha256_hex(payload);

    set(&mut request.headers, "X-Amz-Date", stamp.clone());
    if let Some(token) = creds.session_token.as_ref().filter(|t| !t.is_empty()) {
        set(&mut request.headers, "X-Amz-Security-Token", token.clone());
    }
    if s3 && !request.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("x-amz-content-sha256")) {
        set(&mut request.headers, "X-Amz-Content-Sha256", payload_hash.clone());
    }

    let mut signed: BTreeMap<String, String> = BTreeMap::new();
    signed.insert("host".into(), host);
    for (name, value) in &request.headers {
        let lower = name.to_ascii_lowercase();
        if lower != "host" && !UNSIGNED.contains(&lower.as_str()) {
            signed.insert(lower, collapse(value));
        }
    }
    let names = signed.keys().cloned().collect::<Vec<_>>().join(";");
    let headers: String = signed.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let content_hash = request
        .headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("x-amz-content-sha256"))
        .map_or(payload_hash, |(_, v)| v.clone());
    let canonical = format!(
        "{}\n{}\n{}\n{headers}\n{names}\n{content_hash}",
        request.method.to_ascii_uppercase(),
        canonical_uri(&url, s3),
        canonical_query(&url)
    );

    let scope = format!("{date}/{region}/{service}/aws4_request");
    let to_sign = format!("{ALGORITHM}\n{stamp}\n{scope}\n{}", sha256_hex(canonical.as_bytes()));
    let key = ["AWS4", creds.secret_access_key.as_str()].concat();
    let key = [date, region.as_str(), service.as_str(), "aws4_request"]
        .iter()
        .fold(key.into_bytes(), |key, part| hmac_sha256(&key, part));
    let signature = hex(&hmac_sha256(&key, &to_sign));
    set(
        &mut request.headers,
        "Authorization",
        format!("{ALGORITHM} Credential={}/{scope}, SignedHeaders={names}, Signature={signature}", creds.access_key_id),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::time::millis;

    fn creds(region: &str, service: &str) -> AwsCredentials {
        AwsCredentials {
            access_key_id: "AKIDEXAMPLE".into(),
            secret_access_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into(),
            session_token: None,
            region: region.into(),
            service: service.into(),
        }
    }

    fn request(method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> HttpRequest {
        HttpRequest {
            method: method.into(),
            url: url.into(),
            headers: headers.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
            body: (!body.is_empty()).then(|| body.as_bytes().to_vec()),
            timeout: Duration::from_secs(1),
            max_response_body: None,
            network: crate::Network::default(),
        }
    }

    fn header(request: &HttpRequest, name: &str) -> String {
        request.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.clone()).unwrap_or_default()
    }

    fn at() -> u64 {
        millis(2015, 8, 30, 12, 36, 0)
    }

    #[test]
    fn ef_aut_01_aws_signs_the_get_vanilla_vector_of_the_official_test_suite() {
        let mut r = request("GET", "https://example.amazonaws.com/", &[], "");
        sign(&mut r, &creds("us-east-1", "service"), at()).unwrap();
        assert_eq!(
            header(&r, "authorization"),
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
        assert_eq!(header(&r, "x-amz-date"), "20150830T123600Z");
    }

    #[test]
    fn ef_aut_01_aws_signs_the_post_vanilla_and_the_query_order_vectors() {
        let mut post = request("POST", "https://example.amazonaws.com/", &[], "");
        sign(&mut post, &creds("us-east-1", "service"), at()).unwrap();
        assert!(
            header(&post, "authorization")
                .ends_with("Signature=5da7c1a2acd57cee7505fc6676e4e544621c30862966e37dddb68e92efbe5d6b"),
            "{}",
            header(&post, "authorization")
        );

        let mut query = request("GET", "https://example.amazonaws.com/?Param2=value2&Param1=value1", &[], "");
        sign(&mut query, &creds("us-east-1", "service"), at()).unwrap();
        assert!(
            header(&query, "authorization")
                .ends_with("Signature=b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500"),
            "{}",
            header(&query, "authorization")
        );
    }

    #[test]
    fn ef_aut_01_aws_signs_the_headers_it_finds_and_the_session_token() {
        let mut r = request(
            "POST",
            "https://example.amazonaws.com/",
            &[("Content-Type", "application/json"), ("X-Custom", "  a   b ")],
            "{}",
        );
        let mut with_token = creds("us-east-1", "service");
        with_token.session_token = Some("TOKEN".into());
        sign(&mut r, &with_token, at()).unwrap();
        let auth = header(&r, "authorization");
        assert!(auth.contains("SignedHeaders=content-type;host;x-amz-date;x-amz-security-token;x-custom,"), "{auth}");
        assert_eq!(header(&r, "x-amz-security-token"), "TOKEN");
    }

    #[test]
    fn ef_aut_01_aws_s3_signs_the_payload_hash_and_does_not_double_encode_the_path() {
        let mut r = request("PUT", "https://bucket.s3.eu-west-1.amazonaws.com/a%20b/c.txt", &[], "hello");
        sign(&mut r, &creds("", ""), at()).unwrap();
        let auth = header(&r, "authorization");
        assert!(auth.contains("/eu-west-1/s3/aws4_request"), "{auth}");
        assert!(auth.contains("SignedHeaders=host;x-amz-content-sha256;x-amz-date,"), "{auth}");
        assert_eq!(header(&r, "x-amz-content-sha256"), sha256_hex(b"hello"));
        let url = Url::parse("https://b.s3.amazonaws.com/a%20b/c.txt").unwrap();
        assert_eq!(canonical_uri(&url, true), "/a%20b/c.txt");
        assert_eq!(canonical_uri(&url, false), "/a%2520b/c.txt");
    }

    #[test]
    fn ef_aut_01_aws_infers_region_and_service_from_the_host() {
        assert_eq!(infer("s3.amazonaws.com"), Some(("us-east-1".into(), "s3".into())));
        assert_eq!(infer("iam.amazonaws.com"), Some(("us-east-1".into(), "iam".into())));
        assert_eq!(infer("bucket.s3.us-west-2.amazonaws.com"), Some(("us-west-2".into(), "s3".into())));
        assert_eq!(infer("dynamodb.eu-central-1.amazonaws.com:443"), Some(("eu-central-1".into(), "dynamodb".into())));
        assert_eq!(infer("example.com"), None);
    }

    #[test]
    fn ef_aut_01_aws_normalizes_paths_and_sorts_the_query() {
        let url = Url::parse("https://example.amazonaws.com//a//b/").unwrap();
        assert_eq!(canonical_uri(&url, false), "/a/b/");
        let url = Url::parse("https://example.amazonaws.com/?b=2&a=1&a=0&c=x+y").unwrap();
        assert_eq!(canonical_query(&url), "a=0&a=1&b=2&c=x%20y");
        assert_eq!(canonical_uri(&Url::parse("https://example.amazonaws.com").unwrap(), false), "/");
    }

    #[test]
    fn ef_aut_01_aws_refuses_a_url_without_host() {
        let mut r = request("GET", "not a url", &[], "");
        assert!(sign(&mut r, &creds("r", "s"), 0).is_err());
    }
}
