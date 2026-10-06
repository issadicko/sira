//! JavaScript : `fetch`, dans le navigateur ou Node 18 et plus.

use crate::quote::{string, Dialect};
use crate::{Body, PartValue, Snippet};

fn js(text: &str) -> String {
    string(text, Dialect::JavaScript)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn fetch(s: &Snippet) -> String {
    let mut out = String::new();
    for note in &s.notes {
        out.push_str(&format!("// {note}\n"));
    }
    let parts = match &s.body {
        Body::Multipart(parts) => Some(parts),
        _ => None,
    };
    if parts.is_some_and(|p| p.iter().any(|p| matches!(p.value, PartValue::File(_)))) {
        out.push_str("const fs = require('node:fs');\n\n");
    }
    if let Some(parts) = parts {
        out.push_str("const form = new FormData();\n");
        for part in parts {
            let name = js(&part.name);
            match (&part.value, &part.content_type) {
                (PartValue::Text(value), None) => out.push_str(&format!("form.append({name}, {});\n", js(value))),
                (PartValue::Text(value), Some(kind)) => out.push_str(&format!(
                    "form.append({name}, new Blob([{}], {{ type: {} }}));\n",
                    js(value),
                    js(kind)
                )),
                (PartValue::File(path), kind) => {
                    let options = kind.as_ref().map(|k| format!(", {{ type: {} }}", js(k))).unwrap_or_default();
                    out.push_str(&format!(
                        "form.append({name}, new Blob([fs.readFileSync({})]{options}), {});\n",
                        js(path),
                        js(file_name(path))
                    ));
                }
            }
        }
        out.push('\n');
    }

    out.push_str("const options = {\n");
    out.push_str(&format!("  method: {}", js(&s.method_upper())));
    if !s.headers.is_empty() {
        out.push_str(",\n  headers: ");
        if s.has_duplicate_headers() {
            let pairs: Vec<String> = s.headers.iter().map(|(k, v)| format!("    [{}, {}]", js(k), js(v))).collect();
            out.push_str(&format!("[\n{}\n  ]", pairs.join(",\n")));
        } else {
            let pairs: Vec<String> = s.headers.iter().map(|(k, v)| format!("    {}: {}", js(k), js(v))).collect();
            out.push_str(&format!("{{\n{}\n  }}", pairs.join(",\n")));
        }
    }
    match &s.body {
        Body::None => {}
        Body::Raw(text) => out.push_str(&format!(",\n  body: {}", js(text))),
        Body::Multipart(_) => out.push_str(",\n  body: form"),
    }
    out.push_str("\n};\n\n");
    out.push_str(&format!(
        "fetch({}, options)\n  .then((response) => response.text())\n  .then((body) => console.log(body))\n  .catch((error) => console.error(error));\n",
        js(&s.url)
    ));
    out
}
