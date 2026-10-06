//! Les extraits de chaque langage pour une série de requêtes, comparés à des fichiers de référence
//! (`tests/golden/<cas>.<extension>`). `UPDATE_GOLDEN=1` les réécrit ; `CODEGEN_DUMP=dossier` écrit en plus les extraits
//! pour qu'un banc extérieur les exécute contre un serveur local.

use std::fs;
use std::path::{Path, PathBuf};

use xc_codegen::{generate, Auth, Body, Language, Part, PartValue, Snippet};

fn extension(language: Language) -> &'static str {
    match language {
        Language::Curl => "sh",
        Language::JavaScript => "js",
        Language::Python => "py",
        Language::Go => "go",
        Language::Java => "java",
        Language::Kotlin => "kt",
        Language::Dart => "dart",
        Language::Php => "php",
        Language::CSharp => "cs",
    }
}

fn headers(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect()
}

fn text(name: &str, value: &str, content_type: Option<&str>) -> Part {
    Part { name: name.into(), value: PartValue::Text(value.into()), content_type: content_type.map(str::to_owned) }
}

fn file(name: &str, path: &str, content_type: Option<&str>) -> Part {
    Part { name: name.into(), value: PartValue::File(path.into()), content_type: content_type.map(str::to_owned) }
}

const BASE: &str = "http://127.0.0.1:8765";

/// Les requêtes d'essai : de la plus simple aux caractères qui compliquent l'échappement.
pub fn cases() -> Vec<(&'static str, Snippet)> {
    let get = Snippet {
        method: "GET".into(),
        url: format!("{BASE}/items?x=1&y=a%20b"),
        headers: headers(&[
            ("Accept", "application/json"),
            ("X-Odd", "valé \"double\" 'simple' $dollar \\anti `tick`"),
        ]),
        ..Snippet::default()
    };
    let json = Snippet {
        method: "POST".into(),
        url: format!("{BASE}/users"),
        headers: headers(&[("Content-Type", "application/json; charset=utf-8"), ("Authorization", "Bearer abc.def")]),
        body: Body::Raw("{\n  \"name\": \"Aminata \\\"Ami\\\" Diallo\",\n  \"note\": \"café $HOME ${x} 日本語\\n\",\n  \"path\": \"C:\\\\temp\"\n}".into()),
        ..Snippet::default()
    };
    let form = Snippet {
        method: "POST".into(),
        url: format!("{BASE}/login"),
        headers: headers(&[("Content-Type", "application/x-www-form-urlencoded")]),
        body: Body::Raw("grant=password&user=x%40y.test&scope=a+b".into()),
        ..Snippet::default()
    };
    let multipart = Snippet {
        method: "POST".into(),
        url: format!("{BASE}/upload"),
        headers: headers(&[("X-Trace", "1")]),
        body: Body::Multipart(vec![
            text("title", "Photo; \"cover\"", None),
            text("note", "typed", Some("text/plain")),
            file("avatar", "files/a.png", Some("image/png")),
            file("doc", "files/b.txt", None),
        ]),
        ..Snippet::default()
    };
    let empty_put = Snippet { method: "PUT".into(), url: format!("{BASE}/items/7"), ..Snippet::default() };
    let delete = Snippet {
        method: "DELETE".into(),
        url: format!("{BASE}/items/7"),
        headers: headers(&[("X-Reason", "cleanup")]),
        ..Snippet::default()
    };
    let head = Snippet { method: "head".into(), url: format!("{BASE}/items"), ..Snippet::default() };
    let patch_text = Snippet {
        method: "PATCH".into(),
        url: format!("{BASE}/items/7"),
        headers: headers(&[("Content-Type", "text/plain")]),
        body: Body::Raw("quoi ? ça va\r\ntrès bien".into()),
        ..Snippet::default()
    };
    let digest = Snippet {
        method: "GET".into(),
        url: format!("{BASE}/secure"),
        auth: Auth::Digest { username: "ada".into(), password: "p@ss".into() },
        notes: vec!["Auth OAuth 2.0 : ajoute le jeton obtenu dans l'en-tête Authorization.".into()],
        ..Snippet::default()
    };
    vec![
        ("get", get),
        ("json", json),
        ("form", form),
        ("multipart", multipart),
        ("empty_put", empty_put),
        ("delete", delete),
        ("head", head),
        ("patch_text", patch_text),
        ("digest", digest),
    ]
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

#[test]
fn every_language_matches_its_reference_snippet() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let dump = std::env::var_os("CODEGEN_DUMP").map(PathBuf::from);
    let mut mismatches = Vec::new();
    for (name, snippet) in cases() {
        for language in Language::ALL {
            let code = generate(&snippet, language);
            let file = format!("{name}.{}", extension(language));
            if let Some(dir) = &dump {
                fs::create_dir_all(dir).unwrap();
                fs::write(dir.join(&file), &code).unwrap();
            }
            let path = golden_dir().join(&file);
            if update {
                fs::write(&path, &code).unwrap();
            } else if fs::read_to_string(&path).ok().as_deref() != Some(code.as_str()) {
                mismatches.push(file);
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "extraits différents de leur référence : {mismatches:?} (UPDATE_GOLDEN=1 pour les réécrire)"
    );
}

#[test]
fn language_ids_round_trip() {
    for language in Language::ALL {
        assert_eq!(Language::from_id(language.id()), Some(language));
    }
    assert_eq!(Language::from_id("CURL"), Some(Language::Curl));
    assert_eq!(Language::from_id("cobol"), None);
}

#[test]
fn curl_quotes_single_quotes_and_keeps_the_body_verbatim() {
    let snippet = Snippet {
        method: "POST".into(),
        url: "https://x.test/a?b='c'".into(),
        body: Body::Raw("it's\n{\"a\": 1}".into()),
        ..Snippet::default()
    };
    let code = generate(&snippet, Language::Curl);
    assert!(code.contains("--url 'https://x.test/a?b='\\''c'\\'''"), "{code}");
    assert!(code.contains("--data-raw 'it'\\''s\n{\"a\": 1}'"), "{code}");
}

#[test]
fn java_never_emits_unicode_escapes_for_line_breaks() {
    let snippet = Snippet {
        method: "POST".into(),
        url: "http://x".into(),
        body: Body::Raw("a\u{1}b\nc".into()),
        ..Snippet::default()
    };
    let code = generate(&snippet, Language::Java);
    assert!(code.contains("\"a\\001b\\nc\""), "{code}");
    assert!(!code.contains("\\u000"), "{code}");
}
