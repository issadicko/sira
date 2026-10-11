use std::path::Path;

use xc_core::{open_collection, read_docs, read_request, CoreError, TreeItem};

use super::postman::sorted_for_export;
use super::Exported;

const MAX_LEVEL: usize = 6;

/// La documentation de la collection en un seul fichier Markdown : le titre et la documentation de la collection,
/// puis chaque dossier et chaque requête (méthode et adresse, puis sa documentation) dans l'ordre de l'arbre.
pub fn markdown(root: &Path) -> Result<Exported, CoreError> {
    let info = open_collection(root)?;
    let mut out = String::new();
    let mut issues = Vec::new();
    let title = if info.name.is_empty() { "Collection" } else { info.name.as_str() };
    out.push_str(&format!("# {title}\n"));
    push_text(&mut out, &read_docs(root, "")?);
    section(root, &info.items, 2, &mut out, &mut issues)?;
    Ok(Exported { text: out, issues })
}

fn section(
    root: &Path,
    items: &[TreeItem],
    level: usize,
    out: &mut String,
    issues: &mut Vec<String>,
) -> Result<(), CoreError> {
    let marks = "#".repeat(level.min(MAX_LEVEL));
    for item in sorted_for_export(items) {
        match item {
            TreeItem::Folder { path, name, children, .. } => {
                out.push_str(&format!("\n{marks} {}\n", if name.is_empty() { "Dossier sans nom" } else { name }));
                push_text(out, &read_docs(root, path)?);
                section(root, children, level + 1, out, issues)?;
            }
            TreeItem::Request { path, name, error: Some(error), .. } => {
                issues.push(format!("{name} ({path}) : requête illisible, ignorée ({error})"));
            }
            TreeItem::Request { path, name, .. } => {
                let doc = read_request(root, path)?;
                out.push_str(&format!("\n{marks} {name}\n"));
                out.push_str(&format!("\n`{}`\n", summary(&doc.request_type, &doc.method, &doc.url)));
                push_text(out, doc.docs.as_deref().unwrap_or_default());
            }
        }
    }
    Ok(())
}

fn summary(kind: &str, method: &str, url: &str) -> String {
    let label = match kind {
        "http" => method.to_uppercase(),
        "graphql" => "GRAPHQL".to_owned(),
        "grpc" => "GRPC".to_owned(),
        "websocket" => "WS".to_owned(),
        other => other.to_uppercase(),
    };
    format!("{label} {url}").trim().to_owned()
}

fn push_text(out: &mut String, text: &str) {
    let text = text.trim_matches(['\n', '\r']);
    if !text.is_empty() {
        out.push('\n');
        out.push_str(text);
        out.push('\n');
    }
}
