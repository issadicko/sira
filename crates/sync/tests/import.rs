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
fn ef_imp_02_snapshot_lists_every_request_with_its_base() {
    for case in cases() {
        let label = format!("{} ({})", case.stem, case.group_by);
        let imported = import(&case);
        let files = files_under(&imported.root);
        let source = utf8(&files[".oc-sync/openapi/source.yml"]);
        let document = tree(source);

        let spec_name = spec_file(&case.stem).file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(document.str("source"), Some(format!("../../specs/{spec_name}").as_str()), "{label}");
        assert_eq!(document.str("groupBy"), Some(case.group_by.as_str()), "{label}");

        let operations: Vec<&yaml::Map> = document.seq("operations").iter().filter_map(yaml::Value::as_map).collect();
        let keys: Vec<&str> = operations.iter().filter_map(|o| o.str("key")).collect();
        assert_eq!(keys, keys_of(&case), "{label} : clés d'opération dans l'ordre de la spec");
        let requests: Vec<&String> = files.keys().filter(|p| is_request_file(p)).collect();
        assert_eq!(operations.len(), requests.len(), "{label} : une entrée par requête");

        let mut bases: Vec<&str> = Vec::new();
        for operation in &operations {
            let (file, base) = (operation.str("file").unwrap(), operation.str("base").unwrap());
            assert!(files.contains_key(file), "{label} : {file} absent");
            let base_file = format!(".oc-sync/openapi/base/{base}.yml");
            assert_eq!(files.get(&base_file), files.get(file), "{label} : la base de {file} est le fichier écrit");
            assert!(!bases.contains(&base), "{label} : base {base} en double");
            bases.push(base);
        }
        let base_files = files.keys().filter(|p| p.starts_with(".oc-sync/openapi/base/")).count();
        assert_eq!(base_files, operations.len(), "{label} : une base par opération");

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
        "https://example.com/openapi.json",
        GroupBy::Tags,
    )
    .unwrap();
    let files: Vec<String> =
        files_under(&root).into_keys().filter(|p| !p.starts_with(".oc-sync/openapi/base/")).collect();
    assert_eq!(
        files,
        [
            ".oc-sync/openapi/source.yml",
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
    let first = write_collection(&collection, dir.path(), "spec.yaml", GroupBy::Tags).unwrap();
    let second = write_collection(&collection, dir.path(), "spec.yaml", GroupBy::Tags).unwrap();
    let third = write_collection(&collection, dir.path(), "spec.yaml", GroupBy::Tags).unwrap();
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
    let root = write_collection(&collection("Long", items, vec![]), dir.path(), "spec.yaml", GroupBy::Tags).unwrap();
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
    let root =
        write_collection(&collection("A", items.clone(), vec![]), dir.path(), " https://x.test/s.json ", GroupBy::Path)
            .unwrap();
    let source = tree(&fs::read_to_string(root.join(".oc-sync/openapi/source.yml")).unwrap());
    assert_eq!(source.str("source"), Some("https://x.test/s.json"));
    assert_eq!(source.str("groupBy"), Some("path"));

    let spec = dir.path().join("specs/api.yaml");
    fs::create_dir_all(spec.parent().unwrap()).unwrap();
    fs::write(&spec, "openapi: 3.0.0\n").unwrap();
    let root =
        write_collection(&collection("B", items, vec![]), dir.path(), spec.to_str().unwrap(), GroupBy::Tags).unwrap();
    let source = tree(&fs::read_to_string(root.join(".oc-sync/openapi/source.yml")).unwrap());
    assert_eq!(source.str("source"), Some("../specs/api.yaml"));
}

#[test]
fn ef_imp_02_errors_tell_the_input_from_the_import() {
    let dir = location();
    let missing = dir.path().join("absent");
    let items = collection("A", vec![], vec![]);
    let error = write_collection(&items, &missing, "s", GroupBy::Tags).unwrap_err();
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
