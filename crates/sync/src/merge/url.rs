//! Adresse et query d'une URL, avec la même sémantique que `urlFromParams` et `paramsFromUrl` de l'interface
//! (`app/src/app/core/url.ts`).

use xc_core::{Param, ParamKind};

/// L'URL sans sa query.
pub fn address(url: &str) -> &str {
    url.find('?').map_or(url, |i| &url[..i])
}

/// La query de l'URL, `?` compris, ou le texte vide.
pub fn query_suffix(url: &str) -> &str {
    url.find('?').map_or("", |i| &url[i..])
}

/// Noms des segments `:nom` de l'adresse, dans l'ordre.
pub fn path_names(address: &str) -> Vec<String> {
    let names = address.split('/').filter(|segment| segment.len() > 1).filter_map(|segment| segment.strip_prefix(':'));
    names.map(str::to_owned).collect()
}

/// Paramètres query activés et nommés : ce qui apparaît dans l'URL.
pub fn query_view(params: &[Param]) -> Vec<(&str, &str)> {
    let enabled = params.iter().filter(|p| p.kind == ParamKind::Query && p.enabled && !p.name.is_empty());
    enabled.map(|p| (p.name.as_str(), p.value.as_str())).collect()
}

fn segment(name: &str, value: &str) -> String {
    if value.is_empty() {
        name.to_owned()
    } else {
        format!("{name}={value}")
    }
}

/// L'adresse suivie de la query de l'équipe mise à jour : le segment d'un paramètre retiré ou désactivé disparaît,
/// celui d'un paramètre modifié est remplacé sur place, les paramètres activés que la query ne contient pas sont
/// ajoutés à la fin. Ce qu'aucun paramètre ne représente dans la query est gardé tel quel.
pub fn with_query(address: &str, ours_url: &str, ours: &[Param], merged: &[Param]) -> String {
    let (before, after) = (query_view(ours), query_view(merged));
    let mut gone = difference(&before, &after);
    let mut changed: Vec<Option<(&str, &str)>> = difference(&after, &before).into_iter().map(Some).collect();
    let query = query_suffix(ours_url).strip_prefix('?').unwrap_or_default();
    let mut segments = Vec::new();
    for part in query.split('&').filter(|_| !query.is_empty()) {
        let Some(at) = gone.iter().position(|(name, value)| segment(name, value) == part) else {
            segments.push(part.to_owned());
            continue;
        };
        let (name, _) = gone.remove(at);
        let replacement = changed.iter_mut().find(|pair| pair.is_some_and(|(changed, _)| changed == name));
        segments.extend(replacement.and_then(Option::take).map(|(name, value)| segment(name, value)));
    }
    let mut held = segments.clone();
    for (name, value) in after {
        let wanted = segment(name, value);
        match held.iter().position(|part| *part == wanted) {
            Some(at) => {
                held.remove(at);
            }
            None => segments.push(wanted),
        }
    }
    if segments.is_empty() {
        address.to_owned()
    } else {
        format!("{address}?{}", segments.join("&"))
    }
}

/// Les paires de `list` qui ne sont pas dans `other`, chaque paire de `other` n'en retirant qu'une.
fn difference<'a>(list: &[(&'a str, &'a str)], other: &[(&'a str, &'a str)]) -> Vec<(&'a str, &'a str)> {
    let mut rest = other.to_vec();
    let kept = list.iter().filter(|pair| match rest.iter().position(|known| known == *pair) {
        Some(at) => {
            rest.remove(at);
            false
        }
        None => true,
    });
    kept.copied().collect()
}
