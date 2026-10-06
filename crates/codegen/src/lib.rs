//! Génération de code : une requête HTTP résolue (variables remplacées) devient un extrait prêt à coller dans neuf
//! langages. L'extrait reproduit ce que Sira envoie : même méthode, même adresse, mêmes en-têtes, même corps.
//!
//! Chaque générateur n'utilise que la bibliothèque la plus courante de son langage : cURL, `fetch`, `requests`,
//! `net/http`, OkHttp (Java et Kotlin), `package:http`, l'extension cURL de PHP et `HttpClient`.

mod csharp;
mod dart;
mod go;
mod java;
mod javascript;
mod kotlin;
mod php;
mod python;
mod quote;
mod shell;

use serde::{Deserialize, Serialize};

/// Le langage de l'extrait.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Curl,
    #[serde(rename = "javascript")]
    JavaScript,
    Python,
    Go,
    Java,
    Kotlin,
    Dart,
    Php,
    #[serde(rename = "csharp")]
    CSharp,
}

impl Language {
    pub const ALL: [Language; 9] = [
        Self::Curl,
        Self::JavaScript,
        Self::Python,
        Self::Go,
        Self::Java,
        Self::Kotlin,
        Self::Dart,
        Self::Php,
        Self::CSharp,
    ];

    /// L'identifiant (`curl`, `javascript`…) que la ligne de commande et l'interface s'échangent.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Curl => "curl",
            Self::JavaScript => "javascript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::Dart => "dart",
            Self::Php => "php",
            Self::CSharp => "csharp",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Curl => "cURL",
            Self::JavaScript => "JavaScript (fetch)",
            Self::Python => "Python (requests)",
            Self::Go => "Go (net/http)",
            Self::Java => "Java (OkHttp)",
            Self::Kotlin => "Kotlin (OkHttp)",
            Self::Dart => "Dart (http)",
            Self::Php => "PHP (cURL)",
            Self::CSharp => "C# (HttpClient)",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.id() == id.to_ascii_lowercase())
    }
}

/// Le corps d'une requête.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Body {
    #[default]
    None,
    /// Texte tel qu'envoyé : JSON, texte, XML ou formulaire déjà encodé.
    Raw(String),
    /// Formulaire multipart : le client choisit la frontière, l'extrait ne porte donc pas de `Content-Type` pour lui.
    Multipart(Vec<Part>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub name: String,
    pub value: PartValue,
    pub content_type: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PartValue {
    Text(String),
    /// Chemin d'un fichier ; un champ à plusieurs fichiers donne une partie par fichier.
    File(String),
}

/// Ce que l'auth fait à l'envoi et que des en-têtes ne disent pas.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Auth {
    #[default]
    None,
    Digest {
        username: String,
        password: String,
    },
    Aws {
        access_key_id: String,
        secret_access_key: String,
        session_token: String,
        region: String,
        service: String,
    },
}

/// Une requête résolue, prête à être transcrite.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snippet {
    pub method: String,
    pub url: String,
    /// `Content-Type` compris ; sans lui pour un corps multipart.
    pub headers: Vec<(String, String)>,
    pub body: Body,
    pub auth: Auth,
    /// Remarques à reproduire en commentaire (une auth que l'extrait ne peut pas porter, par exemple).
    pub notes: Vec<String>,
}

/// L'extrait de la requête dans le langage demandé.
pub fn generate(snippet: &Snippet, language: Language) -> String {
    match language {
        Language::Curl => shell::curl(snippet),
        Language::JavaScript => javascript::fetch(snippet),
        Language::Python => python::requests(snippet),
        Language::Go => go::net_http(snippet),
        Language::Java => java::okhttp(snippet),
        Language::Kotlin => kotlin::okhttp(snippet),
        Language::Dart => dart::http(snippet),
        Language::Php => php::curl(snippet),
        Language::CSharp => csharp::http_client(snippet),
    }
}

impl Snippet {
    /// La valeur du `Content-Type`, sans tenir compte de la casse du nom.
    pub(crate) fn content_type(&self) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-type")).map(|(_, v)| v.as_str())
    }

    /// Les en-têtes sans `Content-Type`, qu'un client fixe avec le corps.
    pub(crate) fn headers_without_content_type(&self) -> impl Iterator<Item = &(String, String)> {
        self.headers.iter().filter(|(k, _)| !k.eq_ignore_ascii_case("content-type"))
    }

    /// Les noms d'en-tête se répètent-ils (sans tenir compte de la casse) ?
    pub(crate) fn has_duplicate_headers(&self) -> bool {
        self.headers
            .iter()
            .enumerate()
            .any(|(i, (a, _))| self.headers[..i].iter().any(|(b, _)| a.eq_ignore_ascii_case(b)))
    }

    pub(crate) fn method_upper(&self) -> String {
        self.method.to_ascii_uppercase()
    }
}
