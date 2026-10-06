//! Réglages réseau d'une requête : redirections, TLS (vérification, autorités, certificat client) et proxy.

use url::Url;

use crate::cookies::Cookies;

/// Les redirections, comme Bruno : 301, 302, 303, 307 et 308 sont suivies jusqu'à `max` fois ; la réponse qui dépasse la
/// limite est rendue telle quelle. `forward_authorization` vaut `true` à l'exécution quand le fichier ne dit rien.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Redirects {
    pub follow: bool,
    pub max: u32,
    pub forward_authorization: bool,
}

impl Redirects {
    pub const DEFAULT_MAX: u32 = 5;

    /// Aucune redirection n'est suivie : la 3xx est la réponse.
    pub const fn none() -> Self {
        Self { follow: false, max: 0, forward_authorization: true }
    }

    pub const fn limit(&self) -> u32 {
        if self.follow {
            self.max
        } else {
            0
        }
    }
}

impl Default for Redirects {
    fn default() -> Self {
        Self { follow: true, max: Self::DEFAULT_MAX, forward_authorization: true }
    }
}

/// Un certificat client et sa clé privée, en PEM.
#[derive(Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    pub cert_pem: Vec<u8>,
    pub key_pem: Vec<u8>,
}

impl std::fmt::Debug for ClientIdentity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientIdentity").field("cert_pem", &self.cert_pem.len()).field("key_pem", &"…").finish()
    }
}

/// TLS : la vérification du serveur (chaîne et nom d'hôte), les autorités de confiance et le certificat client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tls {
    /// `false` : aucune vérification, ni de la chaîne ni du nom d'hôte.
    pub verify: bool,
    /// Des autorités en PEM (un ou plusieurs certificats par élément), ajoutées à celles de confiance.
    pub extra_roots: Vec<Vec<u8>>,
    /// `false` avec des `extra_roots` : seules celles-ci font confiance (ni le système ni les racines intégrées).
    pub keep_default_roots: bool,
    pub client: Option<ClientIdentity>,
}

impl Default for Tls {
    fn default() -> Self {
        Self { verify: true, extra_roots: Vec::new(), keep_default_roots: true, client: None }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProxyScheme {
    Http,
    Https,
    Socks4,
    Socks5,
}

/// Un proxy : l'adresse à joindre, ses identifiants et la liste de ce qu'il ne doit pas voir passer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proxy {
    pub scheme: ProxyScheme,
    pub host: String,
    pub port: u16,
    pub auth: Option<(String, String)>,
    /// Noms d'hôte contournés, séparés par des virgules, points-virgules ou espaces ; `*` contourne tout.
    pub bypass: String,
}

impl Proxy {
    /// Vrai quand la requête vers `url` passe par ce proxy : `bypass` suit `proxy-from-env`, comme Bruno.
    pub fn applies_to(&self, url: &Url) -> bool {
        uses_proxy(url, &self.bypass)
    }
}

/// `proxy-from-env` : `*` contourne tout ; un nom exact doit être l'hôte ; `.x` et `*.x` couvrent les sous-domaines
/// (et `*x` aussi `x`) ; `nom:port` ne vaut que pour ce port. Ni CIDR, ni `<local>`.
fn uses_proxy(url: &Url, bypass: &str) -> bool {
    let bypass = bypass.trim();
    if bypass.is_empty() {
        return true;
    }
    if bypass == "*" {
        return false;
    }
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else { return false };
    let port = url.port_or_known_default().unwrap_or(0);
    bypass.split([',', ';', ' ', '\t', '\n']).filter(|entry| !entry.is_empty()).all(|entry| {
        let (name, entry_port) = match entry.rsplit_once(':') {
            Some((name, digits))
                if !name.is_empty() && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
            {
                (name, digits.parse::<u16>().unwrap_or(0))
            }
            _ => (entry, 0),
        };
        if entry_port != 0 && entry_port != port {
            return true;
        }
        if !name.starts_with(['.', '*']) {
            return host != name;
        }
        let suffix = name.strip_prefix('*').unwrap_or(name);
        !host.ends_with(suffix)
    })
}

#[derive(Debug, Clone, Default)]
pub struct Network {
    pub redirects: Redirects,
    pub tls: Tls,
    pub proxy: Option<Proxy>,
    /// Le pot de cookies à utiliser ; `None` : aucun cookie n'est envoyé ni gardé.
    pub cookies: Option<Cookies>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn goes(url: &str, bypass: &str) -> bool {
        uses_proxy(&Url::parse(url).unwrap(), bypass)
    }

    #[test]
    fn ef_req_04_bypass_follows_proxy_from_env() {
        assert!(goes("http://a.test/", ""));
        assert!(!goes("http://a.test/", "*"));
        assert!(!goes("http://localhost:3000/", "localhost"));
        assert!(goes("http://sub.example.com/", "example.com"), "un nom exact ne couvre pas les sous-domaines");
        assert!(!goes("http://sub.example.com/", ".example.com"));
        assert!(!goes("http://sub.example.com/", "*.example.com"));
        assert!(goes("http://example.com/", "*.example.com"), "*.x ne couvre pas x");
        assert!(!goes("http://example.com/", "*example.com"));
        assert!(!goes("http://fooexample.com/", "*example.com"));
    }

    #[test]
    fn ef_req_04_bypass_entries_split_on_commas_semicolons_and_spaces_and_honour_ports() {
        assert!(!goes("http://b.test/", "a.test, b.test;c.test"));
        assert!(!goes("http://c.test/", "a.test b.test\tc.test"));
        assert!(goes("http://a.test:8080/", "a.test:9090"));
        assert!(!goes("http://a.test:9090/", "a.test:9090"));
        assert!(!goes("https://a.test/", "a.test:443"), "le port par défaut du schéma compte");
        assert!(!goes("http://[::1]:8080/", "[::1]"));
        assert!(goes("http://10.1.2.3/", "10.0.0.0/8"), "les CIDR ne sont pas gérés");
    }
}
