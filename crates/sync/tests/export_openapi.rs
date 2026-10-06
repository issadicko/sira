use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use xc_sync::export::openapi;
use xc_sync::openapi::{load_spec, to_bruno, GroupBy};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

const COLLECTION: &str = "opencollection: 1.0.0\n\ninfo:\n  name: Paiements\n  version: 2.3.0\n\nrequest:\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n  variables:\n    - name: baseUrl\n      value: https://api.example.com/v1\n\ndocs:\n  content: \"# Paiements\"\n  type: text/markdown\nextensions:\n  bruno:\n    presets:\n      defaultEnvironment: Dev\n";

fn http(name: &str, seq: u32, method: &str, url: &str, rest: &str) -> String {
    format!(
        "info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {method}\n  url: \"{url}\"\n  auth: inherit\n{rest}"
    )
}

fn exported_from(collection: &str, build: impl FnOnce(&Path)) -> (Value, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "opencollection.yml", collection);
    build(dir.path());
    let out = openapi(dir.path()).unwrap();
    (serde_json::from_str(&out.text).unwrap(), out.issues)
}

fn exported(build: impl FnOnce(&Path)) -> (Value, Vec<String>) {
    exported_from(COLLECTION, build)
}

#[test]
fn ef_imp_03_the_document_has_its_header_server_security_and_paths() {
    let (json, issues) = exported(|root| write(root, "ping.yml", &http("Ping", 1, "GET", "{{baseUrl}}/ping", "")));

    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(json["openapi"], "3.0.3");
    assert_eq!(json["info"], json!({ "title": "Paiements", "description": "# Paiements", "version": "2.3.0" }));
    assert_eq!(json["servers"], json!([{ "url": "https://api.example.com/v1" }]));
    assert_eq!(json["security"], json!([{ "bearerAuth": [] }]));
    assert_eq!(json["components"]["securitySchemes"]["bearerAuth"], json!({ "type": "http", "scheme": "bearer" }));
    assert_eq!(
        json["paths"]["/ping"]["get"],
        json!({
            "summary": "Ping",
            "operationId": "ping",
            "responses": { "200": { "description": "Réponse 200" } },
        })
    );
}

#[test]
fn ef_imp_03_without_a_version_the_collection_is_1_0_0() {
    let (json, _) = exported_from("opencollection: 1.0.0\n\ninfo:\n  name: Sans version\n", |_| {});

    assert_eq!(json["info"], json!({ "title": "Sans version", "version": "1.0.0" }));
    assert_eq!(json["paths"], json!({}));
    assert!(json.get("servers").is_none() && json.get("security").is_none() && json.get("components").is_none());
}

#[test]
fn ef_imp_03_a_folder_is_a_tag_with_its_docs_and_its_requests_carry_summary_docs_and_parameters() {
    let rest = "  headers:\n    - name: X-Trace\n      value: abc\n    - name: Accept\n      value: application/json\n    - name: X-Off\n      value: \"1\"\n      disabled: true\n  params:\n    - name: verbose\n      value: \"true\"\n      type: query\n      description: Détail\n    - name: page\n      value: \"2\"\n      type: query\n    - name: skipped\n      value: x\n      type: query\n      disabled: true\n    - name: id\n      value: \"7\"\n      type: path\n\nruntime:\n  assertions:\n    - expression: res.body.id\n      operator: eq\n      value: \"7\"\n    - expression: res.status\n      operator: eq\n      value: \"203\"\n\ndocs: Lit un utilisateur\n";
    let (json, _) = exported(|root| {
        write(root, "Users/folder.yml", "info:\n  name: Users\n  type: folder\n  seq: 1\n\ndocs: Les utilisateurs\n");
        write(root, "Users/get.yml", &http("Get user by ID", 1, "GET", "{{baseUrl}}/users/:id?verbose=true", rest));
        write(root, "Users/Admin/folder.yml", "info:\n  name: Admin\n  type: folder\n  seq: 1\n");
        write(root, "Users/Admin/ban.yml", &http("Ban", 1, "POST", "{{baseUrl}}/users/{{id}}/ban", ""));
        write(root, "Empty/folder.yml", "info:\n  name: Empty\n  type: folder\n  seq: 2\n");
    });

    assert_eq!(
        json["tags"],
        json!([{ "name": "Users / Admin" }, { "name": "Users", "description": "Les utilisateurs" }]),
        "seuls les dossiers qui portent une opération sont déclarés"
    );
    let get = &json["paths"]["/users/{id}"]["get"];
    assert_eq!(get["tags"], json!(["Users"]));
    assert_eq!(get["summary"], "Get user by ID");
    assert_eq!(get["operationId"], "getUserById");
    assert_eq!(get["description"], "Lit un utilisateur");
    assert_eq!(
        get["parameters"],
        json!([
            { "name": "id", "in": "path", "required": true, "schema": { "type": "integer" }, "example": 7 },
            { "name": "verbose", "in": "query", "description": "Détail", "schema": { "type": "boolean" }, "example": true },
            { "name": "page", "in": "query", "schema": { "type": "integer" }, "example": 2 },
            { "name": "X-Trace", "in": "header", "schema": { "type": "string" }, "example": "abc" },
        ]),
        "ni Accept, ni les paramètres désactivés"
    );
    assert_eq!(get["responses"], json!({ "203": { "description": "Réponse 203" } }));
    let ban = &json["paths"]["/users/{id}/ban"]["post"];
    assert_eq!(ban["tags"], json!(["Users / Admin"]));
    assert_eq!(
        ban["parameters"],
        json!([{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }])
    );
}

#[test]
fn ef_imp_03_operation_identifiers_are_unique_camel_case_names() {
    let (json, issues) = exported(|root| {
        write(root, "a.yml", &http("List all items", 1, "GET", "{{baseUrl}}/a", ""));
        write(root, "b.yml", &http("List all items", 2, "GET", "{{baseUrl}}/b", ""));
        write(root, "c.yml", &http("'!!!'", 3, "GET", "{{baseUrl}}/c", ""));
    });

    assert!(issues.is_empty(), "{issues:?}");
    let ids: Vec<&str> =
        ["/a", "/b", "/c"].iter().map(|p| json["paths"][p]["get"]["operationId"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["listAllItems", "listAllItems_2", "request"]);
}

fn environments(root: &Path) {
    write(root, "environments/Dev.yml", "name: Dev\nvariables:\n  - name: baseUrl\n    value: http://localhost:8080\n  - name: secretUrl\n    secret: true\n");
    write(root, "environments/Prod.yml", "name: Prod\nvariables:\n  - name: baseUrl\n    value: https://api.example.com/v1\n  - name: other\n    value: https://other.test\n");
    write(
        root,
        "environments/Staging.yml",
        "name: Staging\nvariables:\n  - name: baseUrl\n    value: https://staging.example.com\n",
    );
}

#[test]
fn ef_imp_03_the_values_of_a_variable_in_each_environment_are_the_servers_default_environment_first() {
    let (json, issues) = exported(|root| {
        environments(root);
        write(root, "ping.yml", &http("Ping", 1, "GET", "{{baseUrl}}/ping", ""));
    });

    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(
        json["servers"],
        json!([
            { "url": "http://localhost:8080", "description": "Dev" },
            { "url": "https://api.example.com/v1", "description": "Prod" },
            { "url": "https://staging.example.com", "description": "Staging" },
        ]),
        "la valeur de la collection est celle de Prod : elle n'est pas répétée"
    );
}

#[test]
fn ef_imp_03_the_most_used_prefix_is_global_and_the_others_belong_to_their_operations() {
    let (json, _) = exported(|root| {
        write(root, "a.yml", &http("A", 1, "GET", "{{baseUrl}}/a", ""));
        write(root, "b.yml", &http("B", 2, "GET", "{{baseUrl}}/b", ""));
        write(root, "c.yml", &http("C", 3, "GET", "https://billing.test:8443/c", ""));
        write(root, "d.yml", &http("D", 4, "GET", "localhost:3000/d", ""));
        write(root, "e.yml", &http("E", 5, "GET", "/relative", ""));
    });

    assert_eq!(json["servers"], json!([{ "url": "https://api.example.com/v1" }]));
    assert!(json["paths"]["/a"]["get"].get("servers").is_none());
    assert_eq!(json["paths"]["/c"]["get"]["servers"], json!([{ "url": "https://billing.test:8443" }]));
    assert_eq!(json["paths"]["/d"]["get"]["servers"], json!([{ "url": "http://localhost:3000" }]));
    assert!(json["paths"]["/relative"]["get"].get("servers").is_none());
}

#[test]
fn ef_imp_03_a_variable_inside_the_origin_becomes_a_server_variable_with_its_known_value_as_default() {
    let (json, issues) = exported(|root| {
        write(root, "a.yml", &http("A", 1, "GET", "https://{{tenant}}.example.com/a", ""));
        write(root, "b.yml", &http("B", 2, "GET", "{{missing}}/b", ""));
    });

    assert_eq!(
        json["servers"],
        json!([{ "url": "https://{tenant}.example.com", "variables": { "tenant": { "default": "" } } }])
    );
    assert_eq!(
        json["paths"]["/b"]["get"]["servers"],
        json!([{ "url": "{missing}", "variables": { "missing": { "default": "" } } }])
    );
    assert!(issues.iter().any(|i| i.contains("{{missing}}") && i.contains("aucune valeur")), "{issues:?}");
}

#[test]
fn ef_imp_03_every_body_kind_has_its_media_type_and_schema() {
    let (json, issues) = exported(|root| {
        write(root, "json.yml", &http("Json", 1, "POST", "{{baseUrl}}/json", "  body:\n    type: json\n    data: '{\"name\": \"Ada\", \"age\": 36, \"score\": 1.5, \"tags\": [\"a\"], \"address\": {\"city\": \"Paris\"}, \"none\": null, \"ok\": true}'\n"));
        write(root, "xml.yml", &http("Xml", 2, "POST", "{{baseUrl}}/xml", "  body:\n    type: xml\n    data: <a/>\n"));
        write(root, "text.yml", &http("Text", 3, "PUT", "{{baseUrl}}/text", "  body:\n    type: text\n    data: hi\n"));
        write(root, "form.yml", &http("Form", 4, "POST", "{{baseUrl}}/form", "  body:\n    type: form-urlencoded\n    data:\n      - name: a\n        value: \"1\"\n      - name: b\n        value: x\n        description: Lettre\n      - name: off\n        value: y\n        disabled: true\n"));
        write(root, "multi.yml", &http("Multi", 5, "POST", "{{baseUrl}}/multi", "  body:\n    type: multipart-form\n    data:\n      - name: f\n        type: file\n        value:\n          - /tmp/a.png\n        contentType: image/png\n      - name: many\n        type: file\n        value:\n          - /tmp/a.png\n          - /tmp/b.png\n      - name: t\n        type: text\n        value: hello\n"));
        write(
            root,
            "bad.yml",
            &http("Bad", 6, "POST", "{{baseUrl}}/bad", "  body:\n    type: json\n    data: '{\"id\": {{id}}}'\n"),
        );
        write(root, "none.yml", &http("None", 7, "POST", "{{baseUrl}}/none", ""));
    });

    let body = |path: &str, method: &str| json["paths"][path][method]["requestBody"].clone();
    let json_body = body("/json", "post");
    assert_eq!(json_body["required"], true);
    assert_eq!(
        json_body["content"]["application/json"]["schema"],
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" },
                "score": { "type": "number" },
                "tags": { "type": "array", "items": { "type": "string" } },
                "address": { "type": "object", "properties": { "city": { "type": "string" } } },
                "none": { "nullable": true },
                "ok": { "type": "boolean" },
            },
        })
    );
    assert_eq!(json_body["content"]["application/json"]["example"]["address"], json!({ "city": "Paris" }));
    assert_eq!(
        body("/xml", "post")["content"]["application/xml"],
        json!({ "schema": { "type": "string" }, "example": "<a/>" })
    );
    assert_eq!(
        body("/text", "put")["content"]["text/plain"],
        json!({ "schema": { "type": "string" }, "example": "hi" })
    );
    assert_eq!(
        body("/form", "post")["content"]["application/x-www-form-urlencoded"]["schema"],
        json!({
            "type": "object",
            "properties": {
                "a": { "type": "integer", "example": 1 },
                "b": { "type": "string", "example": "x", "description": "Lettre" },
            },
        })
    );
    let multipart = &body("/multi", "post")["content"]["multipart/form-data"];
    assert_eq!(
        multipart["schema"]["properties"],
        json!({
            "f": { "type": "string", "format": "binary" },
            "many": { "type": "array", "items": { "type": "string", "format": "binary" } },
            "t": { "type": "string", "example": "hello" },
        })
    );
    assert_eq!(multipart["encoding"], json!({ "f": { "contentType": "image/png" } }));
    assert_eq!(body("/bad", "post")["content"]["application/json"]["example"], "{\"id\": {{id}}}");
    assert!(issues.iter().any(|i| i.contains("Bad") && i.contains("JSON illisible")), "{issues:?}");
    assert!(json["paths"]["/none"]["post"].get("requestBody").is_none());
}

#[test]
fn ef_imp_03_a_graphql_request_posts_its_query_and_variables_as_json() {
    let (json, _) = exported(|root| {
        write(root, "g.yml", "info:\n  name: G\n  type: graphql\n  seq: 1\n\ngraphql:\n  method: POST\n  url: \"{{baseUrl}}/graphql\"\n  body:\n    query: '{ me { id } }'\n    variables: '{\"a\": 1}'\n  auth: inherit\n");
    });

    let media = &json["paths"]["/graphql"]["post"]["requestBody"]["content"]["application/json"];
    assert_eq!(media["schema"]["required"], json!(["query"]));
    assert_eq!(media["example"], json!({ "query": "{ me { id } }", "variables": { "a": 1 } }));
}

#[test]
fn ef_imp_03_authentication_is_inherited_and_repeated_only_where_it_differs() {
    let (json, issues) = exported(|root| {
        write(
            root,
            "Open/folder.yml",
            "info:\n  name: Open\n  type: folder\n  seq: 1\n\nrequest:\n  auth:\n    type: none\n",
        );
        write(root, "Open/a.yml", &http("A", 1, "GET", "{{baseUrl}}/open", ""));
        write(root, "Keyed/folder.yml", "info:\n  name: Keyed\n  type: folder\n  seq: 2\n\nrequest:\n  auth:\n    type: apikey\n    key: X-Api-Key\n    value: abc\n    placement: header\n");
        write(root, "Keyed/b.yml", &http("B", 1, "GET", "{{baseUrl}}/keyed", ""));
        write(
            root,
            "Keyed/c.yml",
            &http("C", 2, "GET", "{{baseUrl}}/keyed-basic", "")
                .replace("auth: inherit", "auth:\n    type: basic\n    username: u\n    password: p"),
        );
        write(
            root,
            "Plain/folder.yml",
            "info:\n  name: Plain\n  type: folder\n  seq: 3\n\nrequest:\n  auth: inherit\n",
        );
        write(root, "Plain/d.yml", &http("D", 1, "GET", "{{baseUrl}}/inherited", ""));
        write(
            root,
            "e.yml",
            &http("E", 4, "GET", "{{baseUrl}}/query-key", "").replace(
                "auth: inherit",
                "auth:\n    type: apikey\n    key: api_key\n    value: k\n    placement: query",
            ),
        );
        write(
            root,
            "f.yml",
            &http("F", 5, "GET", "{{baseUrl}}/exotic", "")
                .replace("auth: inherit", "auth:\n    type: ntlm\n    username: u"),
        );
        write(root, "g.yml", &http("G", 6, "GET", "{{baseUrl}}/aws", "").replace("auth: inherit", "auth:\n    type: awsv4\n    accessKeyId: AK\n    secretAccessKey: SK\n    service: s3\n    region: eu-west-1"));
        write(
            root,
            "h.yml",
            &http("H", 7, "GET", "{{baseUrl}}/digest", "")
                .replace("auth: inherit", "auth:\n    type: digest\n    username: u\n    password: p"),
        );
    });

    let security = |path: &str| json["paths"][path]["get"].get("security").cloned();
    assert_eq!(security("/open"), Some(json!([])), "aucune authentification là où la collection en demande une");
    assert_eq!(security("/keyed"), Some(json!([{ "apiKey_X-Api-Key": [] }])));
    assert_eq!(security("/keyed-basic"), Some(json!([{ "basicAuth": [] }])));
    assert_eq!(security("/inherited"), None);
    assert_eq!(security("/query-key"), Some(json!([{ "apiKey_api_key": [] }])));
    assert_eq!(security("/aws"), Some(json!([{ "awsSigV4": [] }])));
    assert_eq!(security("/digest"), Some(json!([{ "digestAuth": [] }])));
    let schemes = &json["components"]["securitySchemes"];
    assert_eq!(schemes["apiKey_X-Api-Key"], json!({ "type": "apiKey", "in": "header", "name": "X-Api-Key" }));
    assert_eq!(schemes["apiKey_api_key"], json!({ "type": "apiKey", "in": "query", "name": "api_key" }));
    assert_eq!(schemes["digestAuth"], json!({ "type": "http", "scheme": "digest" }));
    assert_eq!(schemes["awsSigV4"]["in"], "header");
    assert!(issues.iter().any(|i| i.contains("F") && i.contains("ntlm")), "{issues:?}");
    assert_eq!(security("/exotic"), Some(json!([])));
}

#[test]
fn ef_imp_03_oauth2_flows_keep_their_urls_scopes_and_resolve_known_variables() {
    let collection = "opencollection: 1.0.0\n\ninfo:\n  name: O\n\nrequest:\n  variables:\n    - name: idp\n      value: https://idp.test\n  auth:\n    type: oauth2\n    flow: client_credentials\n    accessTokenUrl: \"{{idp}}/token\"\n    refreshTokenUrl: \"{{idp}}/refresh\"\n    clientId: id\n    clientSecret: secret\n    scope: read write\n";
    let (json, issues) = exported_from(collection, |root| {
        write(root, "a.yml", &http("A", 1, "GET", "https://x.test/a", ""));
        write(root, "b.yml", &http("B", 2, "GET", "https://x.test/b", "").replace("auth: inherit", "auth:\n    type: oauth2\n    flow: authorization_code\n    authorizationUrl: \"{{idp}}/authorize\"\n    accessTokenUrl: \"{{idp}}/token\"\n    clientId: id\n    scope: openid"));
        write(root, "c.yml", &http("C", 3, "GET", "https://x.test/c", "").replace("auth: inherit", "auth:\n    type: oauth2\n    flow: resource_owner_password_credentials\n    accessTokenUrl: \"{{nowhere}}/token\"\n    username: u\n    password: p"));
        write(
            root,
            "d.yml",
            &http("D", 4, "GET", "https://x.test/d", "").replace(
                "auth: inherit",
                "auth:\n    type: oauth2\n    flow: implicit\n    authorizationUrl: https://idp.test/implicit",
            ),
        );
    });

    let schemes = &json["components"]["securitySchemes"];
    assert_eq!(
        schemes["oauth2ClientCredentials"],
        json!({ "type": "oauth2", "flows": { "clientCredentials": {
            "tokenUrl": "https://idp.test/token",
            "refreshUrl": "https://idp.test/refresh",
            "scopes": { "read": "", "write": "" },
        } } })
    );
    assert_eq!(json["security"], json!([{ "oauth2ClientCredentials": ["read", "write"] }]));
    assert_eq!(
        schemes["oauth2AuthorizationCode"]["flows"]["authorizationCode"],
        json!({ "authorizationUrl": "https://idp.test/authorize", "tokenUrl": "https://idp.test/token", "scopes": { "openid": "" } })
    );
    assert_eq!(json["paths"]["/b"]["get"]["security"], json!([{ "oauth2AuthorizationCode": ["openid"] }]));
    assert_eq!(schemes["oauth2Password"]["flows"]["password"]["tokenUrl"], "{{nowhere}}/token");
    assert_eq!(
        schemes["oauth2Implicit"]["flows"]["implicit"],
        json!({ "authorizationUrl": "https://idp.test/implicit", "scopes": {} })
    );
    assert!(issues.iter().any(|i| i.contains("C") && i.contains("OAuth2")), "{issues:?}");
}

#[test]
fn ef_imp_03_two_different_api_keys_with_the_same_name_get_distinct_schemes() {
    let (json, _) = exported_from("opencollection: 1.0.0\n\ninfo:\n  name: K\n", |root| {
        write(
            root,
            "a.yml",
            &http("A", 1, "GET", "https://x.test/a", "")
                .replace("auth: inherit", "auth:\n    type: apikey\n    key: K\n    value: a\n    placement: header"),
        );
        write(
            root,
            "b.yml",
            &http("B", 2, "GET", "https://x.test/b", "")
                .replace("auth: inherit", "auth:\n    type: apikey\n    key: K\n    value: b\n    placement: query"),
        );
    });

    let schemes = json["components"]["securitySchemes"].as_object().unwrap();
    assert_eq!(schemes.len(), 2);
    assert_eq!(json["paths"]["/a"]["get"]["security"], json!([{ "apiKey_K": [] }]));
    assert_eq!(json["paths"]["/b"]["get"]["security"], json!([{ "apiKey_K_2": [] }]));
    assert_eq!(schemes["apiKey_K_2"]["in"], "query");
}

#[test]
fn ef_imp_03_what_openapi_cannot_describe_is_reported_not_exported() {
    let (json, issues) = exported(|root| {
        write(root, "a.yml", &http("Lister", 1, "GET", "{{baseUrl}}/items", ""));
        write(root, "dup.yml", &http("Doublon", 2, "GET", "{{baseUrl}}/items", ""));
        write(
            root,
            "get-body.yml",
            &http("Corps sur GET", 3, "GET", "{{baseUrl}}/search", "  body:\n    type: json\n    data: '{}'\n"),
        );
        write(root, "grpc.yml", "info:\n  name: Grpc\n  type: grpc\n  seq: 4\n\ngrpc:\n  url: localhost:50051\n");
        write(root, "custom.yml", &http("Perso", 5, "PURGE", "{{baseUrl}}/cache", ""));
        write(
            root,
            "other.yml",
            &http("Autre", 6, "DELETE", "{{baseUrl}}/items", "  body:\n    type: json\n    data: '{\"all\": true}'\n"),
        );
    });

    assert_eq!(json["paths"]["/items"].as_object().unwrap().keys().collect::<Vec<_>>(), vec!["get", "delete"]);
    assert_eq!(json["paths"]["/items"]["get"]["summary"], "Lister", "le premier gagne");
    assert!(json["paths"]["/items"]["delete"].get("requestBody").is_some(), "DELETE garde son corps");
    assert!(json["paths"]["/search"]["get"].get("requestBody").is_none());
    assert!(json["paths"].get("/cache").is_none());
    for (needle, other) in
        [("Doublon", "doublon GET /items"), ("Corps sur GET", "corps ignoré"), ("Grpc", "grpc"), ("Perso", "PURGE")]
    {
        assert!(issues.iter().any(|i| i.contains(needle) && i.contains(other)), "{needle} : {issues:?}");
    }
}

#[test]
fn ef_imp_03_secrets_and_values_of_secret_variables_are_not_written() {
    let (json, _) = exported(|root| {
        environments(root);
        write(root, "a.yml", &http("A", 1, "GET", "{{secretUrl}}/a", ""));
    });

    let text = json.to_string();
    assert!(!text.contains("{{token}}"), "le jeton reste une variable, jamais sa valeur : {text}");
    assert_eq!(
        json["servers"],
        json!([{ "url": "{secretUrl}", "variables": { "secretUrl": { "default": "" } } }]),
        "la valeur d'un secret n'est jamais écrite : le serveur reste à renseigner"
    );
}

#[test]
fn ef_imp_03_the_exported_document_is_imported_back_with_its_operations() {
    let (json, _) = exported(|root| {
        environments(root);
        write(root, "Users/folder.yml", "info:\n  name: Users\n  type: folder\n  seq: 1\n");
        write(root, "Users/get.yml", &http("Get user", 1, "GET", "{{baseUrl}}/users/:id?verbose=true", "  params:\n    - name: verbose\n      value: \"true\"\n      type: query\n    - name: id\n      value: \"7\"\n      type: path\n"));
        write(
            root,
            "Users/create.yml",
            &http(
                "Create user",
                2,
                "POST",
                "{{baseUrl}}/users",
                "  body:\n    type: json\n    data: '{\"name\": \"Ada\"}'\n",
            ),
        );
        write(root, "ping.yml", &http("Ping", 3, "GET", "{{baseUrl}}/ping", ""));
    });

    let spec = load_spec(&json.to_string()).unwrap();
    let bruno = to_bruno(&spec, GroupBy::Tags).unwrap();

    fn flatten(items: &Value, out: &mut Vec<(String, String, String)>) {
        for item in items.as_array().into_iter().flatten() {
            if item["type"] == "folder" {
                flatten(&item["items"], out);
            } else {
                let request = &item["request"];
                out.push((
                    item["name"].as_str().unwrap().to_owned(),
                    request["method"].as_str().unwrap().to_owned(),
                    request["url"].as_str().unwrap().to_owned(),
                ));
            }
        }
    }
    let mut requests = Vec::new();
    flatten(&bruno["items"], &mut requests);
    requests.sort();
    let names: Vec<(&str, &str)> = requests.iter().map(|(n, m, _)| (n.as_str(), m.as_str())).collect();
    assert_eq!(names, vec![("Create user", "POST"), ("Get user", "GET"), ("Ping", "GET")]);
    assert!(requests.iter().all(|(_, _, url)| url.starts_with("{{baseUrl}}/")), "{requests:?}");
    assert!(requests.iter().any(|(_, _, url)| url.contains("/users/:id")), "{requests:?}");
    let folders: Vec<&str> = bruno["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["type"] == "folder")
        .map(|i| i["name"].as_str().unwrap())
        .collect();
    assert_eq!(folders, vec!["Users"]);
}
