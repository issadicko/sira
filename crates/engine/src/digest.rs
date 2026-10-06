//! Authentification HTTP Digest (RFC 7616, avec la variante MD5 de la RFC 2617) : analyse du défi du serveur et calcul de
//! l'en-tête `Authorization`.

use ring::digest::{self, Algorithm as Hash};

use crate::aws::hex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Md5,
    Md5Sess,
    Sha256,
    Sha256Sess,
}

impl Algorithm {
    fn parse(name: &str) -> Option<Self> {
        match name.to_ascii_uppercase().as_str() {
            "MD5" => Some(Self::Md5),
            "MD5-SESS" => Some(Self::Md5Sess),
            "SHA-256" => Some(Self::Sha256),
            "SHA-256-SESS" => Some(Self::Sha256Sess),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Md5 => "MD5",
            Self::Md5Sess => "MD5-sess",
            Self::Sha256 => "SHA-256",
            Self::Sha256Sess => "SHA-256-sess",
        }
    }

    fn session(self) -> bool {
        matches!(self, Self::Md5Sess | Self::Sha256Sess)
    }

    fn hash(self, data: &str) -> String {
        match self {
            Self::Md5 | Self::Md5Sess => format!("{:x}", md5::compute(data)),
            Self::Sha256 | Self::Sha256Sess => {
                const SHA256: &Hash = &digest::SHA256;
                hex(digest::digest(SHA256, data.as_bytes()).as_ref())
            }
        }
    }
}

/// Le défi d'un `WWW-Authenticate: Digest …`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub realm: String,
    pub nonce: String,
    pub opaque: Option<String>,
    pub algorithm: Algorithm,
    /// `auth` quand le serveur l'offre (`auth-int` n'est pas pris en charge) : le calcul ajoute alors `nc` et `cnonce`.
    pub qop_auth: bool,
}

/// Les paramètres `clé=valeur` ou `clé="valeur"` de `text`, séparés par des virgules.
fn params(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text.trim_start_matches([' ', ',']);
    while let Some((key, after)) = rest.split_once('=') {
        let key = key.trim().to_ascii_lowercase();
        let after = after.trim_start();
        let (value, tail) = if let Some(quoted) = after.strip_prefix('"') {
            let mut value = String::new();
            let mut chars = quoted.char_indices();
            let mut end = quoted.len();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => value.extend(chars.next().map(|(_, escaped)| escaped)),
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    c => value.push(c),
                }
            }
            (value, &quoted[end..])
        } else {
            let end = after.find(',').unwrap_or(after.len());
            (after[..end].trim().to_owned(), &after[end..])
        };
        out.push((key, value));
        rest = tail.trim_start_matches([' ', ',']);
    }
    out
}

/// Le premier défi Digest d'un `WWW-Authenticate` (qui peut en porter plusieurs) ; `None` s'il n'y en a pas, ou si son
/// algorithme n'est pas pris en charge.
pub fn parse_challenge(header: &str) -> Option<Challenge> {
    let start = header.to_ascii_lowercase().find("digest ")?;
    let pairs = params(&header[start + "digest ".len()..]);
    let get = |name: &str| pairs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone());
    let algorithm = match get("algorithm") {
        Some(name) => Algorithm::parse(&name)?,
        None => Algorithm::Md5,
    };
    let qop_auth = get("qop").is_some_and(|q| q.split(',').any(|o| o.trim().eq_ignore_ascii_case("auth")));
    Some(Challenge {
        realm: get("realm").unwrap_or_default(),
        nonce: get("nonce")?,
        opaque: get("opaque"),
        algorithm,
        qop_auth,
    })
}

fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// L'en-tête `Authorization` qui répond à `challenge` pour `method` et `uri` (chemin et requête). `nc` compte les
/// réponses faites avec ce `nonce`, `cnonce` est le nonce du client.
pub fn authorization(
    challenge: &Challenge,
    username: &str,
    password: &str,
    method: &str,
    uri: &str,
    nc: u32,
    cnonce: &str,
) -> String {
    let algorithm = challenge.algorithm;
    let mut ha1 = algorithm.hash(&format!("{username}:{}:{password}", challenge.realm));
    if algorithm.session() {
        ha1 = algorithm.hash(&format!("{ha1}:{}:{cnonce}", challenge.nonce));
    }
    let ha2 = algorithm.hash(&format!("{}:{uri}", method.to_ascii_uppercase()));
    let nc = format!("{nc:08x}");
    let response = if challenge.qop_auth {
        algorithm.hash(&format!("{ha1}:{}:{nc}:{cnonce}:auth:{ha2}", challenge.nonce))
    } else {
        algorithm.hash(&format!("{ha1}:{}:{ha2}", challenge.nonce))
    };
    let mut fields = vec![
        format!("username={}", quote(username)),
        format!("realm={}", quote(&challenge.realm)),
        format!("nonce={}", quote(&challenge.nonce)),
        format!("uri={}", quote(uri)),
        format!("algorithm={}", algorithm.name()),
        format!("response={}", quote(&response)),
    ];
    if let Some(opaque) = &challenge.opaque {
        fields.push(format!("opaque={}", quote(opaque)));
    }
    if challenge.qop_auth {
        fields.push("qop=auth".into());
        fields.push(format!("nc={nc}"));
        fields.push(format!("cnonce={}", quote(cnonce)));
    }
    format!("Digest {}", fields.join(", "))
}

/// Un nonce de client : 16 octets aléatoires en hexadécimal.
pub fn client_nonce() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    ring::rand::SystemRandom::new().fill(&mut bytes).map_or_else(|_| "0".repeat(32), |()| hex(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ef_aut_01_digest_answers_the_rfc_2617_example() {
        let challenge = parse_challenge(
            r#"Digest realm="testrealm@host.com", qop="auth,auth-int", nonce="dcd98b7102dd2f0e8b11d0f600bfb0c093", opaque="5ccc069c403ebaf9f0171e9517f40e41""#,
        )
        .unwrap();
        let header = authorization(&challenge, "Mufasa", "Circle Of Life", "GET", "/dir/index.html", 1, "0a4f113b");
        assert!(header.contains(r#"response="6629fae49393a05397450978507c4ef1""#), "{header}");
        assert!(header.contains("qop=auth, nc=00000001, cnonce=\"0a4f113b\""), "{header}");
        assert!(header.contains(r#"opaque="5ccc069c403ebaf9f0171e9517f40e41""#), "{header}");
        assert!(header.starts_with(r#"Digest username="Mufasa", realm="testrealm@host.com""#), "{header}");
    }

    #[test]
    fn ef_aut_01_digest_answers_the_rfc_7616_md5_and_sha256_examples() {
        let header = |algorithm: &str| {
            format!(
                r#"Digest realm="http-auth@example.org", qop="auth, auth-int", algorithm={algorithm}, nonce="7ypf/xlj9XXwfDPEoM4URrv/xwf94BcCAzFZH4GiTo0v", opaque="FQhe/qaU925kfnzjCev0ciny7QMkPqMAFRtzCUYo5tdS""#
            )
        };
        let cnonce = "f2/wE4q74E6zIJEtWaHKaf5wv/H5QzzpXusqGemxURZJ";
        let md5 = authorization(
            &parse_challenge(&header("MD5")).unwrap(),
            "Mufasa",
            "Circle of Life",
            "GET",
            "/dir/index.html",
            1,
            cnonce,
        );
        assert!(md5.contains(r#"response="8ca523f5e9506fed4657c9700eebdbec""#), "{md5}");
        let sha = authorization(
            &parse_challenge(&header("SHA-256")).unwrap(),
            "Mufasa",
            "Circle of Life",
            "GET",
            "/dir/index.html",
            1,
            cnonce,
        );
        assert!(
            sha.contains(r#"response="753927fa0e85d155564e2e272a28d1802ca10daf4496794697cf8db5856cb6c1""#),
            "{sha}"
        );
        assert!(sha.contains("algorithm=SHA-256"), "{sha}");
    }

    #[test]
    fn ef_aut_01_digest_without_qop_uses_the_rfc_2069_response() {
        let challenge = parse_challenge(r#"Digest realm="r", nonce="n""#).unwrap();
        assert!(!challenge.qop_auth);
        let header = authorization(&challenge, "u", "p", "get", "/x", 1, "c");
        assert!(!header.contains("nc=") && !header.contains("cnonce") && !header.contains("qop"), "{header}");
        let ha1 = format!("{:x}", md5::compute("u:r:p"));
        let ha2 = format!("{:x}", md5::compute("GET:/x"));
        assert!(header.contains(&format!("response=\"{:x}\"", md5::compute(format!("{ha1}:n:{ha2}")))), "{header}");
    }

    #[test]
    fn ef_aut_01_digest_session_algorithms_hash_the_nonces_into_ha1() {
        let challenge = parse_challenge(r#"Digest realm="r", nonce="n", qop="auth", algorithm=MD5-sess"#).unwrap();
        assert_eq!(challenge.algorithm, Algorithm::Md5Sess);
        let header = authorization(&challenge, "u", "p", "GET", "/x", 2, "cn");
        let ha1 = format!("{:x}", md5::compute(format!("{:x}:n:cn", md5::compute("u:r:p"))));
        let ha2 = format!("{:x}", md5::compute("GET:/x"));
        let expected = format!("{:x}", md5::compute(format!("{ha1}:n:00000002:cn:auth:{ha2}")));
        assert!(
            header.contains(&format!("response=\"{expected}\"")) && header.contains("algorithm=MD5-sess"),
            "{header}"
        );
    }

    #[test]
    fn ef_aut_01_the_challenge_is_found_among_other_schemes_and_odd_formats() {
        let challenge =
            parse_challenge(r#"Basic realm="b", Digest realm="a\"b", nonce=abc123, qop=auth, stale=false"#).unwrap();
        assert_eq!((challenge.realm.as_str(), challenge.nonce.as_str(), challenge.qop_auth), ("a\"b", "abc123", true));
        assert_eq!(parse_challenge("Basic realm=\"x\""), None);
        assert_eq!(parse_challenge(r#"Digest realm="r""#), None, "sans nonce, pas de réponse possible");
        assert_eq!(parse_challenge(r#"Digest realm="r", nonce="n", algorithm=SHA-512-256"#), None);
    }

    #[test]
    fn ef_aut_01_client_nonces_are_32_hex_characters_and_differ() {
        let (a, b) = (client_nonce(), client_nonce());
        assert!(a.len() == 32 && a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }

    #[test]
    fn ef_aut_01_quoted_values_escape_quotes_and_backslashes() {
        let challenge = parse_challenge(r#"Digest realm="r", nonce="n""#).unwrap();
        let header = authorization(&challenge, r#"a"b\c"#, "p", "GET", "/", 1, "c");
        assert!(header.contains(r#"username="a\"b\\c""#), "{header}");
    }
}
