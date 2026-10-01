use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use xc_sync::openapi::{load_spec, summary, to_bruno, GroupBy, OpenApiError};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/openapi")
}

fn specs() -> Vec<(String, String)> {
    let mut out: Vec<_> = fs::read_dir(fixtures().join("specs"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|p| (p.file_stem().unwrap().to_string_lossy().into_owned(), fs::read_to_string(&p).unwrap()))
        .collect();
    out.sort();
    out
}

fn spec(name: &str) -> Value {
    let (_, text) = specs().into_iter().find(|(n, _)| n == name).unwrap();
    load_spec(&text).unwrap()
}

fn convert(text: &str, group_by: GroupBy) -> Result<Value, OpenApiError> {
    to_bruno(&load_spec(text)?, group_by)
}

fn requests(items: &Value) -> Vec<&Value> {
    let mut out = Vec::new();
    for item in items.as_array().into_iter().flatten() {
        if item["type"] == "folder" {
            out.extend(requests(&item["items"]));
        } else {
            out.push(item);
        }
    }
    out
}

fn without_operation_keys(mut v: Value) -> Value {
    fn strip(items: &mut Value) {
        for item in items.as_array_mut().into_iter().flatten().filter_map(Value::as_object_mut) {
            item.remove("operationKey");
            if let Some(children) = item.get_mut("items") {
                strip(children);
            }
        }
    }
    if let Some(items) = v.get_mut("items") {
        strip(items);
    }
    v
}

/// Première différence entre deux JSON, ordre des clés compris.
fn first_diff(expected: &Value, actual: &Value, path: &str) -> Option<String> {
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            let (ek, ak): (Vec<_>, Vec<_>) = (e.keys().collect(), a.keys().collect());
            if ek != ak {
                return Some(format!("{path} : clés attendues {ek:?}, obtenues {ak:?}"));
            }
            e.iter().find_map(|(k, v)| first_diff(v, &a[k], &format!("{path}.{k}")))
        }
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                return Some(format!("{path} : {} éléments attendus, {} obtenus", e.len(), a.len()));
            }
            e.iter().zip(a).enumerate().find_map(|(i, (x, y))| first_diff(x, y, &format!("{path}[{i}]")))
        }
        _ if expected == actual => None,
        _ => Some(format!("{path}\n  attendu : {expected}\n  obtenu  : {actual}")),
    }
}

fn check_corpus(group_by: GroupBy, suffix: &str) {
    let mut failures = Vec::new();
    for (name, text) in specs() {
        let file = fixtures().join("expected").join(format!("{name}.{suffix}.json"));
        let expected: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        match (expected.get("$error"), convert(&text, group_by)) {
            (Some(_), Err(_)) => {}
            (Some(message), Ok(_)) => failures.push(format!("{name} : Bruno échoue ({message}), pas le port")),
            (None, Err(e)) => failures.push(format!("{name} : {e}")),
            (None, Ok(actual)) => {
                if let Some(diff) = first_diff(&expected, &without_operation_keys(actual), "$") {
                    failures.push(format!("{name} : {diff}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{} écart(s) avec l'oracle :\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn ef_imp_02_corpus_matches_bruno_grouped_by_tags() {
    check_corpus(GroupBy::Tags, "tags");
}

#[test]
fn ef_imp_02_corpus_matches_bruno_grouped_by_path() {
    check_corpus(GroupBy::Path, "path");
}

#[test]
fn ef_imp_02_operation_keys_are_unique_and_last() {
    for (name, text) in specs() {
        for group_by in [GroupBy::Tags, GroupBy::Path] {
            let Ok(collection) = convert(&text, group_by) else { continue };
            let mut seen = HashSet::new();
            for request in requests(&collection["items"]) {
                let key = request["operationKey"].as_str().unwrap_or_else(|| panic!("{name} : clé absente"));
                assert!(seen.insert(key.to_owned()), "{name} : clé dupliquée {key}");
                assert_eq!(request.as_object().unwrap().keys().next_back().unwrap(), "operationKey", "{name}");
            }
        }
    }
}

#[test]
fn ef_imp_02_operation_keys_do_not_depend_on_grouping() {
    for (name, text) in specs() {
        let keys = |g| -> Option<HashSet<String>> {
            let c = convert(&text, g).ok()?;
            Some(requests(&c["items"]).iter().map(|r| r["operationKey"].as_str().unwrap().to_owned()).collect())
        };
        assert_eq!(keys(GroupBy::Tags), keys(GroupBy::Path), "{name}");
    }
}

#[test]
fn ef_imp_02_operation_keys_use_operation_id_or_normalized_path() {
    let collection = to_bruno(&spec("edge-operation-keys"), GroupBy::Path).unwrap();
    let mut keys: Vec<_> = requests(&collection["items"]).iter().map(|r| r["operationKey"].as_str().unwrap()).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "DELETE /users/{}",
            "GET /users/{}/posts/{}",
            "GET /users/{}/posts/{} #2",
            "PUT /users/{}",
            "listUsers (GET /admin/users)",
            "listUsers (GET /users)",
            "listUsers (POST /users)",
        ]
    );
}

fn keys_of(text: &str) -> Vec<String> {
    let collection = convert(text, GroupBy::Tags).unwrap();
    requests(&collection["items"]).iter().map(|r| r["operationKey"].as_str().unwrap().to_owned()).collect()
}

#[test]
fn ef_imp_02_a_shared_operation_id_is_told_apart_by_method_and_path_not_by_rank() {
    let both = "openapi: 3.0.0
info: {title: D, version: 1.0.0}
paths:
  /a:
    get: {operationId: dup, responses: {'200': {description: ok}}}
  /b:
    get: {operationId: dup, responses: {'200': {description: ok}}}
";
    let only_b = "openapi: 3.0.0
info: {title: D, version: 1.0.0}
paths:
  /a:
    get: {operationId: other, responses: {'200': {description: ok}}}
  /b:
    get: {operationId: dup, responses: {'200': {description: ok}}}
  /c:
    get: {operationId: dup, responses: {'200': {description: ok}}}
";
    assert_eq!(keys_of(both), ["dup (GET /a)", "dup (GET /b)"]);
    let swapped = both.replace("/a:", "/tmp:").replace("/b:", "/a:").replace("/tmp:", "/b:");
    let mut keys = keys_of(&swapped);
    keys.sort();
    assert_eq!(keys, ["dup (GET /a)", "dup (GET /b)"], "la clé ne dépend pas de l'ordre dans la spec");
    assert_eq!(keys_of(only_b), ["other", "dup (GET /b)", "dup (GET /c)"]);

    let variants = "openapi: 3.0.0
info: {title: D, version: 1.0.0}
paths:
  /users/{id}:
    get:
      operationId: dup
      x-bruno-variants:
        - {operationId: dup, summary: variante}
      responses: {'200': {description: ok}}
  /users/{name}:
    get: {operationId: dup, responses: {'200': {description: ok}}}
";
    assert_eq!(
        keys_of(variants),
        ["dup (GET /users/{})", "dup (GET /users/{}) #2", "dup (GET /users/{}) #3"],
        "méthode et chemin normalisé identiques : ` #n` en dernier recours"
    );
}

#[test]
fn ef_imp_02_summary_describes_the_spec() {
    let s = summary(&spec("oai-petstore-expanded-3.0"));
    assert_eq!(s.title, "Swagger Petstore");
    assert_eq!(s.version.as_deref(), Some("1.0.0"));
    assert_eq!((s.format.as_str(), s.format_version.as_deref()), ("openapi", Some("3.0.0")));
    assert_eq!(s.operation_count, 4);
    assert_eq!(s.servers, ["https://petstore.swagger.io/v2"]);

    let s = summary(&spec("oai-petstore-2.0"));
    assert_eq!((s.format.as_str(), s.format_version.as_deref()), ("swagger", Some("2.0")));
    assert_eq!(s.operation_count, 3);
    assert_eq!(s.tags, ["pets"]);
    assert_eq!(s.servers, ["http://petstore.swagger.io/v1"]);

    let value = serde_json::to_value(summary(&spec("bump-train-travel"))).unwrap();
    let keys: Vec<_> = value.as_object().unwrap().keys().cloned().collect();
    assert_eq!(keys, ["title", "version", "format", "formatVersion", "operationCount", "tags", "servers"]);
}

#[test]
fn ef_imp_02_summary_counts_what_the_import_creates() {
    for (name, text) in specs() {
        let spec = load_spec(&text).unwrap();
        if let Ok(collection) = to_bruno(&spec, GroupBy::Tags) {
            assert_eq!(summary(&spec).operation_count, requests(&collection["items"]).len(), "{name}");
        }
    }
}

#[test]
fn ef_imp_02_invalid_input_is_rejected() {
    assert!(matches!(load_spec(""), Err(OpenApiError::Empty)));
    assert!(matches!(load_spec("openapi: 3.0.0\ninfo: [\n"), Err(OpenApiError::Syntax(_))));
    assert!(matches!(load_spec("a: 1\na: 2\n"), Err(OpenApiError::Syntax(_))));
    assert!(matches!(to_bruno(&Value::Null, GroupBy::Tags), Err(OpenApiError::Invalid(_))));
    assert!(matches!(to_bruno(&json!("texte"), GroupBy::Tags), Err(OpenApiError::Invalid(_))));
    assert!(matches!(to_bruno(&json!({"openapi": "3.1.0"}), GroupBy::Tags), Err(OpenApiError::Invalid(_))));
    let null_path = json!({"openapi": "3.0.0", "paths": {"/a": null}});
    assert!(matches!(to_bruno(&null_path, GroupBy::Tags), Err(OpenApiError::Invalid(_))));
    let numeric_description = json!({"openapi": "3.0.0", "paths": {"/a": {"get": {
        "parameters": [{"name": "q", "in": "query", "description": 5}]
    }}}});
    assert!(matches!(to_bruno(&numeric_description, GroupBy::Tags), Err(OpenApiError::Schema(_))));
    let message = to_bruno(&null_path, GroupBy::Tags).unwrap_err().to_string();
    assert!(message.starts_with("spécification impossible à convertir"), "{message}");
}

#[test]
fn ef_imp_02_swagger_is_detected_from_its_version() {
    let swagger = json!({"swagger": 2.0, "info": {"title": " API "}, "paths": {"/a": {"get": {}}}});
    let collection = to_bruno(&swagger, GroupBy::Tags).unwrap();
    assert_eq!(collection["name"], "API");
    assert_eq!(collection["items"][0]["request"]["script"], json!({"res": null}));
    assert_eq!(collection["items"][0]["operationKey"], "GET /a");
}

/// Spec dont la réponse de `/a` renvoie `S0` ; `component(i)` est le corps du schéma `S<i>`, `i` de 0 à `levels`.
fn schema_spec(levels: usize, component: impl Fn(usize) -> String) -> String {
    let mut text = String::from(
        "openapi: 3.0.0\ninfo: {title: t, version: '1'}\npaths:\n  /a:\n    get:\n      responses:\n        '200':\n          description: ok\n          content:\n            application/json:\n              schema:\n                $ref: '#/components/schemas/S0'\ncomponents:\n  schemas:\n",
    );
    for level in 0..=levels {
        text.push_str(&format!("    S{level}:\n{}", component(level)));
    }
    text
}

fn elapsed<T>(work: impl FnOnce() -> T) -> (T, std::time::Duration) {
    let start = std::time::Instant::now();
    (work(), start.elapsed())
}

#[test]
fn ef_imp_02_a_long_chain_of_references_is_refused_instead_of_overflowing_the_stack() {
    let links = 1_500;
    let text = schema_spec(links, |i| match i {
        i if i == links => "      type: string\n".into(),
        i => format!(
            "      type: object\n      properties:\n        next:\n          $ref: '#/components/schemas/S{}'\n",
            i + 1
        ),
    });
    let error = convert(&text, GroupBy::Tags).unwrap_err();
    assert!(error.to_string().contains("récursion trop profonde"), "{error}");
    let shallow = schema_spec(20, |i| match i {
        20 => "      type: string\n".into(),
        i => format!(
            "      type: object\n      properties:\n        next:\n          $ref: '#/components/schemas/S{}'\n",
            i + 1
        ),
    });
    assert!(convert(&shallow, GroupBy::Tags).is_ok());
}

#[test]
fn ef_imp_02_references_fanning_out_cannot_blow_up_the_generated_examples() {
    let levels = 40;
    let text = schema_spec(levels, |i| {
        match i {
        i if i == levels => "      type: string\n".into(),
        i => format!(
            "      type: object\n      properties:\n        a:\n          $ref: '#/components/schemas/S{n}'\n        b:\n          $ref: '#/components/schemas/S{n}'\n",
            n = i + 1
        ),
    }
    });
    let (result, time) = elapsed(|| convert(&text, GroupBy::Tags));
    assert!(result.unwrap_err().to_string().contains("exemples trop volumineux"));
    assert!(time.as_secs() < 2, "{time:?}");
}

#[test]
fn ef_imp_02_shared_examples_fanning_out_cannot_blow_up_the_serialized_bodies() {
    let levels = 40;
    let mut text = String::from(
        "openapi: 3.0.0\ninfo: {title: t, version: '1'}\npaths:\n  /a:\n    post:\n      requestBody:\n        content:\n          application/json:\n            schema:\n              example:\n                $ref: '#/components/examples/E0'\n      responses:\n        '200': {description: ok}\ncomponents:\n  examples:\n",
    );
    for level in 0..=levels {
        let child = format!("{{$ref: '#/components/examples/E{}'}}", level + 1);
        let body = if level == levels { "1".into() } else { format!("[{child}, {child}]") };
        text.push_str(&format!("    E{level}: {body}\n"));
    }
    let (result, time) = elapsed(|| convert(&text, GroupBy::Tags));
    assert!(result.unwrap_err().to_string().contains("exemples trop volumineux"));
    assert!(time.as_secs() < 2, "{time:?}");
}

#[test]
fn ef_imp_02_yaml_alias_bombs_are_refused_in_bounded_time() {
    let mut text = String::from("a0: &a0 [x,x,x,x,x,x,x,x,x,x]\n");
    for level in 1..8 {
        let children = vec![format!("*a{}", level - 1); 10].join(",");
        text.push_str(&format!("a{level}: &a{level} [{children}]\n"));
    }
    let (result, time) = elapsed(|| load_spec(&text));
    assert!(matches!(&result, Err(OpenApiError::Syntax(message)) if message.contains("alias YAML")), "{result:?}");
    assert!(time.as_secs() < 2, "{time:?}");

    let modest = "base: &b {x: 1, y: [1, 2, 3]}\nuses: [*b, *b, *b]\n";
    assert_eq!(load_spec(modest).unwrap()["uses"][2]["y"], json!([1, 2, 3]));
}
