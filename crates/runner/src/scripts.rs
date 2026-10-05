use std::path::Path;

use serde_json::Value;
use xc_core::request::Script;
use xc_core::vars::Context;
use xc_core::yaml::Map;
use xc_core::RequestDoc;

pub const BEFORE_REQUEST: &str = "before-request";
pub const AFTER_RESPONSE: &str = "after-response";
pub const TESTS: &str = "tests";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    Sandwich,
    Sequential,
}

/// `extensions.bruno.scripts.flow` de la collection : `sandwich` par défaut.
pub fn flow_of(collection: &Map) -> Flow {
    let flow = collection
        .map("extensions")
        .and_then(|e| e.map("bruno"))
        .and_then(|b| b.map("scripts"))
        .and_then(|s| s.str("flow"));
    if flow == Some("sequential") {
        Flow::Sequential
    } else {
        Flow::Sandwich
    }
}

struct Segment {
    code: String,
    file: String,
}

fn scripts_of(map: &Map, kind: &str) -> Vec<String> {
    let section = map.map("request");
    let scripts = section.map(|r| r.seq("scripts")).unwrap_or_default();
    scripts
        .iter()
        .filter_map(|s| s.as_map())
        .filter(|s| s.str("type") == Some(kind))
        .filter_map(|s| s.get("code").and_then(|c| c.scalar()))
        .collect()
}

fn segment(code: String, file: &Path) -> Option<Segment> {
    (!code.trim().is_empty()).then(|| Segment { code, file: file.display().to_string() })
}

/// Le script d'une phase (`before-request`, `after-response` ou `tests`) : un segment par fichier (collection,
/// dossiers, requête), chacun dans sa fonction asynchrone avec son `__dirname` et son `__filename`, comme Bruno. Avant
/// la requête l'ordre va de la collection à la requête ; après, `sandwich` le renverse (requête, dossiers du plus
/// proche au plus éloigné, collection) et `sequential` le garde.
pub fn merged_script(ctx: &Context, root: &Path, request_path: &str, doc: &RequestDoc, kind: &str) -> String {
    let mut segments: Vec<Segment> = Vec::new();
    let collection_file = root.join(xc_core::collection::COLLECTION_FILE);
    segments.extend(scripts_of(&ctx.collection, kind).into_iter().filter_map(|c| segment(c, &collection_file)));
    for (dir, tree) in &ctx.folders {
        let file = root.join(dir).join(xc_core::collection::FOLDER_FILE);
        segments.extend(scripts_of(tree, kind).into_iter().filter_map(|c| segment(c, &file)));
    }
    let request_file = root.join(request_path);
    let own = doc.scripts.iter().filter(|s: &&Script| s.kind == kind);
    segments.extend(own.filter_map(|s| segment(s.code.clone(), &request_file)));

    if kind != BEFORE_REQUEST && flow_of(&ctx.collection) == Flow::Sandwich {
        segments.reverse();
    }
    segments
        .into_iter()
        .map(|s| {
            let dir = Path::new(&s.file).parent().map(|p| p.display().to_string()).unwrap_or_default();
            format!(
                "await (async (__dirname, __filename) => {{\n{}\n}})({}, {});",
                s.code,
                Value::String(dir),
                Value::String(s.file)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
