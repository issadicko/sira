//! Le pot de cookies : en mémoire, partagé par tous les envois d'un hôte, comme celui de Bruno. Les cookies reçus
//! (`Set-Cookie`) de chaque saut y sont rangés, ceux qui correspondent à l'adresse d'un saut en repartent.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use cookie_store::{Cookie, CookieDomain, CookieExpiration, CookieStore, RawCookie, StoreAction};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::time::{iso_from_millis, millis_from_iso};

/// Le contenu d'un pot : les cookies, et l'ordre dans lequel ils sont nés (un cookie remplacé garde sa place).
#[derive(Default)]
struct Inner {
    store: CookieStore,
    born: HashMap<(String, String, String), u64>,
    next: u64,
}

type Key = (String, String, String);

fn key_of(cookie: &CookieView) -> Key {
    (cookie.domain.clone(), cookie.path.clone(), cookie.key.clone())
}

impl Inner {
    fn insert(&mut self, cookie: Cookie<'static>, url: &Url) -> Result<(), String> {
        let key = key_of(&view(&cookie));
        let action = self.store.insert(cookie, url).map_err(|e| e.to_string())?;
        match action {
            StoreAction::Inserted => {
                self.born.insert(key, self.next);
                self.next += 1;
            }
            StoreAction::ExpiredExisting => {
                self.born.remove(&key);
            }
            StoreAction::UpdatedExisting => {}
        }
        Ok(())
    }

    fn remove(&mut self, domain: &str, path: &str, key: &str) -> Option<Cookie<'static>> {
        self.born.remove(&(domain.to_owned(), path.to_owned(), key.to_owned()));
        self.store.remove(domain, path, key)
    }

    fn rank(&self, cookie: &CookieView) -> (std::cmp::Reverse<usize>, u64) {
        (std::cmp::Reverse(cookie.path.len()), self.born.get(&key_of(cookie)).copied().unwrap_or(u64::MAX))
    }
}

/// Un pot de cookies ; le cloner donne une autre poignée sur le même contenu.
#[derive(Clone, Default)]
pub struct CookieJar(Arc<Mutex<Inner>>);

impl fmt::Debug for CookieJar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CookieJar").field("cookies", &self.list().len()).finish()
    }
}

/// Ce qu'un envoi fait du pot : y puiser les cookies à envoyer, y ranger ceux que le serveur pose.
#[derive(Debug, Clone)]
pub struct Cookies {
    pub jar: CookieJar,
    pub send: bool,
    pub store: bool,
}

/// Un cookie vu de l'extérieur (interface, scripts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieView {
    pub key: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    /// Vrai quand le serveur n'a pas donné d'attribut `Domain` : le cookie ne vaut que pour cet hôte.
    pub host_only: bool,
    /// Instant ISO 8601 UTC d'expiration ; `None` pour un cookie de session.
    pub expires: Option<String>,
}

/// Un cookie à poser à la main, hors de toute réponse.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CookieDraft {
    pub key: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub host_only: bool,
    pub expires: Option<String>,
}

/// Un cookie posé par un script pour une adresse : seuls le nom et la valeur sont obligatoires.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptCookie {
    pub key: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub secure: bool,
    pub http_only: bool,
    /// Instant ISO 8601 UTC ; absent pour un cookie de session.
    pub expires: Option<String>,
}

fn view(cookie: &Cookie<'_>) -> CookieView {
    let (domain, host_only) = match &cookie.domain {
        CookieDomain::HostOnly(host) => (host.clone(), true),
        CookieDomain::Suffix(suffix) => (suffix.clone(), false),
        CookieDomain::NotPresent | CookieDomain::Empty => (String::new(), true),
    };
    let expires = match &cookie.expires {
        CookieExpiration::AtUtc(at) => u64::try_from(at.unix_timestamp_nanos() / 1_000_000).ok().map(iso_from_millis),
        CookieExpiration::SessionEnd => None,
    };
    CookieView {
        key: cookie.name().to_owned(),
        value: cookie.value().to_owned(),
        domain,
        path: String::from(&cookie.path),
        secure: cookie.secure().unwrap_or(false),
        http_only: cookie.http_only().unwrap_or(false),
        host_only,
        expires,
    }
}

/// Un nom de domaine qui est lui-même un suffixe public (`com`, `co.uk`) : un cookie ne peut pas y viser tout le monde.
fn is_public_suffix(host: &str) -> bool {
    psl::domain(host.as_bytes()).is_none()
}

fn host_of(url: &Url) -> String {
    url.host_str().unwrap_or_default().to_ascii_lowercase()
}

impl CookieJar {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Les cookies qui partent vers `url` : le chemin le plus long d'abord, puis le plus ancien.
    pub fn matching(&self, url: &Url) -> Vec<CookieView> {
        let inner = self.lock();
        let mut cookies: Vec<CookieView> = inner.store.matches(url).into_iter().map(view).collect();
        cookies.sort_by_key(|cookie| inner.rank(cookie));
        cookies
    }

    /// La valeur de l'en-tête `Cookie` pour `url`, `None` quand aucun cookie ne correspond.
    pub fn header(&self, url: &Url) -> Option<String> {
        let line = self.matching(url).iter().map(|c| format!("{}={}", c.key, c.value)).collect::<Vec<_>>().join("; ");
        (!line.is_empty()).then_some(line)
    }

    /// Range les cookies des en-têtes `Set-Cookie` reçus de `url`. Un en-tête illisible, ou qui vise un domaine
    /// étranger ou un suffixe public, est ignoré, comme dans un navigateur.
    pub fn store(&self, url: &Url, set_cookie: &[&str]) {
        let host = host_of(url);
        let mut inner = self.lock();
        for header in set_cookie {
            let Ok(raw) = RawCookie::parse(header.to_string()) else { continue };
            let attribute = raw.domain().map(|d| d.trim_start_matches('.').to_ascii_lowercase());
            if attribute
                .as_deref()
                .is_some_and(|domain| !domain.is_empty() && domain != host && is_public_suffix(domain))
            {
                continue;
            }
            if let Ok(cookie) = Cookie::try_from_raw_cookie(&raw, url) {
                let _ = inner.insert(cookie.into_owned(), url);
            }
        }
    }

    /// Tous les cookies non expirés, par domaine, chemin puis nom.
    pub fn list(&self) -> Vec<CookieView> {
        let mut cookies: Vec<CookieView> = self.lock().store.iter_unexpired().map(view).collect();
        cookies.sort_by(|a, b| (&a.domain, &a.path, &a.key).cmp(&(&b.domain, &b.path, &b.key)));
        cookies
    }

    /// Pose (ou remplace) un cookie à la main.
    pub fn put(&self, draft: &CookieDraft) -> Result<(), String> {
        let raw = raw_cookie(draft)?;
        let domain = draft.domain.trim().trim_start_matches('.');
        let url = Url::parse(&format!("{}://{domain}{}", if draft.secure { "https" } else { "http" }, path_of(draft)))
            .map_err(|_| format!("domaine invalide : {}", draft.domain))?;
        let cookie = Cookie::try_from_raw_cookie(&raw, &url).map_err(|e| e.to_string())?;
        self.lock().insert(cookie.into_owned(), &url)
    }

    /// Remplace le cookie `(domain, path, key)` par `draft` ; le premier reste en place si le second est refusé.
    pub fn replace(&self, old: (&str, &str, &str), draft: &CookieDraft) -> Result<(), String> {
        let (domain, path, key) = old;
        let previous = self.lock().remove(domain, path, key);
        self.put(draft).inspect_err(|_| {
            if let Some(previous) = previous {
                let url = Url::parse(&format!("http://{}{path}", domain.trim_start_matches('.')));
                if let Ok(url) = url {
                    let _ = self.lock().insert(previous, &url);
                }
            }
        })
    }

    /// Pose `cookie` comme le ferait un serveur dont la réponse vient de `url` : sans domaine, celui de l'adresse ; sans
    /// chemin, celui que donne l'adresse. Un cookie que le pot refuse (domaine étranger, suffixe public) est ignoré.
    pub fn set_for(&self, url: &Url, cookie: &ScriptCookie) -> Result<(), String> {
        if cookie.key.trim().is_empty() {
            return Err("le nom du cookie est obligatoire".into());
        }
        let host = host_of(url);
        let mut raw = RawCookie::new(cookie.key.clone(), cookie.value.clone());
        let domain = cookie.domain.as_deref().map(|d| d.trim_start_matches('.').to_ascii_lowercase());
        let domain =
            domain.filter(|d| !d.is_empty()).or_else(|| (!cookie.key.starts_with("__Host-")).then(|| host.clone()));
        if let Some(domain) = domain {
            if domain != host && is_public_suffix(&domain) {
                return Ok(());
            }
            raw.set_domain(domain);
        }
        if let Some(path) = cookie.path.as_deref().filter(|p| p.starts_with('/')) {
            raw.set_path(path.to_owned());
        }
        raw.set_secure(cookie.secure);
        raw.set_http_only(cookie.http_only);
        if let Some(text) = cookie.expires.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
            let ms = millis_from_iso(text).ok_or_else(|| format!("date d'expiration invalide : {text}"))?;
            let at = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
                .map_err(|_| format!("date d'expiration invalide : {text}"))?;
            raw.set_expires(at);
        }
        if let Ok(cookie) = Cookie::try_from_raw_cookie(&raw, url) {
            let _ = self.lock().insert(cookie.into_owned(), url);
        }
        Ok(())
    }

    /// Supprime les cookies qui partent vers `url`, tous ou seulement ceux de nom `key` ; rend leur nombre.
    pub fn remove_matching(&self, url: &Url, key: Option<&str>) -> usize {
        let doomed: Vec<CookieView> =
            self.matching(url).into_iter().filter(|c| key.is_none_or(|k| c.key == k)).collect();
        let mut inner = self.lock();
        doomed.iter().filter(|c| inner.remove(&c.domain, &c.path, &c.key).is_some()).count()
    }

    /// Supprime un cookie ; `false` quand il n'y en avait pas.
    pub fn delete(&self, domain: &str, path: &str, key: &str) -> bool {
        self.lock().remove(domain, path, key).is_some()
    }

    /// Supprime tous les cookies d'un domaine et rend leur nombre.
    pub fn delete_domain(&self, domain: &str) -> usize {
        let doomed: Vec<CookieView> = self.list().into_iter().filter(|c| c.domain == domain).collect();
        let mut inner = self.lock();
        doomed.iter().filter(|c| inner.remove(&c.domain, &c.path, &c.key).is_some()).count()
    }

    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.store.clear();
        inner.born.clear();
    }
}

fn pairs(line: &str) -> impl Iterator<Item = (&str, &str)> {
    line.split(';')
        .filter_map(|part| part.split_once('=').map(|(name, value)| (name.trim(), value.trim())))
        .filter(|(name, _)| !name.is_empty())
}

/// Les en-têtes avec les cookies du pot : ceux de `jar_line` se joignent à l'en-tête `Cookie` écrit à la main, et
/// l'emportent sur lui pour un même nom.
pub(crate) fn with_jar_cookies(headers: &[(String, String)], jar_line: Option<&str>) -> Vec<(String, String)> {
    let Some(jar_line) = jar_line else { return headers.to_vec() };
    let is_cookie = |name: &str| name.eq_ignore_ascii_case("cookie");
    let mut merged: Vec<(String, String)> = Vec::new();
    for (_, line) in headers.iter().filter(|(name, _)| is_cookie(name)) {
        merged.extend(pairs(line).map(|(name, value)| (name.to_owned(), value.to_owned())));
    }
    for (name, value) in pairs(jar_line) {
        match merged.iter_mut().find(|(known, _)| known == name) {
            Some(slot) => slot.1 = value.to_owned(),
            None => merged.push((name.to_owned(), value.to_owned())),
        }
    }
    let line = merged.iter().map(|(name, value)| format!("{name}={value}")).collect::<Vec<_>>().join("; ");
    let position = headers.iter().position(|(name, _)| is_cookie(name));
    let name = position.map_or_else(|| "Cookie".to_owned(), |at| headers[at].0.clone());
    let mut out: Vec<(String, String)> = headers.iter().filter(|(name, _)| !is_cookie(name)).cloned().collect();
    out.insert(position.unwrap_or(out.len()).min(out.len()), (name, line));
    out
}

fn path_of(draft: &CookieDraft) -> &str {
    if draft.path.starts_with('/') {
        &draft.path
    } else {
        "/"
    }
}

fn raw_cookie(draft: &CookieDraft) -> Result<RawCookie<'static>, String> {
    if draft.key.trim().is_empty() {
        return Err("le cookie demande un nom".into());
    }
    if draft.domain.trim().is_empty() {
        return Err("le cookie demande un domaine".into());
    }
    let mut cookie = RawCookie::new(draft.key.clone(), draft.value.clone());
    if !draft.host_only {
        cookie.set_domain(draft.domain.trim().trim_start_matches('.').to_owned());
    }
    cookie.set_path(path_of(draft).to_owned());
    cookie.set_secure(draft.secure);
    cookie.set_http_only(draft.http_only);
    if let Some(text) = draft.expires.as_deref().map(str::trim).filter(|text| !text.is_empty()) {
        let ms = millis_from_iso(text).ok_or_else(|| format!("date d'expiration invalide : {text}"))?;
        let at = time::OffsetDateTime::from_unix_timestamp_nanos(i128::from(ms) * 1_000_000)
            .map_err(|_| format!("date d'expiration invalide : {text}"))?;
        cookie.set_expires(at);
    }
    Ok(cookie)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> Url {
        Url::parse(text).unwrap()
    }

    fn jar_with(from: &str, set_cookie: &[&str]) -> CookieJar {
        let jar = CookieJar::default();
        jar.store(&url(from), set_cookie);
        jar
    }

    fn keys(cookies: &[CookieView]) -> Vec<&str> {
        cookies.iter().map(|c| c.key.as_str()).collect()
    }

    fn draft(key: &str, domain: &str) -> CookieDraft {
        CookieDraft {
            key: key.into(),
            value: "v".into(),
            domain: domain.into(),
            path: "/".into(),
            ..CookieDraft::default()
        }
    }

    #[test]
    fn ef_ux_02_a_cookie_without_domain_is_for_its_host_only_and_with_domain_for_subdomains_too() {
        let jar = jar_with("https://api.example.com/", &["a=1; Path=/", "b=2; Path=/; Domain=example.com"]);
        assert_eq!(keys(&jar.matching(&url("https://api.example.com/"))), vec!["a", "b"]);
        assert_eq!(keys(&jar.matching(&url("https://www.example.com/"))), vec!["b"]);
        assert!(jar.matching(&url("https://other.test/")).is_empty());
        let listed = jar.list();
        assert_eq!((listed[0].host_only, listed[0].domain.as_str()), (true, "api.example.com"));
        assert_eq!((listed[1].host_only, listed[1].domain.as_str()), (false, "example.com"));
    }

    #[test]
    fn ef_ux_02_a_cookie_aimed_at_another_domain_or_a_public_suffix_is_refused() {
        let jar = jar_with(
            "https://shop.example.co.uk/",
            &[
                "foreign=1; Domain=evil.test",
                "super=1; Domain=co.uk",
                "super2=1; Domain=.com",
                "ok=1; Domain=example.co.uk",
            ],
        );
        assert_eq!(keys(&jar.list()), vec!["ok"]);
    }

    #[test]
    fn ef_ux_02_a_host_that_is_itself_a_suffix_may_set_a_host_only_cookie() {
        let jar = jar_with("http://localhost:3000/", &["a=1; Domain=localhost"]);
        assert_eq!(keys(&jar.matching(&url("http://localhost:3000/"))), vec!["a"]);
    }

    #[test]
    fn ef_ux_02_a_response_can_delete_a_cookie_with_max_age_zero_or_a_past_date() {
        let jar = jar_with("https://a.test/", &["x=1; Path=/", "y=1; Path=/"]);
        jar.store(
            &url("https://a.test/"),
            &["x=; Path=/; Max-Age=0", "y=; Path=/; Expires=Thu, 01 Jan 1970 00:00:00 GMT"],
        );
        assert!(jar.list().is_empty());
    }

    #[test]
    fn ef_ux_02_cookies_are_sent_longest_path_first_then_oldest_first_and_a_replaced_one_keeps_its_place() {
        let jar = CookieJar::default();
        let at = url("https://a.test/app/page");
        jar.store(&at, &["first=1; Path=/", "second=1; Path=/", "deep=1; Path=/app"]);
        jar.store(&at, &["first=2; Path=/"]);
        assert_eq!(jar.header(&at).as_deref(), Some("deep=1; first=2; second=1"));
    }

    #[test]
    fn ef_ux_02_a_secure_cookie_stays_home_on_plain_http_and_http_only_is_sent_to_http_requests() {
        let jar = jar_with("https://a.test/", &["s=1; Path=/; Secure", "h=1; Path=/; HttpOnly"]);
        assert_eq!(jar.header(&url("http://a.test/")).as_deref(), Some("h=1"));
        assert_eq!(jar.header(&url("https://a.test/")).as_deref(), Some("s=1; h=1"));
    }

    #[test]
    fn ef_ux_02_merging_keeps_the_hand_written_header_in_place_and_jar_values_win() {
        let headers = vec![("Accept".to_owned(), "*/*".to_owned()), ("cookie".to_owned(), "a=hand; b=hand".to_owned())];
        let merged = with_jar_cookies(&headers, Some("b=jar; c=jar"));
        assert_eq!(merged, vec![("Accept".into(), "*/*".into()), ("cookie".into(), "a=hand; b=jar; c=jar".into())]);
        assert_eq!(with_jar_cookies(&headers, None), headers);
        let bare = with_jar_cookies(&[("Accept".into(), "*/*".into())], Some("a=1"));
        assert_eq!(bare, vec![("Accept".into(), "*/*".into()), ("Cookie".into(), "a=1".into())]);
        let split = vec![("Cookie".to_owned(), "a=1".to_owned()), ("COOKIE".to_owned(), "b=2".to_owned())];
        assert_eq!(with_jar_cookies(&split, Some("c=3")), vec![("Cookie".into(), "a=1; b=2; c=3".into())]);
    }

    #[test]
    fn ef_ux_02_a_cookie_is_put_by_hand_with_its_attributes_and_listed_back() {
        let jar = CookieJar::default();
        let mut cookie = draft("session", "example.com");
        cookie.path = "/api".into();
        cookie.secure = true;
        cookie.http_only = true;
        cookie.expires = Some("2099-01-02T03:04:05Z".into());
        jar.put(&cookie).unwrap();
        let listed = jar.list();
        assert_eq!(listed.len(), 1);
        let got = &listed[0];
        assert_eq!(
            (got.key.as_str(), got.value.as_str(), got.domain.as_str(), got.path.as_str()),
            ("session", "v", "example.com", "/api")
        );
        assert_eq!((got.secure, got.http_only, got.host_only), (true, true, false));
        assert_eq!(got.expires.as_deref(), Some("2099-01-02T03:04:05.000Z"));
        assert_eq!(jar.header(&url("https://www.example.com/api/x")).as_deref(), Some("session=v"));
    }

    #[test]
    fn ef_ux_02_a_host_only_cookie_put_by_hand_does_not_reach_subdomains() {
        let jar = CookieJar::default();
        jar.put(&CookieDraft { host_only: true, ..draft("a", "example.com") }).unwrap();
        assert!(jar.header(&url("http://www.example.com/")).is_none());
        assert!(jar.header(&url("http://example.com/")).is_some());
    }

    #[test]
    fn ef_ux_02_a_draft_needs_a_name_a_domain_and_a_readable_expiry() {
        let jar = CookieJar::default();
        assert!(jar.put(&draft("", "a.test")).unwrap_err().contains("nom"));
        assert!(jar.put(&draft("a", " ")).unwrap_err().contains("domaine"));
        let bad = CookieDraft { expires: Some("demain".into()), ..draft("a", "a.test") };
        assert!(jar.put(&bad).unwrap_err().contains("expiration"));
        assert!(jar.list().is_empty());
    }

    #[test]
    fn ef_ux_02_a_cookie_is_replaced_deleted_and_a_domain_emptied() {
        let jar = CookieJar::default();
        jar.put(&draft("a", "a.test")).unwrap();
        jar.put(&draft("b", "a.test")).unwrap();
        jar.put(&draft("c", "b.test")).unwrap();

        let renamed = CookieDraft { value: "new".into(), ..draft("a2", "a.test") };
        jar.replace(("a.test", "/", "a"), &renamed).unwrap();
        assert_eq!(keys(&jar.list()), vec!["a2", "b", "c"]);

        let broken = CookieDraft { expires: Some("x".into()), ..draft("a3", "a.test") };
        assert!(jar.replace(("a.test", "/", "a2"), &broken).is_err());
        assert_eq!(keys(&jar.list()), vec!["a2", "b", "c"], "un remplacement refusé laisse l'ancien cookie");

        assert!(jar.delete("a.test", "/", "b"));
        assert!(!jar.delete("a.test", "/", "b"));
        assert_eq!(jar.delete_domain("a.test"), 1);
        assert_eq!(keys(&jar.list()), vec!["c"]);
        jar.clear();
        assert!(jar.list().is_empty());
    }

    fn scripted(key: &str) -> ScriptCookie {
        ScriptCookie { key: key.into(), value: "v".into(), ..ScriptCookie::default() }
    }

    #[test]
    fn ef_scr_02_a_script_cookie_takes_the_domain_and_path_of_the_url_it_is_set_for() {
        let jar = CookieJar::default();
        let at = url("https://api.example.com/v1/users");
        jar.set_for(&at, &scripted("sid")).unwrap();
        let listed = jar.list();
        assert_eq!(
            (listed[0].domain.as_str(), listed[0].path.as_str(), listed[0].host_only),
            ("api.example.com", "/v1", false)
        );
        assert_eq!(
            jar.header(&url("https://www.api.example.com/v1/x")).as_deref(),
            Some("sid=v"),
            "il vaut pour les sous-domaines"
        );
        assert!(jar.header(&url("https://api.example.com/other")).is_none());
    }

    #[test]
    fn ef_scr_02_a_script_cookie_keeps_its_own_attributes_and_a_host_prefix_stays_host_only() {
        let jar = CookieJar::default();
        let at = url("https://a.test/");
        let full = ScriptCookie {
            path: Some("/app".into()),
            secure: true,
            http_only: true,
            expires: Some("2099-01-02T03:04:05.000Z".into()),
            ..scripted("full")
        };
        jar.set_for(&at, &full).unwrap();
        jar.set_for(&at, &scripted("__Host-id")).unwrap();
        let listed = jar.list();
        let got = |key: &str| listed.iter().find(|c| c.key == key).unwrap().clone();
        assert_eq!((got("full").path.as_str(), got("full").secure, got("full").http_only), ("/app", true, true));
        assert_eq!(got("full").expires.as_deref(), Some("2099-01-02T03:04:05.000Z"));
        assert!(got("__Host-id").host_only);
    }

    #[test]
    fn ef_scr_02_a_script_cookie_needs_a_name_and_one_for_a_foreign_or_public_domain_is_ignored() {
        let jar = CookieJar::default();
        let at = url("https://a.test/");
        assert!(jar.set_for(&at, &scripted(" ")).is_err());
        jar.set_for(&at, &ScriptCookie { domain: Some("evil.test".into()), ..scripted("x") }).unwrap();
        jar.set_for(&at, &ScriptCookie { domain: Some("com".into()), ..scripted("y") }).unwrap();
        assert!(jar.list().is_empty());
        let bad = ScriptCookie { expires: Some("demain".into()), ..scripted("z") };
        assert!(jar.set_for(&at, &bad).unwrap_err().contains("expiration"));
    }

    #[test]
    fn ef_scr_02_matching_cookies_are_removed_all_or_by_name() {
        let jar = jar_with("https://a.test/", &["a=1; Path=/", "b=2; Path=/", "elsewhere=3; Path=/other"]);
        let at = url("https://a.test/page");
        assert_eq!(jar.remove_matching(&at, Some("a")), 1);
        assert_eq!(keys(&jar.matching(&at)), vec!["b"]);
        assert_eq!(jar.remove_matching(&at, None), 1);
        assert_eq!(keys(&jar.list()), vec!["elsewhere"], "un cookie d'un autre chemin n'est pas touché");
    }

    #[test]
    fn ef_ux_02_clones_of_a_jar_share_their_cookies() {
        let jar = CookieJar::default();
        let other = jar.clone();
        jar.put(&draft("a", "a.test")).unwrap();
        assert_eq!(keys(&other.list()), vec!["a"]);
    }
}
