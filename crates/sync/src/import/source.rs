//! Source d'une spec OpenAPI : lecture d'un fichier ou téléchargement d'une URL, et référence stockée dans le snapshot.

use std::fs;
use std::path::{Component, Path};
use std::time::Duration;

use url::Url;
use xc_engine::{HttpRequest, HttpResponse};

use super::ImportError;

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 5;
const ACCEPT: &str = "application/json, application/yaml, */*";

fn source_error(message: impl std::fmt::Display) -> ImportError {
    ImportError::Source(message.to_string())
}

pub fn is_url(source: &str) -> bool {
    let lower = source.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Contenu de la spec : un fichier est lu tel quel, une URL http(s) est lue par GET (cinq redirections au plus,
/// trente secondes par échange). Aucun autre appel réseau.
pub async fn fetch_spec(source: &str) -> Result<String, ImportError> {
    let source = source.trim();
    if !is_url(source) {
        return fs::read_to_string(source).map_err(|e| source_error(format!("{source} : {e}")));
    }
    let mut url = Url::parse(source).map_err(|e| source_error(format!("URL invalide : {e}")))?;
    for _ in 0..=MAX_REDIRECTS {
        let request = HttpRequest {
            method: "GET".into(),
            url: url.to_string(),
            headers: vec![("Accept".into(), ACCEPT.into())],
            body: None,
            timeout: TIMEOUT,
        };
        let response = xc_engine::send(&request).await.map_err(|e| source_error(format!("{url} : {e}")))?;
        if (200..300).contains(&response.status) {
            return Ok(String::from_utf8_lossy(&response.body).into_owned());
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

fn redirection(from: &Url, response: &HttpResponse) -> Result<Option<Url>, ImportError> {
    if !matches!(response.status, 301 | 302 | 303 | 307 | 308) {
        return Ok(None);
    }
    let location = response.headers.iter().find(|(name, _)| name.eq_ignore_ascii_case("location"));
    let Some((_, location)) = location else { return Ok(None) };
    let next = from.join(location.trim()).map_err(|e| source_error(format!("redirection invalide : {e}")))?;
    match next.scheme() {
        "http" | "https" => Ok(Some(next)),
        other => Err(source_error(format!("redirection vers un schéma non pris en charge : {other}"))),
    }
}

/// Valeur de `source` du snapshot : l'URL telle quelle, ou le chemin du fichier relatif à la racine de la
/// collection, avec des `/` (absolu quand les deux chemins n'ont pas de racine commune, comme deux disques Windows).
pub fn source_value(source: &str, root: &Path) -> String {
    let source = source.trim();
    if is_url(source) {
        return source.to_owned();
    }
    let absolute =
        |path: &Path| fs::canonicalize(path).or_else(|_| std::path::absolute(path)).unwrap_or(path.to_owned());
    portable(&relative_path(&absolute(root), &absolute(Path::new(source))))
}

fn relative_path(base: &Path, target: &Path) -> std::path::PathBuf {
    let (base, target): (Vec<Component>, Vec<Component>) = (base.components().collect(), target.components().collect());
    let common = base.iter().zip(&target).take_while(|(a, b)| a == b).count();
    if common == 0 {
        return target.iter().collect();
    }
    let up = std::iter::repeat_n(Component::ParentDir, base.len() - common);
    up.chain(target[common..].iter().copied()).collect()
}

fn portable(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}
