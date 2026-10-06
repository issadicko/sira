//! Go : `net/http`.

use crate::quote::{self, string, Dialect};
use crate::{Auth, Body, PartValue, Snippet};

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn net_http(s: &Snippet) -> String {
    let mut notes: Vec<String> = s.notes.iter().map(|n| format!("// {n}")).collect();
    match &s.auth {
        Auth::Digest { .. } => notes.push("// Digest : net/http ne le gère pas, ajoute un client qui le fait.".into()),
        Auth::Aws { .. } => notes.push("// AWS Signature V4 : signe la requête avec le SDK AWS (v4.Signer).".into()),
        Auth::None => {}
    }

    let multipart = match &s.body {
        Body::Multipart(parts) => Some(parts),
        _ => None,
    };
    let has_files = multipart.is_some_and(|p| p.iter().any(|p| matches!(p.value, PartValue::File(_))));
    let mut imports = vec!["fmt", "io", "net/http"];
    match &s.body {
        Body::None => {}
        Body::Raw(_) => imports.push("strings"),
        Body::Multipart(_) => {
            imports.extend(["bytes", "mime/multipart"]);
            if has_files {
                imports.extend(["net/textproto", "os"]);
            }
        }
    }
    imports.sort_unstable();

    let mut out = String::new();
    for note in &notes {
        out.push_str(note);
        out.push('\n');
    }
    out.push_str("package main\n\nimport (\n");
    for import in imports {
        out.push_str(&format!("\t\"{import}\"\n"));
    }
    out.push_str(")\n\nfunc main() {\n");
    out.push_str(&format!("\turl := {}\n\n", string(&s.url, Dialect::Go)));

    let reader = match &s.body {
        Body::None => "nil".to_owned(),
        Body::Raw(text) => {
            out.push_str(&format!("\tpayload := strings.NewReader({})\n\n", quote::go(text)));
            "payload".to_owned()
        }
        Body::Multipart(parts) => {
            out.push_str("\tpayload := &bytes.Buffer{}\n\twriter := multipart.NewWriter(payload)\n");
            for part in parts {
                let name = string(&part.name, Dialect::Go);
                match &part.value {
                    PartValue::Text(value) if part.content_type.is_none() => {
                        out.push_str(&format!(
                            "\tif err := writer.WriteField({name}, {}); err != nil {{\n\t\tpanic(err)\n\t}}\n",
                            string(value, Dialect::Go)
                        ));
                    }
                    PartValue::Text(value) => {
                        let kind = string(part.content_type.as_deref().unwrap_or_default(), Dialect::Go);
                        out.push_str(&format!(
                            "\t{{\n\t\th := make(textproto.MIMEHeader)\n\t\th.Set(\"Content-Disposition\", fmt.Sprintf(\"form-data; name=%q\", {name}))\n\t\th.Set(\"Content-Type\", {kind})\n\t\tpart, err := writer.CreatePart(h)\n\t\tif err != nil {{\n\t\t\tpanic(err)\n\t\t}}\n\t\tif _, err := part.Write([]byte({})); err != nil {{\n\t\t\tpanic(err)\n\t\t}}\n\t}}\n",
                            string(value, Dialect::Go)
                        ));
                    }
                    PartValue::File(path) => {
                        let kind = part.content_type.as_deref().unwrap_or("application/octet-stream");
                        out.push_str(&format!(
                            "\t{{\n\t\tfile, err := os.Open({})\n\t\tif err != nil {{\n\t\t\tpanic(err)\n\t\t}}\n\t\tdefer file.Close()\n\t\th := make(textproto.MIMEHeader)\n\t\th.Set(\"Content-Disposition\", fmt.Sprintf(\"form-data; name=%q; filename=%q\", {name}, {}))\n\t\th.Set(\"Content-Type\", {})\n\t\tpart, err := writer.CreatePart(h)\n\t\tif err != nil {{\n\t\t\tpanic(err)\n\t\t}}\n\t\tif _, err := io.Copy(part, file); err != nil {{\n\t\t\tpanic(err)\n\t\t}}\n\t}}\n",
                            string(path, Dialect::Go),
                            string(file_name(path), Dialect::Go),
                            string(kind, Dialect::Go)
                        ));
                    }
                }
            }
            out.push_str("\tif err := writer.Close(); err != nil {\n\t\tpanic(err)\n\t}\n\n");
            "payload".to_owned()
        }
    };

    out.push_str(&format!(
        "\treq, err := http.NewRequest({}, url, {reader})\n\tif err != nil {{\n\t\tpanic(err)\n\t}}\n\n",
        string(&s.method_upper(), Dialect::Go)
    ));
    for (name, value) in &s.headers {
        out.push_str(&format!("\treq.Header.Add({}, {})\n", string(name, Dialect::Go), string(value, Dialect::Go)));
    }
    if multipart.is_some() {
        out.push_str("\treq.Header.Set(\"Content-Type\", writer.FormDataContentType())\n");
    }
    if !s.headers.is_empty() || multipart.is_some() {
        out.push('\n');
    }
    out.push_str(
        "\tres, err := http.DefaultClient.Do(req)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\tdefer res.Body.Close()\n\n\tbody, err := io.ReadAll(res.Body)\n\tif err != nil {\n\t\tpanic(err)\n\t}\n\n\tfmt.Println(res.Status)\n\tfmt.Println(string(body))\n}\n",
    );
    out
}
