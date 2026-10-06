//! PHP : l'extension cURL.

use crate::quote;
use crate::{Auth, Body, PartValue, Snippet};

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

pub fn curl(s: &Snippet) -> String {
    let mut out = String::from("<?php\n\n");
    for note in &s.notes {
        out.push_str(&format!("// {note}\n"));
    }
    if let Auth::Aws { .. } = s.auth {
        out.push_str("// AWS Signature V4 : signe la requête avec le SDK AWS pour PHP.\n");
    }
    let method = s.method_upper();
    out.push_str(&format!(
        "$curl = curl_init();\n\ncurl_setopt_array($curl, [\n  CURLOPT_URL => {},\n  CURLOPT_RETURNTRANSFER => true,\n",
        quote::php(&s.url)
    ));
    if method == "HEAD" {
        out.push_str("  CURLOPT_NOBODY => true,\n");
    } else {
        out.push_str(&format!("  CURLOPT_CUSTOMREQUEST => {},\n", quote::php(&method)));
    }
    match &s.body {
        Body::None => {}
        Body::Raw(text) => out.push_str(&format!("  CURLOPT_POSTFIELDS => {},\n", quote::php(text))),
        Body::Multipart(parts) => {
            out.push_str("  CURLOPT_POSTFIELDS => [\n");
            for part in parts {
                let name = quote::php(&part.name);
                match &part.value {
                    PartValue::Text(value) => out.push_str(&format!("    {name} => {},\n", quote::php(value))),
                    PartValue::File(path) => {
                        let kind = part.content_type.as_deref().map(quote::php).unwrap_or_else(|| "null".into());
                        out.push_str(&format!(
                            "    {name} => new CURLFile({}, {kind}, {}),\n",
                            quote::php(path),
                            quote::php(file_name(path))
                        ));
                    }
                }
            }
            out.push_str("  ],\n");
        }
    }
    if !s.headers.is_empty() {
        out.push_str("  CURLOPT_HTTPHEADER => [\n");
        for (name, value) in &s.headers {
            out.push_str(&format!("    {},\n", quote::php(&format!("{name}: {value}"))));
        }
        out.push_str("  ],\n");
    }
    if let Auth::Digest { username, password } = &s.auth {
        out.push_str(&format!(
            "  CURLOPT_HTTPAUTH => CURLAUTH_DIGEST,\n  CURLOPT_USERPWD => {},\n",
            quote::php(&format!("{username}:{password}"))
        ));
    }
    out.push_str(
        "]);\n\n$response = curl_exec($curl);\n$error = curl_error($curl);\n\ncurl_close($curl);\n\nif ($error) {\n  echo 'cURL Error #:' . $error;\n} else {\n  echo $response;\n}\n",
    );
    out
}
