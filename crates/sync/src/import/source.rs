//! Source d'une spec OpenAPI : lecture d'un fichier ou téléchargement d'une URL, et référence stockée dans `source.yml`.

use std::fs;
use std::io::Read;
use std::path::{Component, Path};
use std::time::Duration;

use url::Url;
use xc_engine::{HttpRequest, HttpResponse, Network, Redirects};

use super::ImportError;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 5;
const MAX_SPEC_BYTES: u64 = 32 << 20;
const ACCEPT: &str = "application/json, application/yaml, */*";
const SECRET_NAMES: [&str; 7] = ["auth", "authorization", "code", "pass", "passwd", "pwd", "sig"];
const SECRET_SUFFIXES: [&str; 6] = ["token", "key", "secret", "password", "signature", "credential"];

fn source_error(message: impl std::fmt::Display) -> ImportError {
    ImportError::Source(message.to_string())
}

pub fn is_url(source: &str) -> bool {
    let lower = source.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Contenu de la spec : un fichier est lu tel quel, une URL http(s) est lue par GET (cinq redirections au plus,
/// trente secondes par échange). Dans les deux cas la spec ne dépasse pas 32 Mo. Aucun autre appel réseau.
pub async fn fetch_spec(source: &str) -> Result<String, ImportError> {
    let source = source.trim();
    if !is_url(source) {
        let path = source.to_owned();
        return tokio::task::spawn_blocking(move || read_file(&path)).await.map_err(source_error)?;
    }
    let mut url = Url::parse(source).map_err(|e| source_error(format!("URL invalide : {e}")))?;
    for _ in 0..=MAX_REDIRECTS {
        let request = HttpRequest {
            method: "GET".into(),
            url: url.to_string(),
            headers: vec![("Accept".into(), ACCEPT.into())],
            body: None,
            timeout: TIMEOUT,
            max_response_body: Some(MAX_SPEC_BYTES),
            network: Network { redirects: Redirects::none() },
        };
        let response = xc_engine::send(request).await.map_err(|e| source_error(format!("{url} : {e}")))?;
        if (200..300).contains(&response.status) {
            return Ok(xc_engine::lossy_text(response.body));
        }
        match redirection(&url, &response)? {
            Some(next) => url = next,
            None => {
                let reason = format!("{} {}", response.status, response.reason);
                return Err(source_error(format!("{url} : le serveur a répondu {}", reason.trim())));
            }
        }
    }
    Err(source_error(format!("plus de {MAX_REDIRECTS} redirections à partir de {source}")))
}

fn read_file(path: &str) -> Result<String, ImportError> {
    let failed = |e: std::io::Error| source_error(format!("{path} : {e}"));
    let mut text = String::new();
    fs::File::open(path).map_err(failed)?.take(MAX_SPEC_BYTES + 1).read_to_string(&mut text).map_err(failed)?;
    if text.len() as u64 > MAX_SPEC_BYTES {
        return Err(source_error(format!("{path} : fichier de plus de {} Mo", MAX_SPEC_BYTES >> 20)));
    }
    Ok(text)
}

fn redirection(from: &Url, response: &HttpResponse) -> Result<Option<Url>, ImportError> {
    if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
        return Ok(None);
    }
    let location = response.headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("location"));
    let Some((_, location)) = location else { return Ok(None) };
    let next = from.join(location.trim()).map_err(|e| source_error(format!("redirection invalide : {e}")))?;
    match (from.scheme(), next.scheme()) {
        ("https", "http") => Err(source_error("redirection refusée : de HTTPS vers HTTP")),
        (_, "http" | "https") => Ok(Some(next)),
        (_, other) => Err(source_error(format!("redirection vers un schéma non pris en charge : {other}"))),
    }
}

/// Valeur de `source` de `source.yml` : l'URL sans identifiants ni paramètres qui ressemblent à des secrets (le reste
/// tel quel), ou le chemin du fichier relatif à la racine de la collection, avec des `/` (absolu quand les deux
/// chemins ne partagent aucun dossier, par exemple `/tmp` et `/Users`, ou deux disques Windows).
pub fn source_value(source: &str, root: &Path) -> String {
    let source = source.trim();
    if is_url(source) {
        return without_secrets(source);
    }
    let absolute =
        |path: &Path| fs::canonicalize(path).or_else(|_| std::path::absolute(path)).unwrap_or(path.to_owned());
    portable(&relative_path(&absolute(root), &absolute(Path::new(source))))
}

fn without_secrets(source: &str) -> String {
    let Ok(mut url) = Url::parse(source) else { return source.to_owned() };
    let has_userinfo = !url.username().is_empty() || url.password().is_some();
    let pairs: Vec<&str> = url.query().map(|query| query.split('&').collect()).unwrap_or_default();
    let kept: Vec<&str> = pairs.iter().copied().filter(|pair| !is_secret(pair)).collect();
    if !has_userinfo && kept.len() == pairs.len() {
        return source.to_owned();
    }
    let query = (!kept.is_empty()).then(|| kept.join("&"));
    url.set_query(query.as_deref());
    url.set_username("").ok();
    url.set_password(None).ok();
    url.into()
}

/// Paramètre `nom=valeur` dont le nom (sans casse, `-` comme `_`) est ou finit comme celui d'un secret.
fn is_secret(pair: &str) -> bool {
    let raw = pair.split('=').next().unwrap_or_default();
    let name = url::form_urlencoded::parse(raw.as_bytes()).next().map_or(raw.into(), |(name, _)| name);
    let name = name.to_ascii_lowercase().replace('-', "_");
    SECRET_NAMES.contains(&name.as_str()) || SECRET_SUFFIXES.iter().any(|suffix| name.ends_with(suffix))
}

fn relative_path(base: &Path, target: &Path) -> std::path::PathBuf {
    let (base, target): (Vec<Component>, Vec<Component>) = (base.components().collect(), target.components().collect());
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    if !base[..common].iter().any(|c| matches!(c, Component::Normal(_))) {
        return target.iter().collect();
    }
    let up = std::iter::repeat_n(Component::ParentDir, base.len() - common);
    up.chain(target[common..].iter().copied()).collect()
}

fn portable(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redirect_to(location: &str) -> HttpResponse {
        HttpResponse {
            status: 302,
            reason: "Found".into(),
            http_version: "HTTP/1.1".into(),
            remote_addr: String::new(),
            headers: vec![("Location".into(), location.into())],
            body: Vec::new(),
            timings: Default::default(),
            url: String::new(),
            redirects: Vec::new(),
        }
    }

    fn follow(from: &str, location: &str) -> Result<Option<Url>, ImportError> {
        redirection(&Url::parse(from).unwrap(), &redirect_to(location))
    }

    #[test]
    fn ef_imp_02_a_redirection_from_https_to_http_is_refused() {
        let error = follow("https://api.test/spec.yaml", "http://api.test/spec.yaml").unwrap_err();
        assert!(error.is_input() && error.to_string().contains("de HTTPS vers HTTP"), "{error}");
        let next = follow("https://api.test/spec.yaml", "//cdn.test/spec.yaml").unwrap();
        assert_eq!(next.unwrap().as_str(), "https://cdn.test/spec.yaml");
    }

    #[test]
    fn ef_imp_02_other_redirections_between_http_and_https_are_followed() {
        for (from, location, to) in [
            ("http://api.test/a", "https://api.test/b", "https://api.test/b"),
            ("http://api.test/a", "http://api.test/b", "http://api.test/b"),
            ("https://api.test/a", "/b", "https://api.test/b"),
        ] {
            assert_eq!(follow(from, location).unwrap().unwrap().as_str(), to);
        }
    }

    #[cfg(unix)]
    #[test]
    fn ef_imp_02_spec_path_is_relative_only_when_a_folder_is_shared() {
        let root = Path::new("/home/ada/api/collection");
        assert_eq!(
            relative_path(root, Path::new("/home/ada/api/specs/openapi.yaml")),
            Path::new("../specs/openapi.yaml")
        );
        assert_eq!(relative_path(root, Path::new("/tmp/openapi.yaml")), Path::new("/tmp/openapi.yaml"));
    }
}
