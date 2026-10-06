//! Dart : `package:http`.

use crate::quote::{string, Dialect};
use crate::{Auth, Body, PartValue, Snippet};

fn dart(text: &str) -> String {
    string(text, Dialect::Dart)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn http(s: &Snippet) -> String {
    let mut out = String::new();
    for note in &s.notes {
        out.push_str(&format!("// {note}\n"));
    }
    match &s.auth {
        Auth::Digest { .. } => {
            out.push_str("// Digest : package:http ne le gère pas, utilise un client qui le fait (http_auth).\n")
        }
        Auth::Aws { .. } => {
            out.push_str("// AWS Signature V4 : signe la requête avec un paquet dédié (aws_signature_v4).\n")
        }
        Auth::None => {}
    }
    if s.has_duplicate_headers() {
        out.push_str("// Un nom d'en-tête répété n'est gardé qu'une fois (Map) : la dernière valeur l'emporte.\n");
    }
    for (name, value) in s.headers.iter().filter(|(_, v)| !v.is_ascii()) {
        let _ = value;
        out.push_str(&format!("// L'en-tête {name} contient un caractère hors ASCII : dart:io le refuse.\n"));
    }
    let multipart = matches!(s.body, Body::Multipart(_));
    let typed = matches!(&s.body, Body::Multipart(parts) if parts.iter().any(|p| p.content_type.is_some()));
    out.push_str("import 'package:http/http.dart' as http;\n");
    if typed {
        out.push_str("import 'package:http_parser/http_parser.dart';\n");
    }
    out.push_str("\nFuture<void> main() async {\n");
    let class = if multipart { "MultipartRequest" } else { "Request" };
    out.push_str(&format!(
        "  final request = http.{class}({}, Uri.parse({}));\n",
        dart(&s.method_upper()),
        dart(&s.url)
    ));
    if !s.headers.is_empty() {
        let mut unique: Vec<(&String, &String)> = Vec::new();
        for (k, v) in &s.headers {
            match unique.iter_mut().find(|(name, _)| name.eq_ignore_ascii_case(k)) {
                Some(slot) => *slot = (k, v),
                None => unique.push((k, v)),
            }
        }
        out.push_str("  request.headers.addAll({\n");
        for (k, v) in unique {
            out.push_str(&format!("    {}: {},\n", dart(k), dart(v)));
        }
        out.push_str("  });\n");
    }
    match &s.body {
        Body::None => {}
        Body::Raw(text) => out.push_str(&format!("  request.body = {};\n", dart(text))),
        Body::Multipart(parts) => {
            for part in parts {
                let kind = part.content_type.as_deref().map(|k| format!("contentType: MediaType.parse({})", dart(k)));
                match &part.value {
                    PartValue::Text(value) => match kind {
                        None => out.push_str(&format!("  request.fields[{}] = {};\n", dart(&part.name), dart(value))),
                        Some(kind) => out.push_str(&format!(
                            "  request.files.add(http.MultipartFile.fromString({}, {}, {kind}));\n",
                            dart(&part.name),
                            dart(value)
                        )),
                    },
                    PartValue::File(path) => {
                        let kind = kind.map(|k| format!(", {k}")).unwrap_or_default();
                        out.push_str(&format!(
                            "  request.files.add(await http.MultipartFile.fromPath({}, {}, filename: {}{kind}));\n",
                            dart(&part.name),
                            dart(path),
                            dart(file_name(path))
                        ));
                    }
                }
            }
        }
    }
    out.push_str(
        "\n  final response = await http.Response.fromStream(await request.send());\n  print(response.statusCode);\n  print(response.body);\n}\n",
    );
    out
}
