//! GraphQL : corps JSON d'une requête, requête d'introspection et lecture de sa réponse.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::collection::write_atomic;
use crate::request::Body;
use crate::CoreError;

/// La requête d'introspection standard (`getIntrospectionQuery` de graphql-js, descriptions comprises).
pub const INTROSPECTION_QUERY: &str = "query IntrospectionQuery {
  __schema {
    queryType { name }
    mutationType { name }
    subscriptionType { name }
    types { ...FullType }
    directives {
      name
      description
      locations
      args { ...InputValue }
    }
  }
}

fragment FullType on __Type {
  kind
  name
  description
  fields(includeDeprecated: true) {
    name
    description
    args { ...InputValue }
    type { ...TypeRef }
    isDeprecated
    deprecationReason
  }
  inputFields { ...InputValue }
  interfaces { ...TypeRef }
  enumValues(includeDeprecated: true) {
    name
    description
    isDeprecated
    deprecationReason
  }
  possibleTypes { ...TypeRef }
}

fragment InputValue on __InputValue {
  name
  description
  type { ...TypeRef }
  defaultValue
}

fragment TypeRef on __Type {
  kind
  name
  ofType {
    kind
    name
    ofType {
      kind
      name
      ofType {
        kind
        name
        ofType {
          kind
          name
          ofType {
            kind
            name
            ofType {
              kind
              name
              ofType { kind name }
            }
          }
        }
      }
    }
  }
}
";

/// Le texte sans ses commentaires `//` et `/* */`, que Bruno tolère dans les variables ; les chaînes sont respectées.
pub fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            match c {
                '\\' => out.extend(chars.next()),
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut previous = ' ';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// Les variables d'une requête, une fois leurs `{{…}}` résolues : un objet JSON, `{}` quand le texte est vide.
pub fn parse_variables(text: &str) -> Result<Value, CoreError> {
    let cleaned = strip_comments(text);
    if cleaned.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&cleaned).map_err(|e| CoreError::GraphqlVariables(e.to_string()))
}

/// Le corps envoyé : `{"query": …, "variables": {…}}`.
pub fn payload(query: &str, variables: &str) -> Result<String, CoreError> {
    Ok(json!({ "query": query, "variables": parse_variables(variables)? }).to_string())
}

/// Le corps qui demande le schéma.
pub fn introspection() -> Body {
    Body::Graphql { query: INTROSPECTION_QUERY.to_owned(), variables: String::new() }
}

/// Le résultat d'introspection (`{"__schema": …}`) d'une réponse, ou ce que le serveur a répondu à la place :
/// le premier message d'erreur GraphQL, un corps qui n'est pas du JSON, un schéma absent.
pub fn schema_from_response(status: u16, body: &str) -> Result<Value, String> {
    let parsed: Value = serde_json::from_str(body).map_err(|_| {
        let extract: String = body.trim().chars().take(160).collect();
        format!("réponse {status} qui n'est pas du JSON : {extract}")
    })?;
    if let Some(schema) = parsed.pointer("/data/__schema").filter(|s| s.is_object()) {
        return Ok(json!({ "__schema": schema }));
    }
    let message = parsed
        .get("errors")
        .and_then(Value::as_array)
        .and_then(|errors| errors.first())
        .and_then(|e| e.get("message"))
        .and_then(Value::as_str)
        .or_else(|| parsed.get("message").and_then(Value::as_str));
    Err(match message {
        Some(m) => format!("le serveur refuse l'introspection : {m}"),
        None => format!("réponse {status} sans schéma"),
    })
}

/// Un schéma gardé sur disque : l'URL qui l'a servi, l'instant du chargement et le résultat d'introspection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredSchema {
    pub url: String,
    pub fetched_at: String,
    pub introspection: Value,
}

fn url_key(url: &str) -> String {
    let hash = url.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

fn schema_file(root: &Path, url: &str) -> PathBuf {
    root.join(".oc-sync").join("graphql").join(format!("{}.json", url_key(url)))
}

/// Le schéma gardé pour `url`, `None` s'il n'y en a pas ou si le fichier est illisible : il se recharge alors.
pub fn read_stored(root: &Path, url: &str) -> Option<StoredSchema> {
    let text = fs::read_to_string(schema_file(root, url)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Garde le schéma de `url` dans `.oc-sync/graphql/`, hors des fichiers de la collection. Rien n'est écrit à travers un
/// lien symbolique (`.oc-sync` ou son dossier `graphql`) : le stockage ne sort pas de la collection.
pub fn store(root: &Path, schema: &StoredSchema) -> Result<(), CoreError> {
    let file = schema_file(root, &schema.url);
    let folder = file.parent().unwrap_or(root);
    for dir in [root.join(".oc-sync"), folder.to_path_buf()] {
        if dir.is_symlink() {
            return Err(CoreError::Symlink(dir.display().to_string()));
        }
    }
    fs::create_dir_all(folder).map_err(|e| CoreError::io(folder, e))?;
    let text = serde_json::to_string(schema)
        .map_err(|e| CoreError::Io { path: file.display().to_string(), message: e.to_string() })?;
    write_atomic(root, &file, &text)
}
