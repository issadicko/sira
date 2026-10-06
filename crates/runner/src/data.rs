//! Données d'itération : un fichier CSV (une ligne par itération, en-têtes en première ligne) ou JSON (un tableau
//! d'objets). Les champs d'une ligne deviennent des variables runtime au début de l'itération.

use std::path::Path;

use serde_json::{Map, Value};

/// Une ligne de données : les variables de l'itération.
pub type Row = Map<String, Value>;

#[derive(Debug, thiserror::Error)]
pub enum DataError {
    #[error("lecture de {path} impossible : {reason}")]
    Read { path: String, reason: String },
    #[error("format de données non reconnu pour {0} : attendu .csv ou .json")]
    Format(String),
    #[error("CSV invalide : {0}")]
    Csv(String),
    #[error("JSON invalide : {0}")]
    Json(String),
    #[error("le JSON d'itérations doit être un tableau d'objets, la ligne {0} n'est pas un objet")]
    NotAnObject(usize),
}

/// Lit le fichier `path` selon son extension.
pub fn read_rows(path: &Path) -> Result<Vec<Row>, DataError> {
    let name = path.display().to_string();
    let text =
        std::fs::read_to_string(path).map_err(|e| DataError::Read { path: name.clone(), reason: e.to_string() })?;
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("csv") => parse_csv(&text),
        Some("json") => parse_json(&text),
        _ => Err(DataError::Format(name)),
    }
}

/// Un tableau d'objets ; un objet seul vaut une itération. Les valeurs gardent leur type JSON.
pub fn parse_json(text: &str) -> Result<Vec<Row>, DataError> {
    let value: Value =
        serde_json::from_str(text.trim_start_matches('\u{feff}')).map_err(|e| DataError::Json(e.to_string()))?;
    match value {
        Value::Array(items) => items
            .into_iter()
            .enumerate()
            .map(|(i, item)| match item {
                Value::Object(map) => Ok(map),
                _ => Err(DataError::NotAnObject(i + 1)),
            })
            .collect(),
        Value::Object(map) => Ok(vec![map]),
        _ => Err(DataError::NotAnObject(1)),
    }
}

/// CSV de la RFC 4180 : champs entre guillemets (`""` pour un guillemet, retours à la ligne permis), séparateur
/// virgule. Les valeurs restent du texte. Une ligne vide est ignorée ; une ligne plus courte que l'en-tête laisse
/// ses derniers champs absents, une plus longue est refusée.
pub fn parse_csv(text: &str) -> Result<Vec<Row>, DataError> {
    let records = records(text.trim_start_matches('\u{feff}'))?;
    let mut records = records.into_iter();
    let Some(header) = records.next() else { return Ok(Vec::new()) };
    let mut rows = Vec::new();
    for (i, record) in records.enumerate() {
        if record.len() > header.len() {
            return Err(DataError::Csv(format!(
                "la ligne {} a {} champs pour {} colonnes",
                i + 2,
                record.len(),
                header.len()
            )));
        }
        rows.push(header.iter().cloned().zip(record).map(|(name, value)| (name, Value::String(value))).collect());
    }
    Ok(rows)
}

fn records(text: &str) -> Result<Vec<Vec<String>>, DataError> {
    let mut out: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut after_quote = false;
    let mut started = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    field.push('"');
                    chars.next();
                }
                '"' => {
                    quoted = false;
                    after_quote = true;
                }
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() && !after_quote => {
                quoted = true;
                started = true;
            }
            '"' => return Err(DataError::Csv("guillemet au milieu d'un champ".into())),
            ',' => {
                record.push(std::mem::take(&mut field));
                after_quote = false;
                started = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                if started || !field.is_empty() {
                    record.push(std::mem::take(&mut field));
                    out.push(std::mem::take(&mut record));
                }
                after_quote = false;
                started = false;
            }
            c if after_quote => return Err(DataError::Csv(format!("caractère {c:?} après un guillemet fermant"))),
            c => {
                field.push(c);
                started = true;
            }
        }
    }
    if quoted {
        return Err(DataError::Csv("guillemet jamais refermé".into()));
    }
    if started || !field.is_empty() {
        record.push(field);
        out.push(record);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn csv(text: &str) -> Vec<Value> {
        parse_csv(text).unwrap().into_iter().map(Value::Object).collect()
    }

    #[test]
    fn ef_run_02_csv_rows_are_keyed_by_the_header() {
        assert_eq!(
            csv("name,age\nAda,36\nGrace,45\n"),
            [json!({ "name": "Ada", "age": "36" }), json!({ "name": "Grace", "age": "45" })]
        );
    }

    #[test]
    fn ef_run_02_csv_handles_quotes_commas_newlines_and_crlf() {
        let rows = csv("a,b\r\n\"x, y\",\"he said \"\"hi\"\"\"\r\n\"line1\nline2\",z\r\n");
        assert_eq!(rows[0], json!({ "a": "x, y", "b": "he said \"hi\"" }));
        assert_eq!(rows[1], json!({ "a": "line1\nline2", "b": "z" }));
    }

    #[test]
    fn ef_run_02_csv_keeps_empty_fields_skips_blank_lines_and_the_bom() {
        let rows = csv("\u{feff}a,b,c\n1,,3\n\n4,5,6");
        assert_eq!(rows, [json!({ "a": "1", "b": "", "c": "3" }), json!({ "a": "4", "b": "5", "c": "6" })]);
    }

    #[test]
    fn ef_run_02_csv_short_rows_leave_the_last_columns_out_and_long_rows_are_refused() {
        assert_eq!(csv("a,b\n1"), [json!({ "a": "1" })]);
        let err = parse_csv("a,b\n1,2,3").unwrap_err().to_string();
        assert!(err.contains("ligne 2"), "{err}");
    }

    #[test]
    fn ef_run_02_csv_malformed_quotes_are_reported() {
        assert!(parse_csv("a\n\"open").is_err());
        assert!(parse_csv("a\nx\"y").is_err());
        assert!(parse_csv("a\n\"x\"y").is_err());
    }

    #[test]
    fn ef_run_02_a_header_only_or_empty_csv_has_no_rows() {
        assert!(csv("a,b\n").is_empty());
        assert!(csv("").is_empty());
    }

    #[test]
    fn ef_run_02_json_rows_keep_their_types() {
        let rows = parse_json(r#"[{"id": 1, "ok": true, "tags": ["a"]}, {"id": 2}]"#).unwrap();
        assert_eq!(rows[0]["id"], json!(1));
        assert_eq!(rows[0]["tags"], json!(["a"]));
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn ef_run_02_a_single_json_object_is_one_iteration_and_other_shapes_are_refused() {
        assert_eq!(parse_json(r#"{"a": 1}"#).unwrap().len(), 1);
        assert!(matches!(parse_json("[1]"), Err(DataError::NotAnObject(1))));
        assert!(matches!(parse_json("42"), Err(DataError::NotAnObject(_))));
        assert!(matches!(parse_json("{"), Err(DataError::Json(_))));
    }

    #[test]
    fn ef_run_02_the_format_comes_from_the_extension() {
        let dir = tempfile::tempdir().unwrap();
        let csv_file = dir.path().join("data.CSV");
        std::fs::write(&csv_file, "a\n1").unwrap();
        assert_eq!(read_rows(&csv_file).unwrap().len(), 1);
        let txt = dir.path().join("data.txt");
        std::fs::write(&txt, "a\n1").unwrap();
        assert!(matches!(read_rows(&txt), Err(DataError::Format(_))));
        assert!(matches!(read_rows(&dir.path().join("missing.csv")), Err(DataError::Read { .. })));
    }
}
