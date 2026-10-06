//! Kotlin : OkHttp.

use crate::java::{has_non_ascii_header, needs_body};
use crate::quote::{string, Dialect};
use crate::{Auth, Body, PartValue, Snippet};

fn kt(text: &str) -> String {
    string(text, Dialect::Kotlin)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
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
    let parts = match &s.body {
        Body::Multipart(parts) => parts.as_slice(),
        _ => &[],
    };
    let has_files = parts.iter().any(|p| matches!(p.value, PartValue::File(_)));
    let has_types = parts.iter().any(|p| p.content_type.is_some());

    let unsafe_headers = has_non_ascii_header(&s.headers);
    let mut imports: Vec<&str> = vec!["okhttp3.OkHttpClient", "okhttp3.Request"];
    if unsafe_headers {
        imports.push("okhttp3.Headers");
    }
    if matches!(s.body, Body::Raw(_)) || has_types || needs_body(&method) && matches!(s.body, Body::None) {
        imports.push("okhttp3.RequestBody.Companion.toRequestBody");
    }
    if matches!(s.body, Body::Raw(_)) && content_type.is_some() || has_types {
        imports.push("okhttp3.MediaType.Companion.toMediaType");
    }
    if multipart {
        imports.push("okhttp3.MultipartBody");
    }
    if has_files {
        imports.push("okhttp3.RequestBody.Companion.asRequestBody");
        imports.push("java.io.File");
    }
    imports.sort_unstable();
    imports.dedup();
    for import in imports {
        out.push_str(&format!("import {import}\n"));
    }
    out.push_str("\nfun main() {\n    val client = OkHttpClient()\n\n");

    let body = match &s.body {
        Body::None if needs_body(&method) => {
            out.push_str("    val body = \"\".toRequestBody(null)\n\n");
            Some("body")
        }
        Body::None => None,
        Body::Raw(text) => {
            let kind = content_type.map(|k| format!("{}.toMediaType()", kt(k))).unwrap_or_else(|| "null".into());
            out.push_str(&format!("    val body = {}.toRequestBody({kind})\n\n", kt(text)));
            Some("body")
        }
        Body::Multipart(parts) => {
            out.push_str("    val body = MultipartBody.Builder()\n        .setType(MultipartBody.FORM)\n");
            for part in parts {
                let kind = part.content_type.as_deref().map(|k| format!("{}.toMediaType()", kt(k)));
                match &part.value {
                    PartValue::Text(value) => match kind {
                        None => out.push_str(&format!("        .addFormDataPart({}, {})\n", kt(&part.name), kt(value))),
                        Some(kind) => out.push_str(&format!(
                            "        .addFormDataPart({}, null, {}.toRequestBody({kind}))\n",
                            kt(&part.name),
                            kt(value)
                        )),
                    },
                    PartValue::File(path) => out.push_str(&format!(
                        "        .addFormDataPart({}, {}, File({}).asRequestBody({}))\n",
                        kt(&part.name),
                        kt(file_name(path)),
                        kt(path),
                        kind.unwrap_or_else(|| "null".into())
                    )),
                }
            }
            out.push_str("        .build()\n\n");
            Some("body")
        }
    };

    out.push_str(&format!("    val request = Request.Builder()\n        .url({})\n", kt(&s.url)));
    out.push_str(&match (method.as_str(), body) {
        ("GET", None) => "        .get()\n".to_owned(),
        ("HEAD", None) => "        .head()\n".to_owned(),
        (_, Some(body)) => format!("        .method({}, {body})\n", kt(&method)),
        (_, None) => format!("        .method({}, null)\n", kt(&method)),
    });
    let headers: Box<dyn Iterator<Item = &(String, String)>> =
        if body.is_some() { Box::new(s.headers_without_content_type()) } else { Box::new(s.headers.iter()) };
    let headers: Vec<_> = headers.collect();
    if unsafe_headers && !headers.is_empty() {
        out.push_str("        .headers(Headers.Builder()\n");
        for (name, value) in &headers {
            let add = if value.is_ascii() { "add" } else { "addUnsafeNonAscii" };
            out.push_str(&format!("            .{add}({}, {})\n", kt(name), kt(value)));
        }
        out.push_str("            .build())\n");
    } else {
        for (name, value) in headers {
            out.push_str(&format!("        .addHeader({}, {})\n", kt(name), kt(value)));
        }
    }
    out.push_str(
        "        .build()\n\n    client.newCall(request).execute().use { response ->\n        println(response.code)\n        println(response.body?.string())\n    }\n}\n",
    );
    out
}
