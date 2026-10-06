//! cURL.

use crate::quote;
use crate::{Auth, Body, PartValue, Snippet};

pub fn curl(s: &Snippet) -> String {
    let mut out = String::new();
    for note in &s.notes {
        out.push_str(&format!("# {note}\n"));
    }
    let method = s.method_upper();
    let mut args: Vec<String> = Vec::new();
    if method == "HEAD" {
        args.push("--head".into());
    } else {
        let name = if method.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            method.clone()
        } else {
            quote::shell(&method)
        };
        args.push(format!("--request {name}"));
    }
    args.push(format!("--url {}", quote::shell(&s.url)));
    for (name, value) in &s.headers {
        args.push(format!("--header {}", quote::shell(&format!("{name}: {value}"))));
    }
    match &s.auth {
        Auth::None => {}
        Auth::Digest { username, password } => {
            args.push("--digest".into());
            args.push(format!("--user {}", quote::shell(&format!("{username}:{password}"))));
        }
        Auth::Aws { access_key_id, secret_access_key, session_token, region, service } => {
            args.push(format!("--aws-sigv4 {}", quote::shell(&format!("aws:amz:{region}:{service}"))));
            args.push(format!("--user {}", quote::shell(&format!("{access_key_id}:{secret_access_key}"))));
            if !session_token.is_empty() {
                args.push(format!("--header {}", quote::shell(&format!("x-amz-security-token: {session_token}"))));
            }
        }
    }
    match &s.body {
        Body::None => {}
        Body::Raw(text) => args.push(format!("--data-raw {}", quote::shell(text))),
        Body::Multipart(parts) => {
            for part in parts {
                let kind = part.content_type.as_deref().map(|t| format!(";type={t}")).unwrap_or_default();
                match &part.value {
                    PartValue::File(path) => {
                        args.push(format!("--form {}", quote::shell(&format!("{}=@\"{path}\"{kind}", part.name))))
                    }
                    PartValue::Text(value) if kind.is_empty() => {
                        args.push(format!("--form-string {}", quote::shell(&format!("{}={value}", part.name))))
                    }
                    PartValue::Text(value) => {
                        args.push(format!("--form {}", quote::shell(&format!("{}={value}{kind}", part.name))))
                    }
                }
            }
        }
    }
    out.push_str("curl ");
    out.push_str(&args.join(" \\\n  "));
    out.push('\n');
    out
}
