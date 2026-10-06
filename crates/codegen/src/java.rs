//! Java : OkHttp.

use crate::quote::{string, Dialect};
use crate::{Auth, Body, PartValue, Snippet};

fn java(text: &str) -> String {
    string(text, Dialect::Java)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// OkHttp refuse un caractère hors ASCII dans un en-tête, sauf par `addUnsafeNonAscii`.
pub(crate) fn has_non_ascii_header(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(_, value)| value.chars().any(|c| c != '\t' && !(' '..='~').contains(&c)))
}

/// Les méthodes pour lesquelles OkHttp refuse un corps absent.
pub(crate) fn needs_body(method: &str) -> bool {
    matches!(method, "POST" | "PUT" | "PATCH" | "PROPPATCH" | "REPORT")
}

pub fn okhttp(s: &Snippet) -> String {
    let mut out = String::new();
    for note in &s.notes {
        out.push_str(&format!("// {note}\n"));
    }
    match &s.auth {
        Auth::Digest { .. } => out.push_str("// Digest : ajoute un Authenticator OkHttp (le défi vient du serveur).\n"),
        Auth::Aws { .. } => out.push_str("// AWS Signature V4 : signe la requête avec le SDK AWS.\n"),
        Auth::None => {}
    }
    let method = s.method_upper();
    let content_type = s.content_type();
    let multipart = matches!(s.body, Body::Multipart(_));
    let has_files =
        matches!(&s.body, Body::Multipart(parts) if parts.iter().any(|p| matches!(p.value, PartValue::File(_))));

    let unsafe_headers = has_non_ascii_header(&s.headers);
    if unsafe_headers {
        out.push_str("import okhttp3.Headers;\n");
    }
    out.push_str("import okhttp3.MediaType;\n");
    if multipart {
        out.push_str("import okhttp3.MultipartBody;\n");
    }
    out.push_str("import okhttp3.OkHttpClient;\nimport okhttp3.Request;\nimport okhttp3.RequestBody;\nimport okhttp3.Response;\n");
    if has_files {
        out.push_str("import java.io.File;\n");
    }
    out.push_str("\npublic class Main {\n    public static void main(String[] args) throws Exception {\n        OkHttpClient client = new OkHttpClient();\n\n");

    let body = match &s.body {
        Body::None if needs_body(&method) => {
            out.push_str("        RequestBody body = RequestBody.create(\"\", null);\n\n");
            Some("body")
        }
        Body::None => None,
        Body::Raw(text) => {
            match content_type {
                Some(kind) => {
                    out.push_str(&format!("        MediaType mediaType = MediaType.parse({});\n", java(kind)))
                }
                None => out.push_str("        MediaType mediaType = null;\n"),
            }
            out.push_str(&format!("        RequestBody body = RequestBody.create({}, mediaType);\n\n", java(text)));
            Some("body")
        }
        Body::Multipart(parts) => {
            out.push_str(
                "        RequestBody body = new MultipartBody.Builder()\n            .setType(MultipartBody.FORM)\n",
            );
            for part in parts {
                let kind = part
                    .content_type
                    .as_deref()
                    .map(|k| format!("MediaType.parse({})", java(k)))
                    .unwrap_or_else(|| "null".into());
                match &part.value {
                    PartValue::Text(value) if part.content_type.is_none() => {
                        out.push_str(&format!("            .addFormDataPart({}, {})\n", java(&part.name), java(value)))
                    }
                    PartValue::Text(value) => out.push_str(&format!(
                        "            .addFormDataPart({}, null, RequestBody.create({}, {kind}))\n",
                        java(&part.name),
                        java(value)
                    )),
                    PartValue::File(path) => out.push_str(&format!(
                        "            .addFormDataPart({}, {}, RequestBody.create(new File({}), {kind}))\n",
                        java(&part.name),
                        java(file_name(path)),
                        java(path)
                    )),
                }
            }
            out.push_str("            .build();\n\n");
            Some("body")
        }
    };

    out.push_str(&format!("        Request request = new Request.Builder()\n            .url({})\n", java(&s.url)));
    out.push_str(&match (method.as_str(), body) {
        ("GET", None) => "            .get()\n".to_owned(),
        ("HEAD", None) => "            .head()\n".to_owned(),
        (_, Some(body)) => format!("            .method({}, {body})\n", java(&method)),
        (_, None) => format!("            .method({}, null)\n", java(&method)),
    });
    // OkHttp reprend le `Content-Type` du corps : l'écrire en plus ferait doublon.
    let headers: Box<dyn Iterator<Item = &(String, String)>> =
        if body.is_some() { Box::new(s.headers_without_content_type()) } else { Box::new(s.headers.iter()) };
    let headers: Vec<_> = headers.collect();
    if unsafe_headers && !headers.is_empty() {
        out.push_str("            .headers(new Headers.Builder()\n");
        for (name, value) in &headers {
            let add = if value.is_ascii() { "add" } else { "addUnsafeNonAscii" };
            out.push_str(&format!("                .{add}({}, {})\n", java(name), java(value)));
        }
        out.push_str("                .build())\n");
    } else {
        for (name, value) in headers {
            out.push_str(&format!("            .addHeader({}, {})\n", java(name), java(value)));
        }
    }
    out.push_str(
        "            .build();\n\n        try (Response response = client.newCall(request).execute()) {\n            System.out.println(response.code());\n            System.out.println(response.body().string());\n        }\n    }\n}\n",
    );
    out
}
