//! Python : `requests`.

use crate::quote::{string, Dialect};
use crate::{Auth, Body, PartValue, Snippet};

fn py(text: &str) -> String {
    string(text, Dialect::Python)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn requests(s: &Snippet) -> String {
    let mut out = String::new();
    for note in &s.notes {
        out.push_str(&format!("# {note}\n"));
    }
    if s.has_duplicate_headers() {
        out.push_str(
            "# Un nom d'en-tête répété n'est gardé qu'une fois (dictionnaire) : la dernière valeur l'emporte.\n",
        );
    }
    if let Auth::Aws { .. } = s.auth {
        out.push_str("# AWS Signature V4 : signe la requête avec une bibliothèque (par exemple requests-aws4auth).\n");
    }
    out.push_str("import requests\n");
    if let Auth::Digest { .. } = s.auth {
        out.push_str("from requests.auth import HTTPDigestAuth\n");
    }
    out.push_str(&format!("\nurl = {}\n\n", py(&s.url)));

    let mut call = vec![py(&s.method_upper()), "url".to_owned()];
    match &s.body {
        Body::None => {}
        Body::Raw(text) => {
            // `requests` mesure une chaîne en caractères, pas en octets : un corps non ASCII est encodé d'avance.
            let encoded = if text.is_ascii() { "" } else { ".encode(\"utf-8\")" };
            out.push_str(&format!("payload = {}{encoded}\n", py(text)));
            call.push("data=payload".into());
        }
        Body::Multipart(parts) => {
            let items: Vec<String> = parts
                .iter()
                .map(|part| match (&part.value, &part.content_type) {
                    (PartValue::Text(value), None) => format!("    ({}, (None, {}))", py(&part.name), py(value)),
                    (PartValue::Text(value), Some(kind)) => {
                        format!("    ({}, (None, {}, {}))", py(&part.name), py(value), py(kind))
                    }
                    (PartValue::File(path), kind) => {
                        let kind = kind.as_ref().map(|k| format!(", {}", py(k))).unwrap_or_default();
                        format!("    ({}, ({}, open({}, \"rb\"){kind}))", py(&part.name), py(file_name(path)), py(path))
                    }
                })
                .collect();
            out.push_str(&format!("files = [\n{}\n]\n", items.join(",\n")));
            call.push("files=files".into());
        }
    }
    if !s.headers.is_empty() {
        let mut unique: Vec<(&String, &String)> = Vec::new();
        for (k, v) in &s.headers {
            match unique.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(k)) {
                Some(slot) => *slot = (k, v),
                None => unique.push((k, v)),
            }
        }
        let lines: Vec<String> = unique.iter().map(|(k, v)| format!("    {}: {}", py(k), py(v))).collect();
        out.push_str(&format!("headers = {{\n{}\n}}\n", lines.join(",\n")));
        call.push("headers=headers".into());
    }
    if let Auth::Digest { username, password } = &s.auth {
        call.push(format!("auth=HTTPDigestAuth({}, {})", py(username), py(password)));
    }
    out.push_str(&format!("\nresponse = requests.request({})\n\nprint(response.text)\n", call.join(", ")));
    out
}
