use std::fs;
use std::path::Path;

use serde_json::{json, Value};
use xc_sync::export::postman;

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

const COLLECTION: &str = "opencollection: 1.0.0\n\ninfo:\n  name: Paiements\n\nrequest:\n  auth:\n    type: bearer\n    token: \"{{token}}\"\n  variables:\n    - name: baseUrl\n      value: https://api.example.com\n    - name: off\n      value: x\n      disabled: true\n  scripts:\n    - type: before-request\n      code: bru.setVar(\"ts\", Date.now());\n\ndocs:\n  content: \"# Paiements\"\n  type: text/markdown\nbundled: false\nextensions: {}\n";

fn http(name: &str, seq: u32, method: &str, url: &str, rest: &str) -> String {
    format!("info:\n  name: {name}\n  type: http\n  seq: {seq}\n\nhttp:\n  method: {method}\n  url: \"{url}\"\n{rest}")
}

fn exported(build: impl FnOnce(&Path)) -> (Value, Vec<String>) {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "opencollection.yml", COLLECTION);
    build(dir.path());
    let out = postman::collection(dir.path()).unwrap();
    (serde_json::from_str(&out.text).unwrap(), out.issues)
}

#[test]
fn ef_imp_03_the_collection_header_variables_auth_and_scripts_are_exported() {
    let (json, issues) = exported(|root| write(root, "ping.yml", &http("Ping", 1, "GET", "{{baseUrl}}/ping", "")));

    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(json["info"]["name"], "Paiements");
    assert_eq!(json["info"]["description"], "# Paiements");
    assert_eq!(json["info"]["schema"], "https://schema.getpostman.com/json/collection/v2.1.0/collection.json");
    assert_eq!(
        json["auth"],
        json!({ "type": "bearer", "bearer": [{ "key": "token", "value": "{{token}}", "type": "string" }] })
    );
    assert_eq!(json["event"][0]["listen"], "prerequest");
    assert_eq!(json["event"][0]["script"]["exec"], json!(["pm.variables.set(\"ts\", Date.now());"]));
    assert_eq!(json["event"][0]["script"]["type"], "text/javascript");
}

#[test]
fn ef_imp_03_collection_variables_come_from_the_file_then_from_the_placeholders_without_duplicates() {
    let (json, _) = exported(|root| write(root, "ping.yml", &http("Ping", 1, "GET", "{{baseUrl}}/ping/{{id}}", "")));

    let variables = json["variable"].as_array().unwrap();
    let keys: Vec<&str> = variables.iter().map(|v| v["key"].as_str().unwrap()).collect();
    assert_eq!(keys, vec!["baseUrl", "off", "id", "token"]);
    assert_eq!(variables[0], json!({ "key": "baseUrl", "value": "https://api.example.com", "type": "default" }));
    assert_eq!(variables[1]["disabled"], true);
    assert_eq!(variables[2], json!({ "key": "id", "value": "", "type": "default" }));
}

#[test]
fn ef_imp_03_a_request_has_method_headers_url_parts_description_and_events() {
    // Une adresse sans schéma (`{{baseUrl}}/…`) garde sa requête dans le dernier segment du chemin : c'est ce que fait Bruno.
    let rest = "  headers:\n    - name: Content-Type\n      value: application/json\n    - name: X-Old\n      value: \"1\"\n      description: ancien\n      disabled: true\n  params:\n    - name: verbose\n      value: \"true\"\n      type: query\n    - name: id\n      value: \"7\"\n      type: path\n  body:\n    type: json\n    data: '{\"a\": 1}'\n  auth:\n    type: basic\n    username: u\n    password: p\n\nruntime:\n  scripts:\n    - type: after-response\n      code: bru.setVar(\"id\", res.body.id);\n    - type: tests\n      code: test(\"ok\", function() { expect(res.status).to.equal(201); });\n\ndocs: Crée un utilisateur\n";
    let (json, _) = exported(|root| {
        write(root, "create.yml", &http("Create", 1, "POST", "{{baseUrl}}/users/:id?verbose=true", rest))
    });

    let item = &json["item"][0];
    assert_eq!(item["name"], "Create");
    let request = &item["request"];
    assert_eq!(request["method"], "POST");
    assert_eq!(request["description"], "Crée un utilisateur");
    assert_eq!(
        request["header"][0],
        json!({ "key": "Content-Type", "value": "application/json", "description": "", "disabled": false, "type": "default" })
    );
    assert_eq!(request["header"][1]["disabled"], true);
    assert_eq!(request["header"][1]["description"], "ancien");
    assert_eq!(
        request["url"],
        json!({
            "raw": "{{baseUrl}}/users/:id?verbose=true",
            "protocol": "",
            "host": ["{{baseUrl}}"],
            "path": ["users", ":id?verbose=true"],
            "query": [{ "key": "verbose", "value": "true" }],
            "variable": [{ "key": "id", "value": "7" }],
        })
    );
    assert_eq!(
        request["body"],
        json!({ "mode": "raw", "raw": "{\"a\": 1}", "options": { "raw": { "language": "json" } } })
    );
    assert_eq!(request["auth"]["type"], "basic");
    assert_eq!(request["auth"]["basic"][0], json!({ "key": "password", "value": "p", "type": "string" }));
    let test = &item["event"][0];
    assert_eq!(test["listen"], "test");
    let exec = test["script"]["exec"].as_array().unwrap();
    assert_eq!(exec[0], "pm.variables.set(\"id\", pm.response.json().id);");
    assert!(exec.contains(&json!("// Tests")));
    assert!(
        exec.iter().any(|l| l
            .as_str()
            .is_some_and(|l| l.starts_with("pm.test(\"ok\", function() { pm.expect(pm.response.code)"))),
        "{exec:?}"
    );
}

#[test]
fn ef_imp_03_the_url_follows_bruno_for_a_scheme_a_port_and_a_missing_scheme() {
    let (json, _) = exported(|root| {
        write(root, "a.yml", &http("A", 1, "GET", "https://api.example.com:8443/v1/items/", ""));
        write(root, "b.yml", &http("B", 2, "GET", "localhost:3000/ping?x=1", ""));
        write(root, "c.yml", &http("C", 3, "GET", "https://host//double///slash", ""));
        write(root, "d.yml", &http("D", 4, "GET", "https://host.test/", ""));
    });

    let url = |i: usize| json["item"][i]["request"]["url"].clone();
    assert_eq!(url(0)["host"], json!(["api", "example", "com:8443"]));
    assert_eq!(url(0)["path"], json!(["v1", "items", ""]));
    assert_eq!(url(1)["protocol"], "", "sans schéma, la requête reste dans le chemin comme dans Bruno");
    assert_eq!(url(1)["host"], json!(["localhost:3000"]));
    assert_eq!(url(1)["path"], json!(["ping?x=1"]));
    assert_eq!(url(2)["raw"], "https://host/double/slash");
    assert_eq!(url(3)["host"], json!(["host", "test/"]), "une barre finale seule reste dans l'hôte, comme dans Bruno");
    assert_eq!(url(3)["path"], json!([]));
}

#[test]
fn ef_imp_03_every_body_kind_has_its_postman_form() {
    let (json, _) = exported(|root| {
        write(root, "form.yml", &http("Form", 1, "POST", "https://x.test", "  body:\n    type: form-urlencoded\n    data:\n      - name: a\n        value: \"1\"\n      - name: b\n        value: \"2\"\n        disabled: true\n"));
        write(root, "multi.yml", &http("Multi", 2, "POST", "https://x.test", "  body:\n    type: multipart-form\n    data:\n      - name: f\n        type: file\n        value:\n          - /tmp/a.png\n        contentType: image/png\n      - name: t\n        type: text\n        value: hello\n"));
        write(root, "xml.yml", &http("Xml", 3, "POST", "https://x.test", "  body:\n    type: xml\n    data: <a/>\n"));
        write(root, "text.yml", &http("Text", 4, "POST", "https://x.test", "  body:\n    type: text\n    data: hi\n"));
        write(root, "none.yml", &http("None", 5, "POST", "https://x.test", ""));
    });

    let body = |i: usize| json["item"][i]["request"]["body"].clone();
    assert_eq!(body(0)["mode"], "urlencoded");
    assert_eq!(
        body(0)["urlencoded"][1],
        json!({ "key": "b", "value": "2", "disabled": true, "type": "default", "description": "" })
    );
    assert_eq!(body(1)["mode"], "formdata");
    assert_eq!(
        body(1)["formdata"][0],
        json!({ "key": "f", "disabled": false, "description": "", "type": "file", "src": "/tmp/a.png", "contentType": "image/png" })
    );
    assert_eq!(
        body(1)["formdata"][1],
        json!({ "key": "t", "disabled": false, "description": "", "type": "text", "value": "hello" })
    );
    assert_eq!(body(2)["options"]["raw"]["language"], "xml");
    assert_eq!(body(3)["options"]["raw"]["language"], "text");
    assert!(json["item"][4]["request"].get("body").is_none(), "pas de corps, pas de clé");
}

#[test]
fn ef_imp_03_a_graphql_request_keeps_its_query_and_variables() {
    let (json, _) = exported(|root| {
        write(root, "g.yml", "info:\n  name: G\n  type: graphql\n  seq: 1\n\ngraphql:\n  method: POST\n  url: https://x.test/graphql\n  body:\n    query: '{ me { id } }'\n    variables: '{\"a\": 1}'\n  auth: inherit\n");
    });

    assert_eq!(
        json["item"][0]["request"]["body"],
        json!({ "mode": "graphql", "graphql": { "query": "{ me { id } }", "variables": "{\"a\": 1}" } })
    );
    assert!(json["item"][0]["request"].get("auth").is_none(), "hérité : pas d'auth");
}

#[test]
fn ef_imp_03_a_get_with_a_body_asks_postman_not_to_prune_it() {
    let (json, _) = exported(|root| {
        write(root, "g.yml", &http("G", 1, "GET", "https://x.test", "  body:\n    type: json\n    data: '{}'\n"))
    });

    assert_eq!(json["item"][0]["protocolProfileBehavior"], json!({ "disableBodyPruning": true }));
}

#[test]
fn ef_imp_03_authentications_use_the_postman_key_lists() {
    let (json, issues) = exported(|root| {
        write(
            root,
            "k.yml",
            &http(
                "Key",
                1,
                "GET",
                "https://x.test",
                "  auth:\n    type: apikey\n    key: X-Key\n    value: v\n    placement: query\n",
            ),
        );
        write(
            root,
            "d.yml",
            &http(
                "Digest",
                2,
                "GET",
                "https://x.test",
                "  auth:\n    type: digest\n    username: u\n    password: p\n",
            ),
        );
        write(root, "a.yml", &http("Aws", 3, "GET", "https://x.test", "  auth:\n    type: awsv4\n    accessKeyId: AK\n    secretAccessKey: SK\n    service: s3\n    region: eu-west-1\n"));
        write(root, "n.yml", &http("None", 4, "GET", "https://x.test", "  auth: none\n"));
        write(root, "o.yml", &http("OAuth", 5, "GET", "https://x.test", "  auth:\n    type: oauth2\n    flow: authorization_code\n    pkce: {}\n    authorizationUrl: https://a.test/auth\n    accessTokenUrl: https://a.test/token\n    clientId: id\n    scope: read\n    tokenConfig:\n      id: cred\n"));
        write(root, "w.yml", &http("Ntlm", 6, "GET", "https://x.test", "  auth:\n    type: ntlm\n    username: u\n"));
    });

    let auth = |i: usize| json["item"][i]["request"]["auth"].clone();
    assert_eq!(auth(0)["apikey"][2], json!({ "key": "in", "value": "query", "type": "string" }));
    assert_eq!(auth(1)["type"], "digest");
    let aws_auth = auth(2);
    let aws: Vec<&str> = aws_auth["awsv4"].as_array().unwrap().iter().map(|e| e["key"].as_str().unwrap()).collect();
    assert_eq!(aws, vec!["sessionToken", "service", "region", "secretKey", "accessKey"]);
    assert_eq!(auth(3), json!({ "type": "noauth" }));
    let oauth = auth(4);
    assert_eq!(oauth["type"], "oauth2");
    let value =
        |key: &str| oauth["oauth2"].as_array().unwrap().iter().find(|e| e["key"] == key).map(|e| e["value"].clone());
    assert_eq!(value("grant_type"), Some(json!("authorization_code_with_pkce")));
    assert_eq!(value("authUrl"), Some(json!("https://a.test/auth")));
    assert_eq!(value("addTokenTo"), Some(json!("header")));
    assert_eq!(value("refreshTokenUrl"), None, "un champ vide n'est pas exporté");
    assert_eq!(auth(5), json!({ "type": "noauth" }));
    assert!(issues.iter().any(|i| i.contains("Ntlm") && i.contains("ntlm")), "{issues:?}");
}

#[test]
fn ef_imp_03_folders_come_first_in_bruno_order_each_with_its_auth_and_events() {
    let (json, _) = exported(|root| {
        write(root, "zeta.yml", &http("Zeta", 2, "GET", "https://x.test", ""));
        write(root, "alpha.yml", &http("Alpha", 1, "GET", "https://x.test", ""));
        write(root, "Users/folder.yml", "info:\n  name: Users\n  type: folder\n  seq: 2\n\nrequest:\n  auth:\n    type: basic\n    username: u\n    password: p\n  scripts:\n    - type: after-response\n      code: console.log(res.status)\n");
        write(root, "Users/one.yml", &http("One", 1, "GET", "https://x.test/one", ""));
        write(
            root,
            "Admin/folder.yml",
            "info:\n  name: Admin\n  type: folder\n  seq: 1\n\nrequest:\n  auth: inherit\n",
        );
        write(root, "Admin/two.yml", &http("Two", 1, "GET", "https://x.test/two", ""));
        write(root, "Misc/one.yml", &http("Misc one", 1, "GET", "https://x.test/misc", ""));
    });

    let names: Vec<&str> = json["item"].as_array().unwrap().iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec!["Admin", "Users", "Misc", "Alpha", "Zeta"],
        "dossiers par seq (sans seq : par nom, à la fin), puis requêtes par seq"
    );
    let users = &json["item"][1];
    assert_eq!(users["auth"]["type"], "basic");
    assert_eq!(users["event"][0]["script"]["exec"], json!(["console.log(pm.response.code)"]));
    assert_eq!(users["item"][0]["name"], "One");
    assert!(json["item"][0].get("auth").is_none(), "dossier qui hérite : pas d'auth");
}

#[test]
fn ef_imp_03_requests_postman_cannot_hold_are_skipped_and_reported() {
    let (json, issues) = exported(|root| {
        write(root, "ok.yml", &http("Ok", 1, "GET", "https://x.test", ""));
        write(root, "g.yml", "info:\n  name: Grpc\n  type: grpc\n  seq: 2\n\ngrpc:\n  url: localhost:50051\n");
    });

    assert_eq!(json["item"].as_array().unwrap().len(), 1);
    assert!(issues.iter().any(|i| i.contains("Grpc") && i.contains("grpc")), "{issues:?}");
}

#[test]
fn ef_imp_03_what_is_exported_imports_back_with_the_same_structure() {
    let dir = tempfile::tempdir().unwrap();
    write(dir.path(), "opencollection.yml", COLLECTION);
    write(dir.path(), "Users/folder.yml", "info:\n  name: Users\n  type: folder\n  seq: 1\n");
    write(dir.path(), "Users/create.yml", &http("Create", 1, "POST", "{{baseUrl}}/users", "  headers:\n    - name: X-Id\n      value: \"1\"\n  body:\n    type: json\n    data: '{\"a\": 1}'\n  auth:\n    type: bearer\n    token: t\n"));
    write(dir.path(), "ping.yml", &http("Ping", 1, "GET", "{{baseUrl}}/ping", ""));
    let out = postman::collection(dir.path()).unwrap();

    let back = xc_sync::postman::collection_from_text(&out.text).unwrap();

    assert!(back.issues.is_empty(), "{:?}", back.issues);
    let collection = &back.collection;
    assert_eq!(collection["name"], "Paiements");
    let items = collection["items"].as_array().unwrap();
    let names: Vec<&str> = items.iter().map(|i| i["name"].as_str().unwrap()).collect();
    assert_eq!(names, vec!["Users", "Ping"]);
    let create = &items[0]["items"][0];
    assert_eq!(create["request"]["method"], "POST");
    assert_eq!(create["request"]["url"], "{{baseUrl}}/users");
    assert_eq!(create["request"]["body"]["json"], "{\"a\": 1}");
    assert_eq!(create["request"]["auth"]["bearer"]["token"], "t");
}
