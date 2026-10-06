//! Réglages réseau d'un envoi : TLS, autorité de certification, certificat client et proxy. Ils viennent de trois
//! endroits, comme dans Bruno : l'hôte (préférences de l'application ou options de la ligne de commande), le fichier de
//! la collection (`config.proxy`, `config.clientCertificates`) et la requête (redirections, voir `prepare`).

use std::fs;
use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;
use serde::{Deserialize, Serialize};
use url::Url;
use xc_engine::{ClientIdentity, Network, Proxy, ProxyScheme, Redirects, Tls};

use crate::yaml::{Map, Value};
use crate::CoreError;

const LOOPBACK: &str = "localhost,127.0.0.1,[::1]";
const CERT_SCHEMES: [&str; 5] = ["https://", "grpc://", "grpcs://", "ws://", "wss://"];

/// D'où vient le proxy quand la collection n'impose rien.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProxyMode {
    /// Aucun proxy.
    Off,
    /// Les variables d'environnement `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` et `NO_PROXY`.
    #[default]
    System,
    /// Le proxy décrit dans `config`.
    Manual,
}

/// Un proxy tel qu'on l'écrit : des textes, qui peuvent contenir des variables `{{…}}` dans une collection.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProxyConfig {
    /// `http`, `https`, `socks4` ou `socks5`.
    pub protocol: String,
    pub hostname: String,
    pub port: String,
    pub username: String,
    /// Jamais écrit dans un fichier de préférences : l'application le garde dans le trousseau.
    #[serde(skip)]
    pub password: String,
    pub auth_disabled: bool,
    pub bypass_proxy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ProxyPref {
    pub mode: ProxyMode,
    pub config: ProxyConfig,
}

/// Une entrée de certificat client, avec les noms de champs de Bruno (`bruno.json`, `--client-cert-config`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientCertificate {
    pub domain: String,
    /// `cert` (PEM, défaut) ou `pfx` (PKCS#12).
    #[serde(rename = "type")]
    pub kind: String,
    pub cert_file_path: String,
    pub key_file_path: String,
    pub pfx_file_path: String,
    pub passphrase: String,
    pub disabled: bool,
}

/// Les réglages que l'hôte donne à tous ses envois.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NetworkPrefs {
    /// `false` : ni la chaîne ni le nom d'hôte du serveur ne sont vérifiés.
    pub verify_tls: bool,
    /// Un fichier PEM d'autorités de confiance, en plus de celles du système.
    pub ca_file: Option<String>,
    /// `false` avec `ca_file` : seul ce fichier fait confiance.
    pub keep_default_roots: bool,
    /// Après ceux de la collection. Passés par `--client-cert-config` en ligne de commande.
    pub client_certificates: Vec<ClientCertificate>,
    pub proxy: ProxyPref,
    /// Ignore tout proxy, celui de la collection compris (`--noproxy`) ; propre à une exécution, jamais enregistré.
    #[serde(skip)]
    pub no_proxy: bool,
}

impl Default for NetworkPrefs {
    fn default() -> Self {
        Self {
            verify_tls: true,
            ca_file: None,
            keep_default_roots: true,
            client_certificates: Vec::new(),
            proxy: ProxyPref::default(),
            no_proxy: false,
        }
    }
}

const PREFS_FILE: &str = "network.json";

impl NetworkPrefs {
    /// Les préférences enregistrées dans `data_dir` ; celles par défaut quand le fichier manque ou est illisible.
    pub fn load(data_dir: &Path) -> Self {
        fs::read_to_string(data_dir.join(PREFS_FILE))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    /// Enregistre les préférences, sans le mot de passe du proxy ni les options propres à une exécution.
    pub fn save(&self, data_dir: &Path) -> Result<(), CoreError> {
        fs::create_dir_all(data_dir).map_err(|e| CoreError::io(data_dir, e))?;
        let text = serde_json::to_string_pretty(self).map_err(|e| network_error(e.to_string()))?;
        crate::collection::write_atomic(data_dir, &data_dir.join(PREFS_FILE), &format!("{text}\n"))
    }

    /// Ce qui empêcherait d'envoyer : un protocole inconnu, ou un proxy manuel sans hôte ni port valable.
    pub fn validate(&self) -> Result<(), String> {
        let config = &self.proxy.config;
        if self.proxy.mode != ProxyMode::Manual {
            return Ok(());
        }
        if parse_scheme(&config.protocol).is_none() {
            return Err(format!(
                "protocole de proxy non pris en charge : {} (http, https, socks4 ou socks5)",
                config.protocol
            ));
        }
        if config.hostname.trim().is_empty() {
            return Err("le proxy manuel demande un nom d'hôte".into());
        }
        match config.port.trim() {
            "" => Ok(()),
            digits if digits.parse::<u16>().is_ok_and(|port| port > 0) => Ok(()),
            digits => Err(format!("port de proxy invalide : {digits} (de 1 à 65535)")),
        }
    }
}

fn network_error(message: impl Into<String>) -> CoreError {
    CoreError::Network(message.into())
}

/// Ce que la collection dit du proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
enum CollectionProxy {
    Disabled,
    Own(ProxyConfig),
    Inherit,
}

fn config_of(collection: &Map) -> Option<&Map> {
    collection.map("config")
}

fn collection_proxy(collection: &Map) -> CollectionProxy {
    let Some(proxy) = config_of(collection).and_then(|config| config.map("proxy")) else {
        return CollectionProxy::Inherit;
    };
    let (Some(inherit), Some(config)) = (proxy.get("inherit").and_then(bool_of), proxy.map("config")) else {
        return CollectionProxy::Inherit;
    };
    if proxy.get("disabled").is_some_and(Value::is_true) {
        return CollectionProxy::Disabled;
    }
    if inherit {
        return CollectionProxy::Inherit;
    }
    let text = |map: &Map, key: &str| map.get(key).and_then(Value::scalar).unwrap_or_default();
    let auth = config.map("auth");
    CollectionProxy::Own(ProxyConfig {
        protocol: text(config, "protocol"),
        hostname: text(config, "hostname"),
        port: text(config, "port"),
        username: auth.map(|a| text(a, "username")).unwrap_or_default(),
        password: auth.map(|a| text(a, "password")).unwrap_or_default(),
        auth_disabled: auth.and_then(|a| a.get("disabled")).is_some_and(Value::is_true),
        bypass_proxy: text(config, "bypassProxy"),
    })
}

fn bool_of(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        _ => None,
    }
}

/// Les certificats client du fichier de la collection ; un type inconnu est ignoré, comme dans Bruno.
fn collection_certificates(collection: &Map) -> Vec<ClientCertificate> {
    let Some(config) = config_of(collection) else { return Vec::new() };
    let text = |map: &Map, key: &str| map.str(key).unwrap_or_default().to_owned();
    config
        .seq("clientCertificates")
        .iter()
        .filter_map(Value::as_map)
        .filter_map(|entry| {
            let common = ClientCertificate {
                domain: text(entry, "domain"),
                passphrase: entry.get("passphrase").and_then(Value::scalar).unwrap_or_default(),
                disabled: entry.get("disabled").is_some_and(Value::is_true),
                ..ClientCertificate::default()
            };
            match entry.str("type") {
                Some("pem") => Some(ClientCertificate {
                    kind: "cert".into(),
                    cert_file_path: text(entry, "certificateFilePath"),
                    key_file_path: text(entry, "privateKeyFilePath"),
                    ..common
                }),
                Some("pkcs12") => Some(ClientCertificate {
                    kind: "pfx".into(),
                    pfx_file_path: text(entry, "pkcs12FilePath"),
                    ..common
                }),
                _ => None,
            }
        })
        .collect()
}

/// Vrai quand `pattern` est un préfixe de `text`, où `*` vaut n'importe quelle suite de caractères.
fn prefix_matches(pattern: &str, text: &str) -> bool {
    let (pattern, text) = (pattern.as_bytes(), text.as_bytes());
    let (mut p, mut t, mut star) = (0, 0, None::<(usize, usize)>);
    loop {
        if p == pattern.len() {
            return true;
        }
        if pattern[p] == b'*' {
            star = Some((p, t));
            p += 1;
        } else if t < text.len() && pattern[p] == text[t] {
            p += 1;
            t += 1;
        } else {
            match star {
                Some((star_p, star_t)) if star_t < text.len() => {
                    star = Some((star_p, star_t + 1));
                    p = star_p + 1;
                    t = star_t + 1;
                }
                _ => return false,
            }
        }
    }
}

/// Comme Bruno : le domaine, `*` valant « n'importe quoi », est un préfixe de l'adresse, schéma facultatif.
/// Sensible à la casse. Les caractères spéciaux d'une expression régulière n'ont pas de sens particulier ici.
fn domain_matches(domain: &str, url: &str) -> bool {
    prefix_matches(domain, url)
        || CERT_SCHEMES
            .iter()
            .find_map(|scheme| url.strip_prefix(scheme))
            .is_some_and(|rest| prefix_matches(domain, rest))
}

fn absolute(root: &Path, file: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn read(path: &Path, what: &str) -> Result<Vec<u8>, CoreError> {
    fs::read(path)
        .map_err(|e| network_error(format!("{what} illisible ({}) : {}", path.display(), crate::io_message(&e))))
}

fn identity(
    root: &Path,
    cert: &ClientCertificate,
    fill: &mut dyn FnMut(&str) -> String,
) -> Result<ClientIdentity, CoreError> {
    if cert.kind == "pfx" {
        return Err(network_error(
            "un certificat client PKCS#12 n'est pas encore pris en charge : convertissez-le en PEM \
             (openssl pkcs12 -in certificat.pfx -out certificat.pem -nodes)",
        ));
    }
    let (cert_file, key_file) = (fill(&cert.cert_file_path), fill(&cert.key_file_path));
    if cert_file.trim().is_empty() || key_file.trim().is_empty() {
        return Err(network_error("un certificat client PEM demande le fichier du certificat et celui de la clé"));
    }
    let key = read(&absolute(root, &key_file), "clé privée du certificat client")?;
    if String::from_utf8_lossy(&key).contains("ENCRYPTED") {
        return Err(network_error(
            "la clé privée du certificat client est chiffrée : retirez sa passphrase \
             (openssl pkey -in cle.pem -out cle-sans-passphrase.pem)",
        ));
    }
    Ok(ClientIdentity { cert_pem: read(&absolute(root, &cert_file), "certificat client")?, key_pem: key })
}

fn client_identity(
    root: &Path,
    collection: &Map,
    prefs: &NetworkPrefs,
    url: &str,
    fill: &mut dyn FnMut(&str) -> String,
) -> Result<Option<ClientIdentity>, CoreError> {
    let own = collection_certificates(collection);
    for cert in own.iter().chain(&prefs.client_certificates).filter(|c| !c.disabled) {
        let domain = fill(&cert.domain);
        if !domain.is_empty() && domain_matches(&domain, url) {
            return identity(root, cert, fill).map(Some);
        }
    }
    Ok(None)
}

fn extra_roots(prefs: &NetworkPrefs) -> Result<Vec<Vec<u8>>, CoreError> {
    let Some(file) = prefs.ca_file.as_deref().map(str::trim).filter(|f| !f.is_empty()) else { return Ok(Vec::new()) };
    let path = Path::new(file);
    if !path.is_file() {
        return Err(network_error(format!("autorité de certification personnalisée introuvable : {file}")));
    }
    let pem = read(path, "autorité de certification personnalisée")?;
    Ok(if pem.iter().all(u8::is_ascii_whitespace) { Vec::new() } else { vec![pem] })
}

fn parse_scheme(protocol: &str) -> Option<ProxyScheme> {
    let protocol = protocol.trim().to_ascii_lowercase();
    match protocol.as_str() {
        "" | "http" => Some(ProxyScheme::Http),
        "https" => Some(ProxyScheme::Https),
        other if other.starts_with("socks4") => Some(ProxyScheme::Socks4),
        other if other.starts_with("socks") => Some(ProxyScheme::Socks5),
        _ => None,
    }
}

const fn default_port(scheme: ProxyScheme) -> u16 {
    match scheme {
        ProxyScheme::Http => 80,
        ProxyScheme::Https => 443,
        ProxyScheme::Socks4 | ProxyScheme::Socks5 => 1080,
    }
}

/// Le proxy décrit par `config`, variables résolues ; `None` quand il n'a pas d'hôte, ce que Bruno laisse sans proxy.
fn manual_proxy(config: &ProxyConfig, fill: &mut dyn FnMut(&str) -> String) -> Result<Option<Proxy>, CoreError> {
    let protocol = fill(&config.protocol);
    let scheme = parse_scheme(&protocol).ok_or_else(|| {
        network_error(format!("protocole de proxy non pris en charge : {protocol} (http, https, socks4 ou socks5)"))
    })?;
    let host = fill(&config.hostname).trim().to_owned();
    if host.is_empty() {
        return Ok(None);
    }
    let port = match fill(&config.port).trim() {
        "" => default_port(scheme),
        digits => digits.parse().map_err(|_| network_error(format!("port de proxy invalide : {digits}")))?,
    };
    let (username, password) = (fill(&config.username), fill(&config.password));
    let anonymous = username.is_empty() && password.is_empty();
    let auth = (!config.auth_disabled && !anonymous).then_some((username, password));
    Ok(Some(Proxy { scheme, host, port, auth, bypass: config.bypass_proxy.clone() }))
}

fn first_of(env: &dyn Fn(&str) -> Option<String>, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| env(name)).filter(|value| !value.trim().is_empty())
}

/// Le proxy de l'environnement pour `url` : `HTTP_PROXY` ou `HTTPS_PROXY` selon son schéma, puis `ALL_PROXY` ; la
/// liste `NO_PROXY` et la boucle locale ne passent pas par lui.
fn system_proxy(url: &str, env: &dyn Fn(&str) -> Option<String>) -> Result<Option<Proxy>, CoreError> {
    let secure = url.starts_with("https://");
    let names: [&str; 4] = if secure {
        ["https_proxy", "HTTPS_PROXY", "all_proxy", "ALL_PROXY"]
    } else {
        ["http_proxy", "HTTP_PROXY", "all_proxy", "ALL_PROXY"]
    };
    let Some(value) = first_of(env, &names) else { return Ok(None) };
    let text = if value.contains("://") { value.clone() } else { format!("http://{value}") };
    let parsed = Url::parse(&text).map_err(|e| network_error(format!("variable de proxy invalide ({value}) : {e}")))?;
    let scheme = parse_scheme(parsed.scheme())
        .ok_or_else(|| network_error(format!("protocole de proxy non pris en charge : {}", parsed.scheme())))?;
    let host = parsed.host_str().unwrap_or_default().trim_start_matches('[').trim_end_matches(']').to_owned();
    if host.is_empty() {
        return Err(network_error(format!("variable de proxy invalide ({value}) : hôte manquant")));
    }
    let decode = |s: &str| percent_decode_str(s).decode_utf8_lossy().into_owned();
    let auth =
        (!parsed.username().is_empty()).then(|| (decode(parsed.username()), decode(parsed.password().unwrap_or(""))));
    let excluded = first_of(env, &["no_proxy", "NO_PROXY"]).unwrap_or_default();
    let separators = |c: char| c == ',' || c == ';' || c.is_whitespace();
    let listed: Vec<&str> = excluded.split(separators).filter(|entry| !entry.is_empty()).collect();
    let bypass = if listed.is_empty() { LOOPBACK.to_owned() } else { format!("{},{LOOPBACK}", listed.join(",")) };
    Ok(Some(Proxy { scheme, host, port: parsed.port().unwrap_or_else(|| default_port(scheme)), auth, bypass }))
}

fn proxy_for(
    collection: &Map,
    prefs: &NetworkPrefs,
    url: &str,
    fill: &mut dyn FnMut(&str) -> String,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Option<Proxy>, CoreError> {
    if prefs.no_proxy {
        return Ok(None);
    }
    match collection_proxy(collection) {
        CollectionProxy::Disabled => Ok(None),
        CollectionProxy::Own(config) => manual_proxy(&config, fill),
        CollectionProxy::Inherit => match prefs.proxy.mode {
            ProxyMode::Off => Ok(None),
            ProxyMode::System => system_proxy(url, env),
            ProxyMode::Manual => manual_proxy(&prefs.proxy.config, fill),
        },
    }
}

/// Les réglages réseau de l'envoi vers `url` : ceux de l'hôte, complétés par le fichier de la collection (`collection`,
/// dont les chemins sont relatifs à `root`). `fill` résout les variables, `env` lit l'environnement du processus.
pub fn resolve(
    root: &Path,
    collection: &Map,
    prefs: &NetworkPrefs,
    url: &str,
    redirects: Redirects,
    fill: &mut dyn FnMut(&str) -> String,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<Network, CoreError> {
    let tls = Tls {
        verify: prefs.verify_tls,
        extra_roots: if prefs.verify_tls { extra_roots(prefs)? } else { Vec::new() },
        keep_default_roots: prefs.keep_default_roots,
        client: client_identity(root, collection, prefs, url, fill)?,
    };
    Ok(Network { redirects, tls, proxy: proxy_for(collection, prefs, url, fill, env)? })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::yaml;

    fn collection(config: &str) -> Map {
        let text = format!("opencollection: 1.0.0\ninfo:\n  name: c\nconfig:\n{config}");
        yaml::parse(&text).unwrap().as_map().unwrap().clone()
    }

    fn keep(s: &str) -> String {
        s.to_owned()
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn proxy_of(config: &str, prefs: &NetworkPrefs, env: &dyn Fn(&str) -> Option<String>) -> Option<Proxy> {
        proxy_for(&collection(config), prefs, "http://api.test/", &mut keep, env).unwrap()
    }

    #[test]
    fn ef_req_04_a_domain_is_a_case_sensitive_prefix_with_an_optional_scheme_and_star_for_anything() {
        assert!(domain_matches("api.example.com", "https://api.example.com/v1"));
        assert!(domain_matches("api.example.com", "https://api.example.com:8443/v1"));
        assert!(domain_matches("api.example.com", "https://api.example.com.evil.net/"), "simple préfixe, comme Bruno");
        assert!(!domain_matches("api.example.com", "http://api.example.com/"), "http n'est jamais accepté");
        assert!(!domain_matches("api.example.com", "https://API.example.com/"));
        assert!(domain_matches("*.example.com", "https://sub.example.com/"));
        assert!(!domain_matches("*.example.com", "https://example.com/"));
        assert!(domain_matches("example.com:8443", "https://example.com:8443/x"));
        assert!(domain_matches("wss://stream.test", "wss://stream.test/live"));
        assert!(domain_matches("a*b*c", "https://a-b-c/"));
        assert!(!domain_matches("a*b*c", "https://a-b/"));
    }

    #[test]
    fn ef_req_04_the_collection_proxy_wins_over_the_host_and_can_be_disabled() {
        let prefs = NetworkPrefs {
            proxy: ProxyPref {
                mode: ProxyMode::Manual,
                config: ProxyConfig { hostname: "global.test".into(), port: "3128".into(), ..ProxyConfig::default() },
            },
            ..NetworkPrefs::default()
        };
        let own = "  proxy:\n    inherit: false\n    config:\n      protocol: socks5\n      hostname: corp.test\n      port: 1081\n      auth:\n        username: alice\n        password: s3cret\n      bypassProxy: localhost\n";
        let proxy = proxy_of(own, &prefs, &no_env).unwrap();
        assert_eq!((proxy.scheme, proxy.host.as_str(), proxy.port), (ProxyScheme::Socks5, "corp.test", 1081));
        assert_eq!(proxy.auth, Some(("alice".into(), "s3cret".into())));
        assert_eq!(proxy.bypass, "localhost");

        let inherit = "  proxy:\n    inherit: true\n    config:\n      hostname: ignoré.test\n";
        assert_eq!(proxy_of(inherit, &prefs, &no_env).unwrap().host, "global.test");

        let disabled = "  proxy:\n    inherit: false\n    disabled: true\n    config:\n      hostname: corp.test\n";
        assert_eq!(proxy_of(disabled, &prefs, &no_env), None);
    }

    #[test]
    fn ef_req_04_a_malformed_collection_proxy_block_means_inherit() {
        let prefs = NetworkPrefs {
            proxy: ProxyPref { mode: ProxyMode::Off, ..ProxyPref::default() },
            ..NetworkPrefs::default()
        };
        for config in [
            "  proxy:\n    hostname: corp.test\n",
            "  proxy:\n    inherit: yes-please\n    config:\n      hostname: corp.test\n",
        ] {
            assert_eq!(collection_proxy(&collection(config)), CollectionProxy::Inherit, "{config}");
            assert_eq!(proxy_of(config, &prefs, &no_env), None);
        }
    }

    #[test]
    fn ef_req_04_proxy_fields_are_interpolated_but_not_the_bypass_list() {
        let own = "  proxy:\n    inherit: false\n    config:\n      protocol: http\n      hostname: '{{proxy_host}}'\n      port: '{{proxy_port}}'\n      auth:\n        username: '{{user}}'\n        password: x\n      bypassProxy: '{{skip}}'\n";
        let mut fill = |s: &str| {
            s.replace("{{proxy_host}}", "corp.test").replace("{{proxy_port}}", "8080").replace("{{user}}", "bob")
        };
        let proxy =
            proxy_for(&collection(own), &NetworkPrefs::default(), "http://a/", &mut fill, &no_env).unwrap().unwrap();
        assert_eq!((proxy.host.as_str(), proxy.port), ("corp.test", 8080));
        assert_eq!(proxy.auth, Some(("bob".into(), "x".into())));
        assert_eq!(proxy.bypass, "{{skip}}");
    }

    #[test]
    fn ef_req_04_disabled_auth_and_an_empty_port_and_host_follow_bruno() {
        let no_auth = "  proxy:\n    inherit: false\n    config:\n      hostname: corp.test\n      auth:\n        username: a\n        password: b\n        disabled: true\n";
        let proxy = proxy_of(no_auth, &NetworkPrefs::default(), &no_env).unwrap();
        assert_eq!((proxy.auth, proxy.port), (None, 80));

        let no_host = "  proxy:\n    inherit: false\n    config:\n      protocol: http\n";
        assert_eq!(proxy_of(no_host, &NetworkPrefs::default(), &no_env), None, "sans hôte : pas de proxy");

        let bad = "  proxy:\n    inherit: false\n    config:\n      protocol: ftp\n      hostname: corp.test\n";
        assert!(proxy_for(&collection(bad), &NetworkPrefs::default(), "http://a/", &mut keep, &no_env).is_err());
    }

    #[test]
    fn ef_req_04_no_proxy_option_beats_everything() {
        let own = "  proxy:\n    inherit: false\n    config:\n      hostname: corp.test\n";
        let prefs = NetworkPrefs { no_proxy: true, ..NetworkPrefs::default() };
        assert_eq!(proxy_of(own, &prefs, &|_| Some("http://env.test:3128".into())), None);
    }

    #[test]
    fn ef_req_04_the_system_proxy_comes_from_the_environment_by_scheme_with_loopback_kept_direct() {
        let env = |name: &str| match name {
            "HTTP_PROXY" => Some("http://alice:p%40ss@proxy.test:3128".to_owned()),
            "https_proxy" => Some("secure.test:8443".to_owned()),
            "NO_PROXY" => Some("internal.test; *.corp.test".to_owned()),
            _ => None,
        };
        let http = system_proxy("http://api.test/", &env).unwrap().unwrap();
        assert_eq!((http.host.as_str(), http.port), ("proxy.test", 3128));
        assert_eq!(http.auth, Some(("alice".into(), "p@ss".into())));
        assert_eq!(http.bypass, "internal.test,*.corp.test,localhost,127.0.0.1,[::1]");

        let https = system_proxy("https://api.test/", &env).unwrap().unwrap();
        assert_eq!((https.host.as_str(), https.port), ("secure.test", 8443));
        assert_eq!(https.scheme, ProxyScheme::Http, "sans schéma : http");

        assert_eq!(system_proxy("http://api.test/", &no_env).unwrap(), None);
        assert!(!http.applies_to(&Url::parse("http://localhost:3000/").unwrap()));
        assert!(http.applies_to(&Url::parse("http://api.test/").unwrap()));
    }

    #[test]
    fn ef_req_04_all_proxy_serves_both_schemes_after_the_specific_variable() {
        let env = |name: &str| (name == "ALL_PROXY").then(|| "socks5://socks.test:1080".to_owned());
        assert_eq!(system_proxy("https://a/", &env).unwrap().unwrap().scheme, ProxyScheme::Socks5);
        assert_eq!(system_proxy("http://a/", &env).unwrap().unwrap().host, "socks.test");
    }

    #[test]
    fn ef_req_04_an_unreadable_proxy_variable_is_reported() {
        let env = |name: &str| (name == "http_proxy").then(|| "http://".to_owned());
        assert!(system_proxy("http://a/", &env).is_err());
    }

    #[test]
    fn ef_req_04_collection_certificates_are_read_by_type_and_unknown_types_are_dropped() {
        let config = "  clientCertificates:\n    - domain: api.test\n      type: pem\n      certificateFilePath: certs/c.pem\n      privateKeyFilePath: certs/k.pem\n      passphrase: pw\n    - domain: '*.test'\n      type: pkcs12\n      pkcs12FilePath: certs/c.p12\n      disabled: true\n    - domain: other.test\n      type: weird\n";
        let certs = collection_certificates(&collection(config));
        assert_eq!(certs.len(), 2);
        assert_eq!(
            (certs[0].kind.as_str(), certs[0].cert_file_path.as_str(), certs[0].key_file_path.as_str()),
            ("cert", "certs/c.pem", "certs/k.pem")
        );
        assert_eq!(certs[0].passphrase, "pw");
        assert_eq!(
            (certs[1].kind.as_str(), certs[1].pfx_file_path.as_str(), certs[1].disabled),
            ("pfx", "certs/c.p12", true)
        );
    }

    #[test]
    fn ef_req_04_the_first_active_matching_certificate_wins_collection_before_host() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("certs")).unwrap();
        for (name, content) in
            [("c.pem", "CERT-COLLECTION"), ("k.pem", "KEY-COLLECTION"), ("g.pem", "CERT-HOST"), ("gk.pem", "KEY-HOST")]
        {
            fs::write(dir.path().join("certs").join(name), content).unwrap();
        }
        let config = "  clientCertificates:\n    - domain: skipped.test\n      type: pem\n      certificateFilePath: certs/g.pem\n      privateKeyFilePath: certs/gk.pem\n      disabled: true\n    - domain: api.test\n      type: pem\n      certificateFilePath: certs/c.pem\n      privateKeyFilePath: certs/k.pem\n";
        let host = ClientCertificate {
            domain: "api.test".into(),
            kind: "cert".into(),
            cert_file_path: "certs/g.pem".into(),
            key_file_path: "certs/gk.pem".into(),
            ..ClientCertificate::default()
        };
        let prefs = NetworkPrefs { client_certificates: vec![host.clone()], ..NetworkPrefs::default() };

        let picked = |url: &str, prefs: &NetworkPrefs| {
            client_identity(dir.path(), &collection(config), prefs, url, &mut keep)
                .unwrap()
                .map(|i| String::from_utf8(i.cert_pem).unwrap())
        };
        assert_eq!(picked("https://api.test/x", &prefs).as_deref(), Some("CERT-COLLECTION"));
        assert_eq!(picked("https://other.test/x", &prefs), None);
        let host_only = NetworkPrefs {
            client_certificates: vec![ClientCertificate { domain: "other.test".into(), ..host }],
            ..NetworkPrefs::default()
        };
        assert_eq!(picked("https://other.test/x", &host_only).as_deref(), Some("CERT-HOST"));
    }

    #[test]
    fn ef_req_04_certificate_paths_are_relative_to_the_collection_and_problems_are_explained() {
        let dir = tempfile::tempdir().unwrap();
        let entry = |kind: &str, cert: &str, key: &str| ClientCertificate {
            domain: "api.test".into(),
            kind: kind.into(),
            cert_file_path: cert.into(),
            key_file_path: key.into(),
            pfx_file_path: "c.p12".into(),
            ..ClientCertificate::default()
        };
        let try_with = |entry: ClientCertificate| {
            let prefs = NetworkPrefs { client_certificates: vec![entry], ..NetworkPrefs::default() };
            client_identity(dir.path(), &Map::default(), &prefs, "https://api.test/", &mut keep)
        };
        assert!(try_with(entry("cert", "absent.pem", "absent-key.pem")).unwrap_err().to_string().contains("illisible"));
        assert!(try_with(entry("pfx", "", "")).unwrap_err().to_string().contains("PKCS#12"));
        assert!(try_with(entry("cert", "", "")).unwrap_err().to_string().contains("fichier du certificat"));
        fs::write(dir.path().join("encrypted.pem"), "-----BEGIN ENCRYPTED PRIVATE KEY-----\n").unwrap();
        fs::write(dir.path().join("c.pem"), "x").unwrap();
        assert!(try_with(entry("cert", "c.pem", "encrypted.pem")).unwrap_err().to_string().contains("chiffrée"));
    }

    #[test]
    fn ef_req_04_the_custom_authority_file_must_exist_and_a_blank_one_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let prefs = |file: &str| NetworkPrefs { ca_file: Some(file.into()), ..NetworkPrefs::default() };
        assert!(extra_roots(&prefs(&dir.path().join("absent.pem").display().to_string()))
            .unwrap_err()
            .to_string()
            .contains("introuvable"));
        let blank = dir.path().join("blank.pem");
        fs::write(&blank, "  \n").unwrap();
        assert!(extra_roots(&prefs(&blank.display().to_string())).unwrap().is_empty());
        let ca = dir.path().join("ca.pem");
        fs::write(&ca, "PEM").unwrap();
        assert_eq!(extra_roots(&prefs(&ca.display().to_string())).unwrap(), vec![b"PEM".to_vec()]);
        assert!(extra_roots(&NetworkPrefs::default()).unwrap().is_empty());
        assert!(extra_roots(&prefs("  ")).unwrap().is_empty());
    }

    #[test]
    fn ef_req_04_turning_verification_off_skips_loading_the_custom_authority() {
        let prefs =
            NetworkPrefs { verify_tls: false, ca_file: Some("/n/existe/pas.pem".into()), ..NetworkPrefs::default() };
        let network =
            resolve(Path::new("."), &Map::default(), &prefs, "https://a/", Redirects::default(), &mut keep, &no_env)
                .unwrap();
        assert!(!network.tls.verify && network.tls.extra_roots.is_empty());
    }

    #[test]
    fn ef_req_04_prefs_are_saved_in_the_data_directory_and_read_back() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(NetworkPrefs::load(dir.path()), NetworkPrefs::default(), "rien d'enregistré : les défauts");
        let prefs = NetworkPrefs {
            verify_tls: false,
            ca_file: Some("/certs/ca.pem".into()),
            keep_default_roots: false,
            proxy: ProxyPref {
                mode: ProxyMode::Manual,
                config: ProxyConfig {
                    protocol: "socks5".into(),
                    hostname: "p.test".into(),
                    port: "1080".into(),
                    ..ProxyConfig::default()
                },
            },
            ..NetworkPrefs::default()
        };
        prefs.save(dir.path()).unwrap();
        assert_eq!(NetworkPrefs::load(dir.path()), prefs);
        fs::write(dir.path().join(PREFS_FILE), "{ pas du json").unwrap();
        assert_eq!(NetworkPrefs::load(dir.path()), NetworkPrefs::default(), "un fichier illisible vaut les défauts");
    }

    #[test]
    fn ef_req_04_a_manual_proxy_needs_a_known_protocol_a_host_and_a_valid_port() {
        let manual = |protocol: &str, hostname: &str, port: &str| NetworkPrefs {
            proxy: ProxyPref {
                mode: ProxyMode::Manual,
                config: ProxyConfig {
                    protocol: protocol.into(),
                    hostname: hostname.into(),
                    port: port.into(),
                    ..ProxyConfig::default()
                },
            },
            ..NetworkPrefs::default()
        };
        assert!(manual("http", "p.test", "8080").validate().is_ok());
        assert!(manual("", "p.test", "").validate().is_ok(), "protocole et port vides : http et son port");
        assert!(manual("ftp", "p.test", "1").validate().unwrap_err().contains("protocole"));
        assert!(manual("http", "  ", "1").validate().unwrap_err().contains("nom d'hôte"));
        for port in ["0", "65536", "huit", "-1"] {
            assert!(manual("http", "p.test", port).validate().unwrap_err().contains("port"), "{port}");
        }
        let off = NetworkPrefs {
            proxy: ProxyPref { mode: ProxyMode::Off, ..manual("ftp", "", "x").proxy },
            ..NetworkPrefs::default()
        };
        assert!(off.validate().is_ok(), "un proxy désactivé n'est pas vérifié");
    }

    #[test]
    fn ef_req_04_prefs_round_trip_without_the_proxy_password_or_the_noproxy_flag() {
        let prefs = NetworkPrefs {
            no_proxy: true,
            proxy: ProxyPref {
                mode: ProxyMode::Manual,
                config: ProxyConfig {
                    hostname: "p.test".into(),
                    username: "alice".into(),
                    password: "s3cret".into(),
                    ..ProxyConfig::default()
                },
            },
            ..NetworkPrefs::default()
        };
        let json = serde_json::to_string(&prefs).unwrap();
        assert!(!json.contains("s3cret") && !json.contains("noProxy"), "{json}");
        let back: NetworkPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.proxy.config.username, "alice");
        assert!(back.proxy.config.password.is_empty() && !back.no_proxy);
        assert_eq!(serde_json::from_str::<NetworkPrefs>("{}").unwrap(), NetworkPrefs::default());
    }
}
