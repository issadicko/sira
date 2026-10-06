use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use xc_core::graphql::{
    parse_variables, read_stored, schema_from_response, store, strip_comments, StoredSchema, INTROSPECTION_QUERY,
};
use xc_core::request::BLANK_BEFORE;
use xc_core::yaml::{self, Value as Yaml};
use xc_core::{prepare, read_request, Body, CoreError, RequestDoc};

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)).unwrap()
}

fn tree(text: &str) -> yaml::Map {
    match yaml::parse(text).unwrap() {
        Yaml::Map(m) => m,
        _ => panic!("table attendue"),
    }
}

fn saved(tree: yaml::Map) -> String {
    yaml::emit(&Yaml::Map(tree), BLANK_BEFORE)
}

fn collection(request: &str) -> (tempfile::TempDir, RequestDoc) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: G\n").unwrap();
    fs::create_dir(root.join("environments")).unwrap();
    fs::write(
        root.join("environments/dev.yml"),
        "name: dev\nvariables:\n  - name: base\n    value: https://shop.test\n  - name: sku\n    value: DS4-B\n",
    )
    .unwrap();
    fs::write(root.join("req.yml"), request).unwrap();
    let doc = read_request(root, "req.yml").unwrap();
    (dir, doc)
}

fn sent(dir: &tempfile::TempDir, doc: &RequestDoc) -> Result<xc_core::Prepared, CoreError> {
    prepare(dir.path(), "req.yml", doc, Some("dev"), &HashMap::new())
}

#[test]
fn ef_gql_01_a_graphql_body_is_read_as_query_and_variables() {
    let doc = RequestDoc::from_tree(&tree(&fixture("request-graphql-variables.yml")));
    let Body::Graphql { query, variables } = &doc.body else { panic!("{:?}", doc.body) };
    assert!(query.starts_with("query Product($sku: String!) {") && query.ends_with('}'), "{query}");
    assert_eq!(variables, "{\n  \"sku\": \"{{sku}}\"\n}");
    assert_eq!(doc.request_type, "graphql");
}

#[test]
fn ef_gql_01_a_graphql_file_round_trips_without_diff() {
    for name in ["request-graphql-variables.yml", "request-graphql.yml"] {
        let original = fixture(name);
        let mut t = tree(&original);
        let doc = RequestDoc::from_tree(&t);
        doc.apply(&mut t, &doc.clone());
        assert_eq!(saved(t), original, "{name}");
    }
}

#[test]
fn ef_gql_01_editing_the_query_touches_only_the_query_lines() {
    let original = fixture("request-graphql-variables.yml");
    let mut t = tree(&original);
    let previous = RequestDoc::from_tree(&t);
    let mut doc = previous.clone();
    let Body::Graphql { query, .. } = &mut doc.body else { panic!() };
    *query = query.replace("name\n", "name\n    price\n");
    doc.apply(&mut t, &previous);
    assert_eq!(saved(t), original.replace("          name\n", "          name\n          price\n"));
}

#[test]
fn ef_gql_01_clearing_the_variables_removes_only_that_key() {
    let original = fixture("request-graphql-variables.yml");
    let mut t = tree(&original);
    let previous = RequestDoc::from_tree(&t);
    let mut doc = previous.clone();
    let Body::Graphql { variables, .. } = &mut doc.body else { panic!() };
    variables.clear();
    doc.apply(&mut t, &previous);
    let text = saved(t);
    assert!(text.contains("query: |-") && !text.contains("variables"), "{text}");
}

#[test]
fn ef_gql_01_a_query_added_to_an_empty_graphql_request_creates_the_body() {
    let original = "info:\n  name: New\n  type: graphql\n\ngraphql:\n  method: POST\n  url: \"\"\n";
    let mut t = tree(original);
    let previous = RequestDoc::from_tree(&t);
    assert_eq!(previous.body, Body::None);
    let mut doc = previous.clone();
    doc.body = Body::Graphql { query: "{ me { id } }".into(), variables: String::new() };
    doc.apply(&mut t, &previous);
    let text = saved(t);
    assert!(text.contains("graphql:\n  method: POST\n  url: \"\"\n  body:\n    query: \"{ me { id } }\""), "{text}");
    assert!(!text.contains("http:"), "{text}");
    let reread = RequestDoc::from_tree(&tree(&text));
    assert_eq!(reread.body, doc.body);
}

#[test]
fn ef_gql_01_emptying_both_fields_removes_the_body() {
    let original = fixture("request-graphql.yml");
    let mut t = tree(&original);
    let previous = RequestDoc::from_tree(&t);
    let mut doc = previous.clone();
    doc.body = Body::Graphql { query: String::new(), variables: String::new() };
    doc.apply(&mut t, &previous);
    assert!(!saved(t).contains("body:"));
}

#[test]
fn ef_gql_01_the_sent_body_is_json_with_resolved_query_and_variables() {
    let (dir, doc) = collection(&fixture("request-graphql-variables.yml"));
    let prepared = sent(&dir, &doc).unwrap();
    assert_eq!(
        (prepared.request.method.as_str(), prepared.request.url.as_str()),
        ("POST", "https://shop.test/graphql")
    );
    let content_type = prepared.request.headers.iter().find(|(k, _)| k == "Content-Type").map(|(_, v)| v.as_str());
    assert_eq!(content_type, Some("application/json"));
    let body: Value = serde_json::from_slice(prepared.request.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["variables"], json!({ "sku": "DS4-B" }));
    assert!(body["query"].as_str().unwrap().contains("product(sku: $sku)"));
}

#[test]
fn ef_gql_01_a_request_without_variables_sends_an_empty_object() {
    let (dir, doc) = collection(&fixture("request-graphql.yml"));
    let prepared = sent(&dir, &doc).unwrap();
    let body: Value = serde_json::from_slice(prepared.request.body.as_deref().unwrap()).unwrap();
    assert_eq!(body["variables"], json!({}));
    assert!(body["query"].as_str().unwrap().starts_with("mutation {"));
}

#[test]
fn ef_gql_01_invalid_variables_are_reported_before_sending() {
    let request = fixture("request-graphql-variables.yml").replace("\"sku\": \"{{sku}}\"", "sku: 1");
    let (dir, doc) = collection(&request);
    let error = sent(&dir, &doc).err().expect("des variables invalides ne partent pas");
    assert!(matches!(error, CoreError::GraphqlVariables(_)), "{error}");
}

#[test]
fn ef_gql_01_variables_accept_comments_like_bruno() {
    let text = "{\n  // identifiant\n  \"url\": \"http://x.test/a\", /* inline */\n  \"n\": 1\n}";
    assert_eq!(parse_variables(text).unwrap(), json!({ "url": "http://x.test/a", "n": 1 }));
    assert_eq!(strip_comments("\"a // b\" // c\nd"), "\"a // b\" \nd");
    assert_eq!(parse_variables("  \n").unwrap(), json!({}));
}

#[test]
fn ef_gql_01_the_introspection_query_asks_for_the_whole_schema() {
    for needle in
        ["__schema", "queryType", "mutationType", "subscriptionType", "directives", "enumValues", "possibleTypes"]
    {
        assert!(INTROSPECTION_QUERY.contains(needle), "{needle}");
    }
}

#[test]
fn ef_gql_01_a_schema_is_read_from_the_introspection_response() {
    let body = r#"{"data":{"__schema":{"queryType":{"name":"Query"},"types":[]}}}"#;
    let schema = schema_from_response(200, body).unwrap();
    assert_eq!(schema["__schema"]["queryType"]["name"], "Query");
}

#[test]
fn ef_gql_01_a_refused_introspection_reports_the_server_message() {
    let refused = r#"{"errors":[{"message":"GraphQL introspection is not allowed"}]}"#;
    let message = schema_from_response(200, refused).unwrap_err();
    assert!(message.contains("GraphQL introspection is not allowed"), "{message}");
    assert!(schema_from_response(502, "<html>Bad gateway</html>").unwrap_err().contains("502"));
    assert!(schema_from_response(200, "{}").unwrap_err().contains("sans schéma"));
}

fn stored(url: &str) -> StoredSchema {
    StoredSchema {
        url: url.into(),
        fetched_at: "2026-10-06T12:00:00.000Z".into(),
        introspection: json!({ "__schema": {} }),
    }
}

#[test]
fn ef_gql_01_a_stored_schema_is_found_again_by_its_url_only() {
    let dir = tempfile::tempdir().unwrap();
    store(dir.path(), &stored("https://shop.test/graphql")).unwrap();
    assert_eq!(read_stored(dir.path(), "https://shop.test/graphql"), Some(stored("https://shop.test/graphql")));
    assert_eq!(read_stored(dir.path(), "https://other.test/graphql"), None);
    fs::write(dir.path().join(".oc-sync/graphql").read_dir().unwrap().next().unwrap().unwrap().path(), "pas du json")
        .unwrap();
    assert_eq!(read_stored(dir.path(), "https://shop.test/graphql"), None, "un fichier illisible se recharge");
}

#[cfg(unix)]
#[test]
fn ef_gql_01_a_schema_is_never_written_through_a_symlink() {
    let (dir, outside) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    std::os::unix::fs::symlink(outside.path(), dir.path().join(".oc-sync")).unwrap();
    let error = store(dir.path(), &stored("https://shop.test/graphql")).unwrap_err();
    assert!(matches!(error, CoreError::Symlink(_)), "{error}");
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0, "rien n'est écrit hors de la collection");
}
