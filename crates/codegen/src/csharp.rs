//! C# : `HttpClient`, en instructions de niveau supérieur (C# 9 et plus).

use crate::quote::{string, Dialect};
use crate::{Auth, Body, PartValue, Snippet};

fn cs(text: &str) -> String {
    string(text, Dialect::CSharp)
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// Les en-têtes que `HttpClient` range du côté du contenu.
fn is_content_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("content-") || matches!(name.as_str(), "allow" | "expires" | "last-modified")
}

pub fn http_client(s: &Snippet) -> String {
    let mut out = String::new();
    for note in &s.notes {
        out.push_str(&format!("// {note}\n"));
    }
    match &s.auth {
        Auth::Digest { username, password } => out.push_str(&format!(
            "// Digest : passe par un HttpClientHandler avec des identifiants.\n// var handler = new HttpClientHandler {{ Credentials = new System.Net.NetworkCredential({}, {}) }};\n",
            cs(username),
            cs(password)
        )),
        Auth::Aws { .. } => out.push_str("// AWS Signature V4 : signe la requête avec le SDK AWS pour .NET.\n"),
        Auth::None => {}
    }
    let has_files =
        matches!(&s.body, Body::Multipart(parts) if parts.iter().any(|p| matches!(p.value, PartValue::File(_))));
    out.push_str("using System;\n");
    if has_files {
        out.push_str("using System.IO;\n");
    }
    out.push_str("using System.Net.Http;\nusing System.Net.Http.Headers;\nusing System.Text;\n\n");
    out.push_str(&format!(
        "using var client = new HttpClient();\nusing var request = new HttpRequestMessage(new HttpMethod({}), {});\n",
        cs(&s.method_upper()),
        cs(&s.url)
    ));
    let has_body = !matches!(s.body, Body::None);
    for (name, value) in &s.headers {
        if has_body && is_content_header(name) {
            continue;
        }
        out.push_str(&format!("request.Headers.TryAddWithoutValidation({}, {});\n", cs(name), cs(value)));
    }
    match &s.body {
        Body::None => {}
        Body::Raw(text) => {
            out.push_str(&format!("request.Content = new StringContent({}, Encoding.UTF8);\n", cs(text)));
            match s.content_type() {
                Some(kind) => out.push_str(&format!(
                    "request.Content.Headers.ContentType = MediaTypeHeaderValue.Parse({});\n",
                    cs(kind)
                )),
                None => out.push_str("request.Content.Headers.ContentType = null;\n"),
            }
            for (name, value) in s.headers_without_content_type().filter(|(k, _)| is_content_header(k)) {
                out.push_str(&format!(
                    "request.Content.Headers.TryAddWithoutValidation({}, {});\n",
                    cs(name),
                    cs(value)
                ));
            }
        }
        Body::Multipart(parts) => {
            out.push_str("using var form = new MultipartFormDataContent();\n");
            for (index, part) in parts.iter().enumerate() {
                let name = cs(&part.name);
                let kind = part.content_type.as_deref().map(|k| format!("MediaTypeHeaderValue.Parse({})", cs(k)));
                match &part.value {
                    PartValue::Text(value) => match kind {
                        None => out.push_str(&format!("form.Add(new StringContent({}), {name});\n", cs(value))),
                        Some(kind) => out.push_str(&format!(
                            "var part{index} = new StringContent({});\npart{index}.Headers.ContentType = {kind};\nform.Add(part{index}, {name});\n",
                            cs(value)
                        )),
                    },
                    PartValue::File(path) => {
                        out.push_str(&format!("var part{index} = new ByteArrayContent(File.ReadAllBytes({}));\n", cs(path)));
                        if let Some(kind) = kind {
                            out.push_str(&format!("part{index}.Headers.ContentType = {kind};\n"));
                        }
                        out.push_str(&format!("form.Add(part{index}, {name}, {});\n", cs(file_name(path))));
                    }
                }
            }
            out.push_str("request.Content = form;\n");
        }
    }
    out.push_str(
        "\nusing var response = await client.SendAsync(request);\nConsole.WriteLine((int)response.StatusCode);\nConsole.WriteLine(await response.Content.ReadAsStringAsync());\n",
    );
    out
}
