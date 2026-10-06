use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::process::Command;
use std::thread;

const SPEC: &str = r#"openapi: 3.0.3
info:
  title: Boutique
  version: "1.0"
servers:
  - url: https://api.boutique.test/v1
tags:
  - name: produits
paths:
  /produits:
    get:
      operationId: listerProduits
      summary: Lister les produits
      tags: [produits]
      responses:
        "200":
          description: OK
    post:
      operationId: creerProduit
      summary: Créer un produit
      tags: [produits]
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                nom:
                  type: string
      responses:
        "201":
          description: Créé
  /sante:
    get:
      summary: Santé
      responses:
        "200":
          description: OK
"#;

fn xc(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xc")).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn workspace() -> (tempfile::TempDir, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let spec = dir.path().join("boutique.yaml");
    fs::write(&spec, SPEC).unwrap();
    let out = dir.path().join("collections");
    fs::create_dir(&out).unwrap();
    let (spec, out) = (spec.to_str().unwrap().to_owned(), out.to_str().unwrap().to_owned());
    (dir, spec, out)
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

#[test]
fn ef_imp_02_cli_import_creates_the_collection_and_prints_its_path() {
    let (_dir, spec, out) = workspace();
    let (code, stdout, stderr) = xc(&["import", &spec, &out]);
    assert_eq!(code, 0, "{stderr}");
    let root = Path::new(stdout.trim());
    assert_eq!(root.file_name().unwrap(), "Boutique");
    assert_eq!(names(root), [".oc-sync", "Santé.yml", "environments", "opencollection.yml", "produits"]);
    assert_eq!(names(&root.join("produits")), ["Créer un produit.yml", "Lister les produits.yml", "folder.yml"]);
    let source = fs::read_to_string(root.join(".oc-sync/openapi/source.yml")).unwrap();
    assert!(
        source.starts_with("source: ../../boutique.yaml\ngroupBy: tags\nspec: spec.yaml\noperations:\n"),
        "{source}"
    );
    assert_eq!(names(&root.join(".oc-sync/openapi")), ["source.yml", "spec.yaml"]);
    assert_eq!(fs::read_to_string(root.join(".oc-sync/openapi/spec.yaml")).unwrap(), SPEC);

    let (code, checked, _) = xc(&["check", root.to_str().unwrap()]);
    assert_eq!(code, 0, "{checked}");
    assert!(checked.contains("0 renormalisé(s), 0 illisible(s)"), "{checked}");
}

#[test]
fn ef_imp_02_cli_import_group_by_path_and_second_import_does_not_overwrite() {
    let (_dir, spec, out) = workspace();
    let (code, first, stderr) = xc(&["import", &spec, &out, "--group-by", "path"]);
    assert_eq!(code, 0, "{stderr}");
    let root = Path::new(first.trim());
    assert_eq!(names(root), [".oc-sync", "environments", "opencollection.yml", "produits", "sante"]);
    assert_eq!(names(&root.join("sante")), ["Santé.yml", "folder.yml"]);
    let source = fs::read_to_string(root.join(".oc-sync/openapi/source.yml")).unwrap();
    assert!(source.contains("groupBy: path\n"), "{source}");

    let (code, second, stderr) = xc(&["import", &spec, &out, "--group-by", "path"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(Path::new(second.trim()).file_name().unwrap(), "Boutique - 1");
    assert!(root.join("sante/Santé.yml").is_file(), "la première collection est intacte");
}

#[test]
fn ef_imp_02_cli_import_reads_an_url() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let mut stream = listener.incoming().next().unwrap().unwrap();
        let mut buf = [0u8; 4096];
        let n = stream.read(&mut buf).unwrap();
        let request = String::from_utf8_lossy(&buf[..n]).to_lowercase();
        assert!(request.starts_with("get /openapi.yaml "), "{request}");
        assert!(request.contains("accept: application/json, application/yaml, */*"), "{request}");
        write!(
            stream,
            "HTTP/1.1 200 OK\r\ncontent-type: application/yaml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{SPEC}",
            SPEC.len()
        )
        .unwrap();
    });
    let dir = tempfile::tempdir().unwrap();
    let url = format!("http://{addr}/openapi.yaml");
    let (code, stdout, stderr) = xc(&["import", &url, dir.path().to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    let source = fs::read_to_string(Path::new(stdout.trim()).join(".oc-sync/openapi/source.yml")).unwrap();
    assert!(source.starts_with(&format!("source: {url}\n")), "{source}");
}

#[test]
fn ef_imp_02_cli_import_exit_codes() {
    let (dir, spec, out) = workspace();
    let missing = dir.path().join("absent.yaml");
    let (code, _, stderr) = xc(&["import", missing.to_str().unwrap(), &out]);
    assert_eq!(code, 2, "source illisible : {stderr}");
    assert!(stderr.starts_with("erreur : "), "{stderr}");

    let (code, _, stderr) = xc(&["import", &spec, dir.path().join("absent").to_str().unwrap()]);
    assert_eq!(code, 2, "dossier parent introuvable : {stderr}");

    let garbage = dir.path().join("garbage.yaml");
    fs::write(&garbage, "{{{ pas du yaml").unwrap();
    let (code, _, stderr) = xc(&["import", garbage.to_str().unwrap(), &out]);
    assert_eq!(code, 2, "spec illisible : {stderr}");

    let (code, _, _) = xc(&["import", &spec, &out, "--group-by", "folders"]);
    assert_eq!(code, 2, "usage");

    let invalid = dir.path().join("invalid.yaml");
    let schema_error = "openapi: 3.0.0\ninfo: { title: T, version: '1' }\npaths:\n  /a:\n    get:\n      parameters:\n        - { name: q, in: query, description: 42, schema: { type: string } }\n      responses: { '200': { description: ok } }\n";
    fs::write(&invalid, schema_error).unwrap();
    let (code, _, stderr) = xc(&["import", invalid.to_str().unwrap(), &out]);
    assert_eq!(code, 1, "conversion impossible : {stderr}");
    assert!(names(Path::new(&out)).is_empty(), "rien n'est écrit quand la conversion échoue");
}

const POSTMAN: &str = r#"{
  "info": { "name": "Boutique Postman", "schema": "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
  "item": [
    { "name": "Produits", "item": [ { "name": "Lister", "request": { "method": "GET", "url": { "raw": "https://api.boutique.test/produits" } } } ] },
    { "name": "Cassée", "request": { "url": "https://api.boutique.test/x" } }
  ]
}"#;

const POSTMAN_ENV: &str = r#"{ "name": "Recette", "values": [ { "key": "baseUrl", "value": "https://recette.boutique.test", "enabled": true } ] }"#;

#[test]
fn ef_imp_01_cli_import_postman_creates_the_collection_lists_what_it_skipped_and_adds_an_environment() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("boutique.postman_collection.json");
    fs::write(&file, POSTMAN).unwrap();
    let out = dir.path().join("collections");
    fs::create_dir(&out).unwrap();

    let (code, stdout, stderr) = xc(&["import-postman", file.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    let root = Path::new(stdout.trim());
    assert_eq!(root.file_name().unwrap(), "Boutique Postman");
    assert_eq!(names(root), ["Produits", "environments", "opencollection.yml"], "ni .oc-sync ni requête sans méthode");
    assert!(root.join("Produits/Lister.yml").is_file());
    assert!(stderr.contains("Cassée : Missing or invalid request method (ignoré)"), "{stderr}");

    let env = dir.path().join("recette.postman_environment.json");
    fs::write(&env, POSTMAN_ENV).unwrap();
    let (code, name, stderr) = xc(&["import-postman", "--environment", env.to_str().unwrap(), root.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(name.trim(), "Recette");
    assert!(root.join("environments/Recette.yml").is_file());

    let (code, checked, _) = xc(&["check", root.to_str().unwrap()]);
    assert_eq!(code, 0, "{checked}");
}

#[test]
fn ef_imp_01_cli_import_postman_refuses_an_unreadable_or_unsupported_file_with_code_2() {
    let dir = tempfile::tempdir().unwrap();
    let (code, _, stderr) =
        xc(&["import-postman", dir.path().join("absent.json").to_str().unwrap(), dir.path().to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("illisible"), "{stderr}");

    let old = dir.path().join("old.json");
    fs::write(&old, r#"{"info": {"name": "x", "schema": "https://schema.getpostman.com/json/collection/v1.0.0/collection.json"}, "item": []}"#).unwrap();
    let (code, _, stderr) = xc(&["import-postman", old.to_str().unwrap(), dir.path().to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("v2.0 et v2.1"), "{stderr}");
}

const INSOMNIA_V5: &str = r#"type: collection.insomnia.rest/5.0
name: Boutique Insomnia
collection:
  - name: Produits
    children:
      - name: Lister
        method: GET
        url: "{{ _.base }}/produits"
  - name: Orpheline
    meta:
      id: x
environments:
  name: Base
  data:
    base: https://api.boutique.test
"#;

#[test]
fn ef_imp_02_cli_import_insomnia_creates_the_collection_with_its_environments_and_lists_what_it_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("boutique.insomnia.yaml");
    fs::write(&file, INSOMNIA_V5).unwrap();
    let out = dir.path().join("collections");
    fs::create_dir(&out).unwrap();

    let (code, stdout, stderr) = xc(&["import-insomnia", file.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    let root = Path::new(stdout.trim());
    assert_eq!(root.file_name().unwrap(), "Boutique Insomnia");
    assert_eq!(names(root), ["Produits", "environments", "opencollection.yml"]);
    assert!(fs::read_to_string(root.join("Produits/Lister.yml")).unwrap().contains("{{base}}/produits"));
    assert!(root.join("environments/Base.yml").is_file());
    assert!(stderr.contains("Orpheline : Élément ignoré"), "{stderr}");

    let (code, checked, _) = xc(&["check", root.to_str().unwrap()]);
    assert_eq!(code, 0, "{checked}");
}

#[test]
fn ef_imp_02_cli_import_insomnia_refuses_an_unreadable_or_workspace_less_file_with_code_2() {
    let dir = tempfile::tempdir().unwrap();
    let (code, _, stderr) =
        xc(&["import-insomnia", dir.path().join("absent.json").to_str().unwrap(), dir.path().to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("illisible"), "{stderr}");

    let empty = dir.path().join("empty.json");
    fs::write(&empty, r#"{"resources": []}"#).unwrap();
    let (code, _, stderr) = xc(&["import-insomnia", empty.to_str().unwrap(), dir.path().to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("workspace"), "{stderr}");
}

fn bru_collection(dir: &Path) -> std::path::PathBuf {
    let source = dir.join("ancienne");
    fs::create_dir_all(source.join("Produits")).unwrap();
    fs::write(source.join("bruno.json"), r#"{"version": "1", "name": "Boutique Bru", "type": "collection"}"#).unwrap();
    fs::write(
        source.join("Produits/Lister.bru"),
        "meta {\n  name: Lister\n  type: http\n  seq: 1\n}\n\nget {\n  url: https://api.boutique.test/produits\n  body: none\n  auth: none\n}\n",
    )
    .unwrap();
    fs::write(source.join("Cassée.bru"), "meta {\n  name: Cassée\n}\n\nget {\n  url\n}\n").unwrap();
    fs::write(source.join("jeu.csv"), "a,b\n").unwrap();
    source
}

#[test]
fn ef_imp_03_cli_import_bru_converts_a_bru_collection_without_touching_the_source() {
    let dir = tempfile::tempdir().unwrap();
    let source = bru_collection(dir.path());
    let out = dir.path().join("collections");
    fs::create_dir(&out).unwrap();

    let (code, stdout, stderr) = xc(&["import-bru", source.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    let root = Path::new(stdout.trim());
    assert_eq!(root.file_name().unwrap(), "Boutique Bru");
    assert_eq!(names(root), ["Produits", "environments", "opencollection.yml"]);
    assert!(root.join("Produits/Lister.yml").is_file());
    assert!(stderr.contains("Cassée.bru : Requête ignorée : ligne 6"), "{stderr}");
    assert!(stderr.contains(".csv ×1"), "{stderr}");
    assert!(source.join("bruno.json").is_file() && source.join("Produits/Lister.bru").is_file());

    let (code, checked, _) = xc(&["check", root.to_str().unwrap()]);
    assert_eq!(code, 0, "{checked}");
}

#[test]
fn ef_imp_03_cli_import_bru_refuses_a_folder_without_bruno_json_with_code_2() {
    let dir = tempfile::tempdir().unwrap();
    let (code, _, stderr) = xc(&["import-bru", dir.path().to_str().unwrap(), dir.path().to_str().unwrap()]);
    assert_eq!(code, 2);
    assert!(stderr.contains("bruno.json"), "{stderr}");
}
