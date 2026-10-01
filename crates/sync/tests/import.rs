//! EF-IMP-01 et EF-IMP-02 : `xc_sync::import` écrit les mêmes fichiers que l'import de Bruno.
//!
//! Les arbres de `fixtures/import/openapi/<spec>.<regroupement>/` sont ceux que produit
//! `oracle.js import <spec> <dossier> tags|path` (voir `tools/oracle`) ; ceux de `fixtures/import/curl/` sont l'item
//! que Bruno construit pour une commande cURL (`<cas>.new.json`, `<cas>.paste.json`) et le fichier que son
//! sérialiseur en tire (`<cas>.new.yml`, `<cas>.paste.yml`, produits par `stringify-item`). Les tests ne lancent
//! jamais Node.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::thread;

use serde_json::{json, Value};
use xc_core::request::BLANK_BEFORE;
use xc_core::yaml::{self, emit};
use xc_core::{normalize, open_collection, read_request, Auth, Body, KeyValue, Param, ParamKind, RequestDoc, TreeItem};
use xc_sync::import::{
    create_request_from_curl, fetch_spec, import_spec, preview, request_doc_from_curl, write_collection, ImportError,
};
use xc_sync::openapi::{load_spec, to_bruno, GroupBy};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn spec_file(stem: &str) -> PathBuf {
    ["openapi/specs", "import/specs"]
        .iter()
        .flat_map(|dir| fs::read_dir(fixtures().join(dir)).unwrap())
        .map(|e| e.unwrap().path())
        .find(|p| p.file_stem().unwrap() == stem)
        .unwrap_or_else(|| panic!("spec {stem} introuvable"))
}

fn group_by(name: &str) -> GroupBy {
    name.parse().unwrap()
}

fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let relative = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(relative, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn utf8(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap()
}

struct Case {
    stem: String,
    group_by: String,
    expected: PathBuf,
}

fn cases() -> Vec<Case> {
    let mut cases: Vec<Case> = fs::read_dir(fixtures().join("import/openapi"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .map(|expected| {
            let name = expected.file_name().unwrap().to_string_lossy().into_owned();
            let (stem, group_by) = name.rsplit_once('.').unwrap();
            Case { stem: stem.to_owned(), group_by: group_by.to_owned(), expected }
        })
        .collect();
    cases.sort_by(|a, b| (&a.stem, &a.group_by).cmp(&(&b.stem, &b.group_by)));
    cases
}

struct Imported {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

fn import(case: &Case) -> Imported {
    let dir = tempfile::tempdir().unwrap();
    let (specs, out) = (dir.path().join("specs"), dir.path().join("out"));
    fs::create_dir_all(&specs).unwrap();
    fs::create_dir_all(&out).unwrap();
    let source = spec_file(&case.stem);
    let copy = specs.join(source.file_name().unwrap());
    fs::copy(&source, &copy).unwrap();
    let text = fs::read_to_string(&copy).unwrap();
    let root = import_spec(&text, copy.to_str().unwrap(), &out, group_by(&case.group_by)).unwrap();
    Imported { _dir: dir, root }
}

fn collection_dir(expected: &Path) -> PathBuf {
    let mut entries: Vec<_> = fs::read_dir(expected).unwrap().map(|e| e.unwrap().path()).collect();
    assert_eq!(entries.len(), 1, "{} : un seul dossier de collection attendu", expected.display());
    entries.pop().unwrap()
}

fn operation_keys(items: &Value, out: &mut Vec<String>) {
    for item in items.as_array().into_iter().flatten() {
        if item["type"] == "folder" {
            operation_keys(&item["items"], out);
        } else if let Some(key) = item["operationKey"].as_str() {
            out.push(key.to_owned());
        }
    }
}

fn keys_of(case: &Case) -> Vec<String> {
    let text = fs::read_to_string(spec_file(&case.stem)).unwrap();
    let collection = to_bruno(&load_spec(&text).unwrap(), group_by(&case.group_by)).unwrap();
    let mut keys = Vec::new();
    operation_keys(&collection["items"], &mut keys);
    keys
}

fn is_request_file(path: &str) -> bool {
    path.ends_with(".yml")
        && !path.starts_with(".oc-sync/")
        && !path.starts_with("environments/")
        && path != "opencollection.yml"
        && path.rsplit('/').next() != Some("folder.yml")
}

fn tree(text: &str) -> yaml::Map {
    match yaml::parse(text).unwrap() {
        yaml::Value::Map(m) => m,
        _ => panic!("table attendue"),
    }
}

#[test]
fn ef_imp_02_import_writes_the_tree_bruno_writes() {
    let cases = cases();
    assert!(cases.len() >= 16, "au moins huit specs dans les deux regroupements");
    let mut failures = Vec::new();
    for case in &cases {
        let label = format!("{} ({})", case.stem, case.group_by);
        let imported = import(case);
        let expected_root = collection_dir(&case.expected);
        if imported.root.file_name() != expected_root.file_name() {
            failures.push(format!("{label} : dossier {:?} au lieu de {:?}", imported.root, expected_root));
        }
        assert!(imported.root.join("environments").is_dir(), "{label} : environments/ doit toujours exister");
        let expected = files_under(&expected_root);
        let mut actual = files_under(&imported.root);
        actual.retain(|path, _| !path.starts_with(".oc-sync/"));
        if expected.keys().ne(actual.keys()) {
            let missing: Vec<_> = expected.keys().filter(|k| !actual.contains_key(*k)).collect();
            let extra: Vec<_> = actual.keys().filter(|k| !expected.contains_key(*k)).collect();
            failures.push(format!("{label} : fichiers manquants {missing:?}, en trop {extra:?}"));
            continue;
        }
        for (path, bytes) in &expected {
            let ours = &actual[path];
            if path == "opencollection.yml" {
                let with_sync = utf8(bytes).replace("      - .git\n", "      - .git\n      - .oc-sync\n");
                assert_ne!(with_sync, utf8(bytes), "{label} : liste ignore inattendue");
                if utf8(ours) != with_sync {
                    failures.push(format!("{label} : {path} diffère de Bruno hors `.oc-sync`"));
                }
            } else if ours != bytes {
                failures.push(format!("{label} : {path} diffère de Bruno"));
            }
        }
    }
    assert!(failures.is_empty(), "{} écarts :\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn ef_imp_02_store_lists_every_request_and_keeps_a_raw_copy_of_the_spec() {
    for case in cases() {
        let label = format!("{} ({})", case.stem, case.group_by);
        let imported = import(&case);
        let files = files_under(&imported.root);
        let source = utf8(&files[".oc-sync/openapi/source.yml"]);
        let document = tree(source);

        let spec_name = spec_file(&case.stem).file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(document.str("source"), Some(format!("../../specs/{spec_name}").as_str()), "{label}");
        assert_eq!(document.str("groupBy"), Some(case.group_by.as_str()), "{label}");

        let original = fs::read(spec_file(&case.stem)).unwrap();
        let (name, copy) = match xc_core::pretty::pretty_json(utf8(&original)) {
            Some(pretty) => ("spec.json", pretty.into_bytes()),
            None => ("spec.yaml", original),
        };
        assert_eq!(document.str("spec"), Some(name), "{label}");
        assert_eq!(files[&format!(".oc-sync/openapi/{name}")], copy, "{label} : copie brute de la spec");
        let store_files: Vec<&String> = files.keys().filter(|p| p.starts_with(".oc-sync/")).collect();
        assert_eq!(store_files.len(), 2, "{label} : source.yml et la copie brute seulement : {store_files:?}");

        let operations: Vec<&yaml::Map> = document.seq("operations").iter().filter_map(yaml::Value::as_map).collect();
        let keys: Vec<&str> = operations.iter().filter_map(|o| o.str("key")).collect();
        assert_eq!(keys, keys_of(&case), "{label} : clés d'opération dans l'ordre de la spec");
        let requests: Vec<&String> = files.keys().filter(|p| is_request_file(p)).collect();
        assert_eq!(operations.len(), requests.len(), "{label} : une entrée par requête");
        for operation in &operations {
            let file = operation.str("file").unwrap();
            assert!(files.contains_key(file), "{label} : {file} absent");
            assert!(operation.get("base").is_none() && operation.get("removed").is_none(), "{label} : {file}");
        }

        for (path, bytes) in &files {
            assert!(!utf8(bytes).contains("operationKey"), "{label} : operationKey dans {path}");
        }
        assert_eq!(emit(&yaml::parse(source).unwrap(), &[]), source, "{label} : source.yml est réécrit sans diff");
    }
}

#[test]
fn enf_comp_02_imported_collection_opens_and_every_file_rewrites_without_diff() {
    for case in cases() {
        let label = format!("{} ({})", case.stem, case.group_by);
        let imported = import(&case);
        let info = open_collection(&imported.root).unwrap_or_else(|e| panic!("{label} : {e}"));
        assert_eq!(info.request_count, keys_of(&case).len(), "{label}");
        for (path, bytes) in files_under(&imported.root) {
            if !path.ends_with(".yml") {
                continue;
            }
            let blank: &[&str] =
                if path.starts_with("environments/") || path.ends_with("source.yml") { &[] } else { BLANK_BEFORE };
            let text = utf8(&bytes);
            assert_eq!(normalize(text, blank).unwrap(), text, "{label} : {path} est renormalisé");
        }
    }
}

#[test]
fn ef_imp_02_snapshot_is_written_after_every_collection_file() {
    let case = cases().into_iter().find(|c| c.stem == "oai-petstore-expanded-3.0").unwrap();
    let imported = import(&case);
    let modified = |path: &Path| fs::metadata(path).unwrap().modified().unwrap();
    let source = modified(&imported.root.join(".oc-sync/openapi/source.yml"));
    for path in files_under(&imported.root).keys() {
        assert!(modified(&imported.root.join(path)) <= source, "{path} écrit après source.yml");
    }
}

fn request(name: &str, key: &str) -> Value {
    json!({
        "name": name, "type": "http-request", "seq": 1, "operationKey": key,
        "request": { "method": "GET", "url": "http://x.test/a", "headers": [], "params": [],
                     "body": { "mode": "none" }, "auth": { "mode": "inherit" } },
        "settings": {}
    })
}

fn folder(name: &str, items: Vec<Value>) -> Value {
    json!({
        "name": name, "type": "folder", "items": items,
        "root": { "meta": { "name": name }, "request": { "auth": { "mode": "inherit" } } }
    })
}

fn environment(name: &str) -> Value {
    json!({ "name": name, "variables": [{ "name": "a", "value": "1", "type": "text", "enabled": true, "secret": false }] })
}

fn collection(name: &str, items: Vec<Value>, environments: Vec<Value>) -> Value {
    json!({
        "name": name, "version": "1", "items": items, "environments": environments,
        "root": { "meta": { "name": name }, "request": { "auth": { "mode": "none" } } }
    })
}

const SPEC: &str = "{\"openapi\":\"3.0.0\",\"paths\":{}}";

fn location() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

#[test]
fn ef_imp_02_colliding_and_reserved_names_get_a_suffix_instead_of_overwriting() {
    let items = vec![
        request("a/b", "GET /a/b"),
        request("a-b", "GET /a-b"),
        request("A-B", "GET /A-B"),
        request("folder", "GET /folder"),
        request("opencollection", "GET /opencollection"),
        request("environments", "GET /environments"),
        request("---", "GET /dashes"),
        folder("x/y", vec![request("folder", "GET /x/folder"), request("in x", "GET /x/in")]),
        folder("x-y", vec![request("z", "GET /z")]),
        folder("environments", vec![request("e", "GET /e")]),
    ];
    let dir = location();
    let root = write_collection(
        &collection("Mini", items, vec![environment("Dev/1"), environment("Dev-1")]),
        dir.path(),
        SPEC,
        "https://example.com/openapi.json",
        GroupBy::Tags,
    )
    .unwrap();
    let files: Vec<String> = files_under(&root).into_keys().collect();
    assert_eq!(
        files,
        [
            ".oc-sync/openapi/source.yml",
            ".oc-sync/openapi/spec.json",
            "A-B 2.yml",
            "Untitled Request.yml",
            "a-b 1.yml",
            "a-b.yml",
            "environments 1/e.yml",
            "environments 1/folder.yml",
            "environments.yml",
            "environments/Dev-1 1.yml",
            "environments/Dev-1.yml",
            "folder 1.yml",
            "opencollection 1.yml",
            "opencollection.yml",
            "x-y 1/folder.yml",
            "x-y 1/z.yml",
            "x-y/folder 1.yml",
            "x-y/folder.yml",
            "x-y/in x.yml"
        ]
    );
    let named = |path: &str| fs::read_to_string(root.join(path)).unwrap();
    assert!(named("x-y/folder.yml").contains("name: x/y"));
    assert!(named("x-y 1/folder.yml").contains("name: x-y"));
    assert!(named("a-b.yml").contains("name: a/b"));
    assert_eq!(open_collection(&root).unwrap().request_count, 11);

    let snapshot = tree(&named(".oc-sync/openapi/source.yml"));
    assert_eq!(snapshot.str("source"), Some("https://example.com/openapi.json"));
    let listed: Vec<(&str, &str)> = snapshot
        .seq("operations")
        .iter()
        .filter_map(yaml::Value::as_map)
        .map(|o| (o.str("key").unwrap(), o.str("file").unwrap()))
        .collect();
    assert_eq!(listed.len(), 11);
    assert!(listed.contains(&("GET /x/folder", "x-y/folder 1.yml")));
    assert!(listed.contains(&("GET /e", "environments 1/e.yml")));
}

#[test]
fn ef_imp_02_an_existing_collection_folder_is_never_reused() {
    let dir = location();
    let collection = collection("Mini", vec![request("one", "GET /one")], vec![]);
    let first = write_collection(&collection, dir.path(), SPEC, "spec.yaml", GroupBy::Tags).unwrap();
    let second = write_collection(&collection, dir.path(), SPEC, "spec.yaml", GroupBy::Tags).unwrap();
    let third = write_collection(&collection, dir.path(), SPEC, "spec.yaml", GroupBy::Tags).unwrap();
    let names: Vec<_> = [&first, &second, &third].iter().map(|p| p.file_name().unwrap().to_string_lossy()).collect();
    assert_eq!(names, ["Mini", "Mini - 1", "Mini - 2"]);
    let config = fs::read_to_string(second.join("opencollection.yml")).unwrap();
    assert!(config.contains("name: Mini - 1"), "{config}");
    assert!(first.join("one.yml").is_file() && second.join("one.yml").is_file());
}

#[test]
fn ef_imp_02_long_names_are_truncated_to_the_filesystem_limit() {
    let long_ascii = "x".repeat(300);
    let other_ascii = format!("{}y", "x".repeat(299));
    let japanese = "あ".repeat(90);
    let items = vec![
        request(&long_ascii, "GET /1"),
        request(&other_ascii, "GET /2"),
        request(&japanese, "GET /3"),
        folder(&"d".repeat(300), vec![request("inside", "GET /4")]),
    ];
    let dir = location();
    let root =
        write_collection(&collection("Long", items, vec![]), dir.path(), SPEC, "spec.yaml", GroupBy::Tags).unwrap();
    let names: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| !n.starts_with('.') && n != "opencollection.yml" && n != "environments")
        .collect();
    assert_eq!(names.len(), 4, "{names:?}");
    for name in &names {
        assert!(name.len() <= 255 && name.encode_utf16().count() <= 255, "{name}");
    }
    assert!(names.contains(&format!("{}.yml", "x".repeat(251))), "comme `getSafePathToWrite` : 255 - 4 caractères");
    assert!(names.contains(&format!("{} 1.yml", "x".repeat(249))), "le suffixe d'unicité survit à la troncature");
    assert!(names.contains(&"d".repeat(255)));
    let info = open_collection(&root).unwrap();
    assert_eq!(info.request_count, 4);
}

#[test]
fn ef_imp_02_source_is_the_url_or_a_path_relative_to_the_collection() {
    let dir = location();
    let items = vec![request("one", "GET /one")];
    let root = write_collection(
        &collection("A", items.clone(), vec![]),
        dir.path(),
        SPEC,
        " https://x.test/s.json ",
        GroupBy::Path,
    )
    .unwrap();
    let source = tree(&fs::read_to_string(root.join(".oc-sync/openapi/source.yml")).unwrap());
    assert_eq!(source.str("source"), Some("https://x.test/s.json"));
    assert_eq!(source.str("groupBy"), Some("path"));

    let spec = dir.path().join("specs/api.yaml");
    fs::create_dir_all(spec.parent().unwrap()).unwrap();
    fs::write(&spec, "openapi: 3.0.0\n").unwrap();
    let root =
        write_collection(&collection("B", items, vec![]), dir.path(), SPEC, spec.to_str().unwrap(), GroupBy::Tags)
            .unwrap();
    let source = tree(&fs::read_to_string(root.join(".oc-sync/openapi/source.yml")).unwrap());
    assert_eq!(source.str("source"), Some("../specs/api.yaml"));
}

#[test]
fn ef_imp_02_errors_tell_the_input_from_the_import() {
    let dir = location();
    let missing = dir.path().join("absent");
    let items = collection("A", vec![], vec![]);
    let error = write_collection(&items, &missing, SPEC, "s", GroupBy::Tags).unwrap_err();
    assert!(matches!(error, ImportError::Location(_)) && error.is_input(), "{error}");

    let error = import_spec("{{{ pas du yaml", "s", dir.path(), GroupBy::Tags).unwrap_err();
    assert!(error.is_input(), "{error}");
    let broken = fs::read_to_string(spec_file("edge-error-schema")).unwrap();
    let error = import_spec(&broken, "s", dir.path(), GroupBy::Tags).unwrap_err();
    assert!(!error.is_input(), "{error}");
    assert_eq!(dir.path().read_dir().unwrap().count(), 0, "rien n'est écrit quand la conversion échoue");

    assert_eq!("tags".parse::<GroupBy>().unwrap(), GroupBy::Tags);
    assert_eq!("path".parse::<GroupBy>().unwrap(), GroupBy::Path);
    assert!("tag".parse::<GroupBy>().unwrap_err().to_string().contains("tags ou path"));
}

#[test]
fn ef_imp_02_preview_gives_the_summary_and_the_folder_name() {
    let text = fs::read_to_string(spec_file("oai-petstore-3.0")).unwrap();
    let petstore = preview(&text).unwrap();
    assert_eq!(petstore.folder_name, "Swagger Petstore");
    assert_eq!(petstore.summary.operation_count, 3);
    let json = serde_json::to_value(&petstore).unwrap();
    assert_eq!(json["folderName"], "Swagger Petstore");
    assert_eq!(json["summary"]["operationCount"], 3);
    let odd = preview("openapi: 3.0.0\ninfo:\n  title: 'A/B: c?'\n  version: '1'\npaths: {}\n").unwrap();
    assert_eq!(odd.folder_name, "A-B- c-");
}

fn reply(status: &str, headers: &[(&str, &str)], body: &str) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n", body.len());
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(body);
    out.into_bytes()
}

fn route(path: &str, accept: &str) -> Vec<u8> {
    match path {
        "/spec.yaml" => reply("200 OK", &[], "openapi: 3.0.0\n"),
        "/accept" => reply("200 OK", &[], accept),
        "/old" => reply("301 Moved Permanently", &[("location", "spec.yaml")], ""),
        "/ftp" => reply("302 Found", &[("location", "ftp://example.test/spec.yaml")], ""),
        "/no-location" => reply("302 Found", &[], ""),
        "/announced" => b"HTTP/1.1 200 OK\r\ncontent-length: 40000000\r\nconnection: close\r\n\r\nabc".to_vec(),
        "/streamed" => {
            let head = b"HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n";
            [&head[..], &vec![b'a'; (32 << 20) + 1]].concat()
        }
        "/limit" => reply("200 OK", &[], &"a".repeat(32 << 20)),
        p if p.starts_with("/hop/") => match p["/hop/".len()..].parse::<u32>() {
            Ok(0) => reply("200 OK", &[], "arrivé"),
            Ok(n) => reply("302 Found", &[("location", &format!("/hop/{}", n - 1))], ""),
            Err(_) => reply("400 Bad Request", &[], ""),
        },
        _ => reply("404 Not Found", &[], "introuvable"),
    }
}

fn serve() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).into_owned();
            let path = request.split_whitespace().nth(1).unwrap_or("/").to_owned();
            let accept = request
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase().starts_with("accept:").then(|| l["accept:".len()..].trim().to_owned())
                })
                .unwrap_or_default();
            stream.write_all(&route(&path, &accept)).ok();
        }
    });
    base
}

#[tokio::test]
async fn ef_imp_02_fetch_spec_reads_a_file_and_reports_a_missing_one() {
    let dir = location();
    let path = dir.path().join("api.yaml");
    fs::write(&path, "openapi: 3.0.0\n").unwrap();
    assert_eq!(fetch_spec(path.to_str().unwrap()).await.unwrap(), "openapi: 3.0.0\n");
    let missing = dir.path().join("absent.yaml");
    let error = fetch_spec(missing.to_str().unwrap()).await.unwrap_err();
    assert!(matches!(error, ImportError::Source(_)) && error.is_input(), "{error}");
    assert!(error.to_string().contains("absent.yaml"), "{error}");
}

#[tokio::test]
async fn ef_imp_02_fetch_spec_gets_an_url_with_the_documented_accept_header() {
    let base = serve();
    assert_eq!(fetch_spec(&format!("{base}/spec.yaml")).await.unwrap(), "openapi: 3.0.0\n");
    assert_eq!(fetch_spec(&format!("{base}/accept")).await.unwrap(), "application/json, application/yaml, */*");
}

#[tokio::test]
async fn ef_imp_02_fetch_spec_follows_five_redirections_at_most() {
    let base = serve();
    assert_eq!(fetch_spec(&format!("{base}/old")).await.unwrap(), "openapi: 3.0.0\n");
    assert_eq!(fetch_spec(&format!("{base}/hop/5")).await.unwrap(), "arrivé");
    let error = fetch_spec(&format!("{base}/hop/6")).await.unwrap_err();
    assert!(error.to_string().contains("plus de 5 redirections"), "{error}");
}

#[tokio::test]
async fn ef_imp_02_fetch_spec_explains_failures_in_french() {
    let base = serve();
    let error = fetch_spec(&format!("{base}/missing")).await.unwrap_err().to_string();
    assert!(error.contains("le serveur a répondu 404 Not Found"), "{error}");
    let error = fetch_spec(&format!("{base}/ftp")).await.unwrap_err().to_string();
    assert!(error.contains("schéma non pris en charge"), "{error}");
    let error = fetch_spec(&format!("{base}/no-location")).await.unwrap_err().to_string();
    assert!(error.contains("le serveur a répondu 302 Found"), "{error}");
    let closed = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/spec.yaml", closed.local_addr().unwrap());
    drop(closed);
    let error = fetch_spec(&url).await.unwrap_err();
    assert!(error.is_input() && error.to_string().contains("connexion impossible"), "{error}");
}

fn curl_ids() -> Vec<String> {
    let mut ids: Vec<String> = fs::read_dir(fixtures().join("import/curl"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "sh"))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    ids.sort();
    ids
}

fn curl_fixture(id: &str, ext: &str) -> String {
    fs::read_to_string(fixtures().join("import/curl").join(format!("{id}.{ext}"))).unwrap()
}

fn blank_collection() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: Curl\n").unwrap();
    dir
}

fn dummy_requests(dir: &Path, count: u64) {
    for n in 1..=count {
        let text = format!(
            "info:\n  name: Dummy {n}\n  type: http\n  seq: {n}\n\nhttp:\n  method: GET\n  url: http://x.test\n"
        );
        fs::write(dir.join(format!("dummy-{n}.yml")), text).unwrap();
    }
}

#[test]
fn ef_imp_01_created_request_files_match_bruno() {
    let ids = curl_ids();
    assert!(ids.len() >= 16, "{ids:?}");
    for id in ids {
        let item: Value = serde_json::from_str(&curl_fixture(&id, "new.json")).unwrap();
        let name = item["name"].as_str().unwrap();
        let dir = blank_collection();
        dummy_requests(dir.path(), item["seq"].as_u64().unwrap() - 1);
        let created = create_request_from_curl(dir.path(), "", name, curl_fixture(&id, "sh").trim()).unwrap();
        assert_eq!(created, format!("{name}.yml"), "{id}");
        let written = fs::read_to_string(dir.path().join(&created)).unwrap();
        assert_eq!(written, curl_fixture(&id, "new.yml"), "{id}");
        assert_eq!(normalize(&written, BLANK_BEFORE).unwrap(), written, "{id} : réécriture sans diff");
        let info = open_collection(dir.path()).unwrap();
        assert!(info.items.iter().any(|i| matches!(i, TreeItem::Request { name: n, .. } if n == name)), "{id}");
    }
}

#[test]
fn ef_imp_01_new_request_goes_in_the_given_folder_with_the_next_seq() {
    let dir = blank_collection();
    let users = dir.path().join("users");
    fs::create_dir_all(users.join("archive")).unwrap();
    dummy_requests(&users, 2);
    let created = create_request_from_curl(dir.path(), "users/", "List", "curl https://x.test/users").unwrap();
    assert_eq!(created, "users/List.yml");
    let doc = read_request(dir.path(), &created).unwrap();
    assert_eq!((doc.seq, doc.method.as_str(), doc.url.as_str()), (Some(4), "GET", "https://x.test/users"));
    assert_eq!(fs::read_dir(&users).unwrap().flatten().filter(|e| e.file_name() == "List.yml").count(), 1);
    assert_eq!(fs::read_dir(&users).unwrap().count(), 4, "aucun fichier temporaire ne reste");
}

#[test]
fn ef_imp_01_creating_a_request_never_overwrites_and_refuses_bad_input() {
    let dir = blank_collection();
    let curl = "curl https://x.test/a";
    create_request_from_curl(dir.path(), "", "Same", curl).unwrap();
    let before = fs::read_to_string(dir.path().join("Same.yml")).unwrap();
    let error = create_request_from_curl(dir.path(), "", "Same", "curl https://x.test/other").unwrap_err();
    assert!(matches!(&error, ImportError::AlreadyExists(p) if p == "Same.yml"), "{error}");
    assert_eq!(fs::read_to_string(dir.path().join("Same.yml")).unwrap(), before);

    for command in ["", "curl", "not a command", "curl -X POST"] {
        let error = create_request_from_curl(dir.path(), "", "Bad", command).unwrap_err();
        assert!(matches!(error, ImportError::InvalidCurl), "{command:?} : {error}");
    }
    let too_long = "n".repeat(256);
    for name in ["", "   ", ".", "con", "COM1", "folder", "collection", too_long.as_str()] {
        let error = create_request_from_curl(dir.path(), "", name, curl).unwrap_err();
        assert!(matches!(error, ImportError::InvalidName(_)), "{name:?} : {error}");
    }
    let error = create_request_from_curl(dir.path(), "../outside", "Out", curl).unwrap_err();
    assert!(matches!(error, ImportError::Core(_)), "{error}");
    let error = create_request_from_curl(dir.path(), "absent", "Out", curl).unwrap_err();
    assert!(matches!(error, ImportError::FolderNotFound(_)), "{error}");
    let created = create_request_from_curl(dir.path(), "", "Get: users/list?", curl).unwrap();
    assert_eq!(created, "Get- users-list-.yml");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
}

#[test]
fn ef_imp_01_pasted_curl_builds_the_request_bruno_builds() {
    for id in curl_ids() {
        let mut expected = RequestDoc::from_tree(&tree(&curl_fixture(&id, "paste.yml")));
        if matches!(expected.body, Body::FormUrlEncoded { .. } | Body::MultipartForm { .. }) {
            expected.body = RequestDoc::from_tree(&tree(&curl_fixture(&id, "new.yml"))).body;
        }
        let pasted =
            request_doc_from_curl(curl_fixture(&id, "sh").trim()).unwrap_or_else(|| panic!("{id} : cURL refusé"));
        assert_eq!(pasted, expected, "{id}");
    }
}

fn param(name: &str, value: &str, kind: ParamKind) -> Param {
    Param { name: name.into(), value: value.into(), kind, enabled: true, description: None }
}

#[test]
fn ef_imp_01_pasted_curl_fills_method_url_params_headers_body_and_auth() {
    let doc = request_doc_from_curl(
        "curl -X post 'https://x.test/u/:id?a=1&b' -H 'Content-Type: application/json' -H 'X-A: b' -u me:pw -d '{\"a\":1}'",
    )
    .unwrap();
    assert_eq!(doc.method, "POST");
    assert_eq!(doc.url, "https://x.test/u/:id?a=1&b");
    assert_eq!(
        doc.params,
        [param("a", "1", ParamKind::Query), param("b", "", ParamKind::Query), param("id", "", ParamKind::Path)]
    );
    let header = |n: &str, v: &str| KeyValue { name: n.into(), value: v.into(), enabled: true, description: None };
    assert_eq!(doc.headers, [header("Content-Type", "application/json"), header("X-A", "b")]);
    assert_eq!(doc.body, Body::Json { data: "{\n  \"a\": 1\n}".into() });
    assert_eq!(doc.auth, Auth::Basic { username: "me".into(), password: "pw".into() });
}

#[test]
fn ef_imp_01_pasted_curl_copies_form_fields_and_keeps_defaults() {
    let form = request_doc_from_curl("curl https://x.test/login -d 'user=ada&pass=secret'").unwrap();
    let field = |n: &str, v: &str| KeyValue { name: n.into(), value: v.into(), enabled: true, description: None };
    assert_eq!(form.body, Body::FormUrlEncoded { fields: vec![field("user", "ada"), field("pass", "secret")] });
    assert_eq!((form.method.as_str(), form.auth), ("POST", Auth::Inherit));
    let get = request_doc_from_curl("curl https://x.test/users").unwrap();
    assert_eq!((get.method.as_str(), get.body), ("GET", Body::None));
}

#[test]
fn ef_imp_01_only_text_that_looks_like_a_curl_command_is_pasted() {
    for text in ["https://x.test/users", "curl", "curlx https://x.test", "echo curl https://x.test", "", "curl "] {
        assert!(request_doc_from_curl(text).is_none(), "{text:?}");
    }
    for text in ["  CURL https://x.test", "\n\tcurl\nhttps://x.test", "cUrL -X DELETE https://x.test/1"] {
        assert!(request_doc_from_curl(text).is_some(), "{text:?}");
    }
    assert_eq!(request_doc_from_curl("curl -X DELETE https://x.test/1").unwrap().method, "DELETE");
}

fn is_device_radical(name: &str) -> bool {
    let radical = name.split('.').next().unwrap().to_ascii_lowercase();
    let numbered =
        |prefix: &str| radical.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && n.as_bytes()[0].is_ascii_digit());
    ["con", "prn", "aux", "nul"].contains(&radical.as_str()) || numbered("com") || numbered("lpt")
}

#[test]
fn ef_imp_02_a_long_title_over_an_existing_folder_gets_a_suffix_instead_of_looping() {
    let (sender, receiver) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let dir = location();
        let title = "x".repeat(300);
        let collection = collection(&title, vec![request("one", "GET /one")], vec![]);
        let roots: Vec<PathBuf> = (0..3)
            .map(|_| write_collection(&collection, dir.path(), SPEC, "spec.yaml", GroupBy::Tags).unwrap())
            .collect();
        sender.send((dir, roots)).ok();
    });
    let (_dir, roots) = receiver.recv_timeout(std::time::Duration::from_secs(10)).expect("l'import boucle");
    let names: Vec<String> = roots.iter().map(|r| r.file_name().unwrap().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["x".repeat(255), format!("{} - 1", "x".repeat(251)), format!("{} - 2", "x".repeat(251))]);
    let config = fs::read_to_string(roots[1].join("opencollection.yml")).unwrap();
    assert!(config.contains(&format!("name: {} - 1", "x".repeat(300))), "le nom de la collection garde son titre");
}

#[test]
fn ef_imp_02_a_failed_import_leaves_neither_a_collection_nor_a_staging_folder() {
    let nested = |depth: usize| {
        (0..depth).fold(vec![request("deepest", "GET /deep")], |inner, level| {
            let name = format!("{}{level}", "a".repeat(250));
            vec![request("sibling", &format!("GET /s{level}")), folder(&name, inner)]
        })
    };
    let dir = location();
    let items = [vec![request("before", "GET /before")], nested(150)].concat();
    let error =
        write_collection(&collection("Deep", items, vec![]), dir.path(), SPEC, "spec.yaml", GroupBy::Tags).unwrap_err();
    assert!(matches!(error, ImportError::Core(_)), "{error}");
    assert_eq!(dir.path().read_dir().unwrap().count(), 0, "ni dossier final ni dossier de préparation");

    let collection = collection("Fine", vec![request("one", "GET /one")], vec![]);
    let root = write_collection(&collection, dir.path(), SPEC, "spec.yaml", GroupBy::Tags).unwrap();
    assert_eq!(dir.path().read_dir().unwrap().count(), 1, "le dossier de préparation est renommé, pas copié");
    assert!(root.join(".oc-sync/openapi/source.yml").is_file());
}

#[test]
fn ef_imp_02_items_the_collection_tree_would_hide_are_renamed_so_they_stay_listed() {
    let items = vec![
        folder(".well-known", vec![request("jwks", "GET /.well-known/jwks")]),
        folder("node_modules", vec![request("package", "GET /node_modules")]),
        folder("mocks", vec![request("mock", "GET /mocks")]),
        folder("Mocks", vec![request("kept", "GET /Mocks")]),
        request(".hidden", "GET /hidden"),
        request("opencollection", "GET /opencollection"),
        folder("sub", vec![request("opencollection", "GET /sub/opencollection"), request("folder", "GET /sub/folder")]),
        folder("sub-mocks", vec![folder("mocks", vec![request("inner", "GET /inner")])]),
    ];
    let dir = location();
    let root =
        write_collection(&collection("Hidden", items, vec![]), dir.path(), SPEC, "spec.yaml", GroupBy::Tags).unwrap();
    let info = open_collection(&root).unwrap();
    assert_eq!(info.request_count, 9, "chaque requête écrite est montrée par l'arbre");

    let files: Vec<String> = files_under(&root).into_keys().collect();
    assert_eq!(
        files,
        [
            ".oc-sync/openapi/source.yml",
            ".oc-sync/openapi/spec.json",
            "Mocks/folder.yml",
            "Mocks/kept.yml",
            "hidden.yml",
            "mocks 1/folder.yml",
            "mocks 1/mock.yml",
            "node_modules 1/folder.yml",
            "node_modules 1/package.yml",
            "opencollection 1.yml",
            "opencollection.yml",
            "sub-mocks/folder.yml",
            "sub-mocks/mocks/folder.yml",
            "sub-mocks/mocks/inner.yml",
            "sub/folder 1.yml",
            "sub/folder.yml",
            "sub/opencollection 1.yml",
            "well-known/folder.yml",
            "well-known/jwks.yml"
        ]
    );
    let snapshot = tree(&fs::read_to_string(root.join(".oc-sync/openapi/source.yml")).unwrap());
    for operation in snapshot.seq("operations").iter().filter_map(yaml::Value::as_map) {
        assert!(root.join(operation.str("file").unwrap()).is_file());
    }
}

#[test]
fn ef_imp_02_windows_device_names_never_name_a_file_or_a_folder() {
    let names = ["CON", "nul", "Aux", "COM1", "lpt9", "con.txt", "Com3.v2", "console", "COM10"];
    let mut items: Vec<Value> = names.iter().map(|n| request(n, &format!("GET /{n}"))).collect();
    items.push(folder("PRN", vec![request("in", "GET /in")]));
    let dir = location();
    let root = write_collection(
        &collection("Devices", items, vec![environment("NUL")]),
        dir.path(),
        SPEC,
        "s.yaml",
        GroupBy::Tags,
    )
    .unwrap();
    let files = files_under(&root);
    for path in files.keys() {
        for part in path.split('/') {
            assert!(!is_device_radical(part), "{part} dans {path}");
        }
    }
    let listed: Vec<&String> = files.keys().filter(|p| !p.starts_with(".oc-sync/")).collect();
    let renamed =
        ["CON 1", "nul 1", "Aux 1", "COM1 1", "lpt9 1", "con 1.txt", "Com3 1.v2", "PRN 1/in", "environments/NUL 1"];
    for name in renamed.iter().map(|n| format!("{n}.yml")).chain(["console.yml".into(), "COM10.yml".into()]) {
        assert!(listed.iter().any(|p| **p == name), "{name} absent de {listed:?}");
    }

    for (title, folder) in [("CON", "CON - 1"), ("con.example", "con - 1.example"), ("com1", "com1 - 1")] {
        let dir = location();
        let created =
            write_collection(&collection(title, vec![], vec![]), dir.path(), SPEC, "s.yaml", GroupBy::Tags).unwrap();
        assert_eq!(created.file_name().unwrap().to_string_lossy(), folder);
    }
}

#[test]
fn enf_sec_01_snapshot_source_keeps_neither_url_credentials_nor_secret_parameters() {
    let dir = location();
    let items = vec![request("one", "GET /one")];
    let source = |url: &str| {
        let root =
            write_collection(&collection("S", items.clone(), vec![]), dir.path(), SPEC, url, GroupBy::Tags).unwrap();
        let text = fs::read_to_string(root.join(".oc-sync/openapi/source.yml")).unwrap();
        for (path, bytes) in files_under(&root) {
            let content = utf8(&bytes);
            assert!(!content.contains("hunter2") && !content.contains("abc123"), "secret dans {path}");
        }
        tree(&text).str("source").unwrap().to_owned()
    };
    assert_eq!(source("https://ada:hunter2@api.test/s.json?token=abc123&v=2"), "https://api.test/s.json?v=2");
    assert_eq!(
        source("https://api.test/s.json?API_KEY=abc123&lang=fr&X-Amz-Signature=abc123&access_token=abc123&code=abc123"),
        "https://api.test/s.json?lang=fr"
    );
    assert_eq!(
        source("https://ada@api.test:8443/s.json?a=1&client_secret=abc123&b=%20x"),
        "https://api.test:8443/s.json?a=1&b=%20x"
    );
    assert_eq!(source("https://ada:hunter2@api.test/s.json"), "https://api.test/s.json");
    assert_eq!(
        source("https://api.test/s.json?sort_key=1&keyword=k&password=abc123"),
        "https://api.test/s.json?keyword=k"
    );
    assert_eq!(source(" https://api.test/s.json?version=3&lang=fr "), "https://api.test/s.json?version=3&lang=fr");
    assert_eq!(source("https://api.test"), "https://api.test");
}

#[test]
fn ef_imp_02_preview_reports_the_errors_of_the_import_and_names_the_folder_it_creates() {
    for version in ["openapi: 3.0.0", "swagger: '2.0'"] {
        let numeric = format!("{version}\ninfo: {{title: 123, version: '1'}}\npaths: {{}}\n");
        let error = preview(&numeric).unwrap_err();
        let imported = import_spec(&numeric, "s", location().path(), GroupBy::Tags).unwrap_err();
        assert_eq!(error.to_string(), imported.to_string());

        for title in ["'..'", "'-'", "'  x  '", "'a/b: c'", "CON", "'~/x.'", "Untitled", "''"] {
            let spec = format!("{version}\ninfo: {{title: {title}, version: '1'}}\npaths: {{}}\n");
            let dir = location();
            let created = import_spec(&spec, "s", dir.path(), GroupBy::Tags).unwrap();
            let name = created.file_name().unwrap().to_string_lossy().into_owned();
            assert_eq!(preview(&spec).unwrap().folder_name, name, "{version} {title}");
            assert!(!name.is_empty());
        }
    }
    let dots = "openapi: 3.0.0\ninfo: {title: '..', version: '1'}\npaths: {}\n";
    assert_eq!(preview(dots).unwrap().folder_name, "Untitled Collection");
}

#[tokio::test]
async fn ef_imp_02_fetch_spec_refuses_a_spec_over_32_mb() {
    let base = serve();
    let announced = fetch_spec(&format!("{base}/announced")).await.unwrap_err();
    assert!(announced.is_input() && announced.to_string().contains("réponse trop volumineuse"), "{announced}");
    let streamed = fetch_spec(&format!("{base}/streamed")).await.unwrap_err();
    assert!(streamed.to_string().contains("réponse trop volumineuse"), "{streamed}");
    let just_fits = fetch_spec(&format!("{base}/limit")).await.unwrap();
    assert_eq!(just_fits.len(), 32 << 20);

    let dir = location();
    let path = dir.path().join("huge.yaml");
    fs::File::create(&path).unwrap().set_len((32 << 20) + 1).unwrap();
    let error = fetch_spec(path.to_str().unwrap()).await.unwrap_err();
    assert!(error.is_input() && error.to_string().contains("32 Mo"), "{error}");
}

#[test]
fn ef_imp_01_a_new_request_cannot_take_a_name_the_tree_would_not_show() {
    let dir = blank_collection();
    fs::create_dir(dir.path().join("sub")).unwrap();
    let curl = "curl https://x.test/a";
    for name in ["folder.yml", "opencollection", "opencollection.yml", ".hidden", "con.txt", "Aux.yml", "..x"] {
        for folder in ["", "sub"] {
            let error = create_request_from_curl(dir.path(), folder, name, curl).unwrap_err();
            assert!(matches!(error, ImportError::InvalidName(_)), "{folder:?} {name:?} : {error}");
        }
    }
    for folder in ["", "sub"] {
        create_request_from_curl(dir.path(), folder, "Fine", curl).unwrap();
    }
    assert_eq!(open_collection(dir.path()).unwrap().request_count, 2, "les requêtes créées sont montrées");
}

#[test]
fn ef_imp_01_seq_counts_the_entries_the_tree_lists_without_reading_them() {
    let dir = blank_collection();
    let users = dir.path().join("users");
    fs::create_dir_all(users.join("archive")).unwrap();
    fs::create_dir_all(users.join(".git")).unwrap();
    fs::create_dir_all(users.join("node_modules")).unwrap();
    dummy_requests(&users, 2);
    fs::write(users.join("broken.yml"), "{{ pas du yaml").unwrap();
    fs::write(users.join(".secret.yml"), "info:\n  name: caché\n").unwrap();
    fs::write(users.join("notes.txt"), "pas une requête").unwrap();
    fs::write(users.join("folder.yml"), "info:\n  name: Users\n  type: folder\n").unwrap();
    let created = create_request_from_curl(dir.path(), "users", "List", "curl https://x.test/users").unwrap();
    let doc = read_request(dir.path(), &created).unwrap();
    assert_eq!(doc.seq, Some(5), "archive, dummy-1, dummy-2 et broken.yml, plus un");
}

#[test]
fn ef_imp_01_creation_is_exclusive_even_when_the_name_looks_free() {
    let dir = blank_collection();
    #[cfg(unix)]
    {
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path().join("stolen.yml"), dir.path().join("Same.yml")).unwrap();
        let error = create_request_from_curl(dir.path(), "", "Same", "curl https://x.test/a").unwrap_err();
        assert!(matches!(&error, ImportError::AlreadyExists(p) if p == "Same.yml"), "{error}");
        assert!(!outside.path().join("stolen.yml").exists(), "rien n'est écrit à travers le lien");

        std::os::unix::fs::symlink(outside.path(), dir.path().join("leak")).unwrap();
        let error = create_request_from_curl(dir.path(), "leak", "Out", "curl https://x.test/a").unwrap_err();
        assert!(matches!(error, ImportError::Core(xc_core::CoreError::OutsideCollection(_))), "{error}");
        assert_eq!(outside.path().read_dir().unwrap().count(), 0);
    }
    create_request_from_curl(dir.path(), "", "Once", "curl https://x.test/a").unwrap();
}

#[test]
fn ef_imp_01_a_curl_command_over_2_mb_is_refused() {
    let command = format!("curl https://x.test -d '{}'", "a".repeat(2 << 20));
    assert!(request_doc_from_curl(&command).is_none());
    let dir = blank_collection();
    let error = create_request_from_curl(dir.path(), "", "Big", &command).unwrap_err();
    assert!(matches!(error, ImportError::CurlTooLarge), "{error}");
    assert!(error.to_string().contains("2 Mo"), "{error}");
    let fits = format!("curl https://x.test -d '{}'", "a".repeat(1 << 20));
    assert!(request_doc_from_curl(&fits).is_some());
}

#[test]
fn ef_imp_01_pasting_hostile_curl_commands_does_not_freeze() {
    let commands = [
        ("truncated utf-8", format!("curl http://x.test -d 'a={}'", "%E3%81".repeat(3_000))),
        ("shift-jis", format!("curl http://x.test -d 'text={}'", "%83%65%83%58%83%67".repeat(900))),
        ("repeated key", format!("curl http://x.test -d '{}'", "a=b&".repeat(20_000))),
        (
            "distinct keys",
            format!("curl http://x.test -d '{}'", (0..20_000).map(|i| format!("k{i}=v")).collect::<Vec<_>>().join("&")),
        ),
        ("repeated url key", format!("curl 'http://x.test/?{}'", "a=b&".repeat(20_000))),
    ];
    for (label, command) in commands {
        let start = std::time::Instant::now();
        assert!(request_doc_from_curl(&command).is_some(), "{label}");
        assert!(start.elapsed().as_secs_f64() < 1.0, "{label} : {:?}", start.elapsed());
    }
}

#[test]
fn ef_imp_01_deeply_nested_json_bodies_neither_crash_nor_blow_up() {
    let deep = |levels: usize| format!("{}{}", "[".repeat(levels), "]".repeat(levels));
    let start = std::time::Instant::now();
    let doc = request_doc_from_curl(&format!(
        "curl https://x.test -H 'Content-Type: application/json' -d '{}'",
        deep(100_000)
    ))
    .unwrap();
    assert_eq!(doc.body, Body::Json { data: deep(100_000) }, "au-delà de 512 niveaux, le corps n'est pas mis en forme");
    let shallow = request_doc_from_curl("curl https://x.test -H 'Content-Type: application/json' -d '[[1]]'").unwrap();
    assert_eq!(shallow.body, Body::Json { data: "[\n  [\n    1\n  ]\n]".into() });

    let dir = blank_collection();
    let graphql = format!("curl https://x.test/graphql -H 'Content-Type: application/json' -d '{}'", deep(100_000));
    let created = create_request_from_curl(dir.path(), "", "Deep", &graphql).unwrap();
    assert_eq!(created, "Deep.yml");
    assert!(start.elapsed().as_secs_f64() < 2.0, "{:?}", start.elapsed());
}
