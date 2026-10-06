//! Le saut suivant d'une redirection : méthode, corps et en-têtes que Bruno garde ou retire.

use url::Url;

use crate::network::Redirects;
use crate::HttpResponse;

pub(crate) const STATUSES: [u16; 5] = [301, 302, 303, 307, 308];

/// La requête d'un saut : ce qui change d'un saut à l'autre.
#[derive(Debug, Clone)]
pub(crate) struct Hop {
    pub method: String,
    pub url: Url,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

fn drop_header(headers: &mut Vec<(String, String)>, matches: impl Fn(&str) -> bool) {
    headers.retain(|(name, _)| !matches(&name.to_ascii_lowercase()));
}

/// Le saut qui suit `response` envoyée en réponse à `from`, ou `None` quand la réponse est la finale : redirections
/// désactivées, limite atteinte, statut qui n'en est pas une, `Location` absent ou illisible.
pub(crate) fn next(from: &Hop, response: &HttpResponse, redirects: &Redirects, followed: u32) -> Option<Hop> {
    if followed >= redirects.limit() || !STATUSES.contains(&response.status) {
        return None;
    }
    let (_, location) = response.headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("location"))?;
    let url = from.url.join(location.trim()).ok().filter(|url| matches!(url.scheme(), "http" | "https"))?;

    let mut hop = Hop { url, ..from.clone() };
    let becomes_get = matches!(response.status, 301..=303) && !from.method.eq_ignore_ascii_case("HEAD");
    if becomes_get {
        hop.method = "GET".into();
        hop.body = None;
        drop_header(&mut hop.headers, |name| name == "content-length" || name == "content-type");
    }
    if from.url.origin() != hop.url.origin() {
        drop_header(&mut hop.headers, |name| name.starts_with("x-amz-") || name == "host" || name == "cookie");
        if !redirects.forward_authorization {
            drop_header(&mut hop.headers, |name| name == "authorization" || name == "proxy-authorization");
        }
    }
    Some(hop)
}
