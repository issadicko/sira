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

/// L'adresse suivie de la query reconstruite à partir des paramètres query activés.
pub fn from_params(address: &str, params: &[Param]) -> String {
    let query: Vec<String> = query_view(params)
        .into_iter()
        .map(|(name, value)| if value.is_empty() { name.to_owned() } else { format!("{name}={value}") })
        .collect();
    if query.is_empty() {
        address.to_owned()
    } else {
        format!("{address}?{}", query.join("&"))
    }
}
