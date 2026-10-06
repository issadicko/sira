//! Gate 3 (ENF-COMP-03) : sur les collections du corpus, `xc run` donne les mêmes résultats que `bru run`.
//!
//! Chaque collection du corpus (celui de Gate 1, dans `target/corpus`) est copiée, ses adresses `http(s)://hôte` sont
//! réécrites vers un serveur local qui répond de façon déterministe, puis le runner l'exécute. Le résultat, ramené à
//! la forme de `tools/gate3/diff.py`, est comparé au rapport de `bru run` (@usebruno/cli 4.2.1) figé dans
//! `tests/fixtures/gate3/<id>.json` : Node n'est jamais lancé par ces tests. Pour régénérer les fixtures :
//! `FREEZE=1 python3 tools/gate3/diff.py`.
//!
//! Écarts voulus : `fixtures/gate3/divergences.json` et les requêtes OAuth 2 interactives (le runner refuse d'envoyer
//! une requête dont le jeton manque, Bruno l'envoie sans).

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use xc_core::{open_collection, NetworkPrefs};
use xc_runner::{json as json_report, run_collection, select, Job, Redact, Session, MAX_JUMPS};

#[allow(dead_code)]
#[path = "../../core/tests/corpus/fetch.rs"]
mod fetch;
use fetch::{fetch_all, manifest, root_of, Entry};

const FIXED_PORT: &str = "4599";
const MAX_ECHOED_BODY: usize = 65536;
const INTERACTIVE: &str = "demande une fenêtre de connexion";

fn fixtures() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/gate3")
}

fn corpus() -> Option<&'static [Entry]> {
    static CORPUS: OnceLock<Option<Vec<Entry>>> = OnceLock::new();
    CORPUS
        .get_or_init(|| {
            let loaded = manifest().and_then(|entries| fetch_all(&entries).map(|()| entries));
            match loaded {
                Ok(entries) => Some(entries),
                Err(e) if std::env::var_os("CI").is_some() => panic!("corpus indisponible :\n{e}"),
                Err(e) => {
                    eprintln!("corpus ignoré (réseau absent ?) :\n{e}");
                    None
                }
            }
        })
        .as_deref()
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Ce que lit un client : méthode, chemin, requête et corps renvoyés en JSON, comme `tools/gate3/echo-server.js`.
fn echo(method: &str, target: &str, body: &[u8]) -> String {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let mut pairs: Vec<(String, String)> = Vec::new();
    for (k, v) in form_urlencoded::parse(query.as_bytes()) {
        match pairs.iter_mut().find(|(name, _)| *name == k) {
            Some(existing) => existing.1 = v.into_owned(),
            None => pairs.push((k.into_owned(), v.into_owned())),
        }
    }
    let query: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("{}:{}", serde_json::to_string(k).unwrap(), serde_json::to_string(v).unwrap()))
        .collect();
    let raw = String::from_utf8_lossy(body);
    let body = if body.len() > MAX_ECHOED_BODY {
        serde_json::to_string(&format!("<{} octets>", body.len())).unwrap()
    } else if serde_json::from_str::<serde::de::IgnoredAny>(&raw).is_ok() {
        raw.trim().to_owned()
    } else {
        serde_json::to_string(raw.as_ref()).unwrap()
    };
    format!(
        "{{\"method\":{},\"path\":{},\"query\":{{{}}},\"body\":{},\"access_token\":\"echo-token\",\"token_type\":\"Bearer\",\"expires_in\":3600}}",
        serde_json::to_string(method).unwrap(),
        serde_json::to_string(path).unwrap(),
        query.join(","),
        body,
    )
}

async fn read_chunked(stream: &mut TcpStream, mut buffered: Vec<u8>) -> Vec<u8> {
    let mut body = Vec::new();
    loop {
        while find(&buffered, b"\r\n").is_none() {
            let mut chunk = [0u8; 8192];
            let n = stream.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                return body;
            }
            buffered.extend_from_slice(&chunk[..n]);
        }
        let end = find(&buffered, b"\r\n").unwrap();
        let size =
            usize::from_str_radix(String::from_utf8_lossy(&buffered[..end]).split(';').next().unwrap().trim(), 16)
                .unwrap_or(0);
        buffered.drain(..end + 2);
        if size == 0 {
            return body;
        }
        while buffered.len() < size + 2 {
            let mut chunk = [0u8; 8192];
            let n = stream.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                body.extend_from_slice(&buffered);
                return body;
            }
            buffered.extend_from_slice(&chunk[..n]);
        }
        body.extend_from_slice(&buffered[..size]);
        buffered.drain(..size + 2);
    }
}

async fn serve_one(mut stream: TcpStream) {
    let mut buffered = Vec::new();
    let head_end = loop {
        if let Some(i) = find(&buffered, b"\r\n\r\n") {
            break i;
        }
        let mut chunk = [0u8; 8192];
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return,
            Ok(n) => buffered.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&buffered[..head_end]).into_owned();
    let mut rest = buffered[head_end + 4..].to_vec();
    let mut lines = head.lines();
    let mut start = lines.next().unwrap_or_default().split(' ');
    let (method, target) = (start.next().unwrap_or("GET").to_owned(), start.next().unwrap_or("/").to_owned());
    let header = |name: &str| {
        head.lines().skip(1).find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
        })
    };
    if header("expect").is_some_and(|v| v.eq_ignore_ascii_case("100-continue")) {
        let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await;
    }
    let body = if header("transfer-encoding").is_some_and(|v| v.to_ascii_lowercase().contains("chunked")) {
        read_chunked(&mut stream, rest).await
    } else {
        let length: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
        while rest.len() < length {
            let mut chunk = [0u8; 65536];
            match stream.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => rest.extend_from_slice(&chunk[..n]),
            }
        }
        rest
    };
    let text = echo(&method, &target, &body);
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nx-echo: 1\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        text.len()
    );
    let _ = stream.write_all(reply.as_bytes()).await;
    if method != "HEAD" {
        let _ = stream.write_all(text.as_bytes()).await;
    }
    let _ = stream.shutdown().await;
}

/// Démarre le serveur d'écho ; rend son port.
async fn start_echo() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(serve_one(stream));
        }
    });
    port
}

/// Remplace chaque `http(s)://hôte[:port]` de `text` par `base`, comme le `HOSTS` de `tools/gate3/diff.py`.
fn rewrite_hosts(text: &str, base: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut copied, mut i) = (0, 0);
    while i < bytes.len() {
        let scheme = if bytes[i..].starts_with(b"https://") {
            8
        } else if bytes[i..].starts_with(b"http://") {
            7
        } else {
            i += 1;
            continue;
        };
        let host_chars = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-');
        let mut end = i + scheme;
        while end < bytes.len() && host_chars(&bytes[end]) {
            end += 1;
        }
        if end == i + scheme {
            i += 1;
            continue;
        }
        if bytes.get(end) == Some(&b':') {
            let digits = bytes[end + 1..].iter().take_while(|b| b.is_ascii_digit()).count();
            if digits > 0 {
                end += 1 + digits;
            }
        }
        out.push_str(&text[copied..i]);
        out.push_str(base);
        copied = end;
        i = end;
    }
    out.push_str(&text[copied..]);
    out
}

fn copy_rewritten(from: &Path, to: &Path, base: &str) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap().flatten() {
        let name = entry.file_name();
        let (source, target) = (entry.path(), to.join(&name));
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            if name != ".git" {
                copy_rewritten(&source, &target, base);
            }
        } else if kind.is_file() {
            let text = name
                .to_str()
                .filter(|n| n.ends_with(".yml") || n.ends_with(".yaml") || n.ends_with(".js"))
                .and_then(|_| fs::read_to_string(&source).ok());
            match text {
                Some(text) => fs::write(&target, rewrite_hosts(&text, base)).unwrap(),
                None => drop(fs::copy(&source, &target).unwrap()),
            }
        }
    }
}

fn pairs(list: Option<&Value>, left: &str, right: &str) -> Value {
    let rows = list.and_then(Value::as_array).map(|a| a.as_slice()).unwrap_or_default();
    Value::Array(rows.iter().map(|r| json!([r.get(left), r.get(right)])).collect())
}

/// La forme de `normalize` dans `tools/gate3/diff.py`, avec en plus le texte de l'erreur à part.
fn normalize(report: &Value) -> Vec<(String, Value, Option<String>)> {
    report["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let path = r["path"].as_str().or_else(|| r["test"]["filename"].as_str()).unwrap().to_owned();
            let error = r.get("error").and_then(Value::as_str).filter(|e| !e.is_empty()).map(str::to_owned);
            let assertions: Vec<Value> = r["assertionResults"]
                .as_array()
                .map(|a| a.as_slice())
                .unwrap_or_default()
                .iter()
                .map(|a| json!([a.get("lhsExpr"), a.get("rhsExpr"), a["status"]]))
                .collect();
            let row = json!({
                "path": path,
                "status": r["status"],
                "http": r["response"]["status"],
                "assertions": assertions,
                "tests": pairs(r.get("testResults"), "description", "status"),
                "pre": pairs(r.get("preRequestTestResults"), "description", "status"),
                "post": pairs(r.get("postResponseTestResults"), "description", "status"),
                "error": error.is_some(),
            });
            (path, row, error)
        })
        .collect()
}

async fn run(entry: &Entry, base: &str, port: u16) -> Result<Value, String> {
    let work = tempfile::tempdir().unwrap();
    let root = work.path().join("c");
    copy_rewritten(&root_of(entry), &root, base);
    let collection = open_collection(&root).map_err(|e| e.to_string())?;
    let env =
        collection.default_environment.clone().filter(|name| collection.environments.contains(name)).or_else(|| {
            let mut names = collection.environments.clone();
            names.sort();
            names.into_iter().next()
        });
    let items = select(&collection.items, &[]).map_err(|e| e.to_string())?;
    let network = NetworkPrefs {
        verify_tls: true,
        keep_default_roots: true,
        no_proxy: true,
        send_cookies: true,
        store_cookies: true,
        ..NetworkPrefs::default()
    };
    let mut session = Session { network, ..Session::default() };
    let job = Job {
        root: &root,
        collection_name: &collection.name,
        env: env.as_deref(),
        items: &items,
        rows: &[],
        bail: false,
        delay: Duration::ZERO,
        max_jumps: MAX_JUMPS,
        execution_mode: "cli",
        cancel: Arc::new(AtomicBool::new(false)),
    };
    let report = run_collection(job, &mut session, &mut |_| {}).await;
    let text = json_report(&report, &Redact::default());
    let value: Value =
        serde_json::from_str(&text.replace(&format!("127.0.0.1:{port}"), &format!("127.0.0.1:{FIXED_PORT}")))
            .map_err(|e| e.to_string())?;
    Ok(value)
}

struct Divergence {
    id: String,
    path: String,
}

fn divergences() -> Vec<Divergence> {
    let text = fs::read_to_string(fixtures().join("divergences.json")).unwrap();
    let list: Vec<Value> = serde_json::from_str(&text).unwrap();
    list.iter()
        .map(|d| Divergence { id: d["id"].as_str().unwrap().into(), path: d["path"].as_str().unwrap().into() })
        .collect()
}

fn first_difference(expected: &Value, actual: &Value) -> String {
    let keys = ["status", "http", "assertions", "tests", "pre", "post", "error"];
    keys.iter()
        .filter(|k| expected[**k] != actual[**k])
        .map(|k| format!("{k} : bru {} / xc {}", expected[*k], actual[*k]))
        .collect::<Vec<_>>()
        .join(" ; ")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn enf_comp_03_runner_gives_the_same_results_as_bru_run_on_the_corpus() {
    let Some(entries) = corpus() else { return };
    let port = start_echo().await;
    let base = format!("http://127.0.0.1:{port}");
    let known = divergences();
    let mut problems = Vec::new();
    let mut compared = 0;
    for entry in entries {
        let expected_text = fs::read_to_string(fixtures().join(format!("{}.json", entry.id)))
            .unwrap_or_else(|e| panic!("fixture de {} absente : {e}", entry.id));
        let expected: Vec<Value> = serde_json::from_str(&expected_text).unwrap();
        let report = match run(entry, &base, port).await {
            Ok(report) => report,
            Err(e) => {
                problems.push(format!("{} : {e}", entry.id));
                continue;
            }
        };
        let mut expected: BTreeMap<String, Value> =
            expected.into_iter().map(|r| (r["path"].as_str().unwrap().to_owned(), r)).collect();
        let mut actual: BTreeMap<String, Value> = BTreeMap::new();
        for (path, row, error) in normalize(&report) {
            let interactive = error.as_deref().is_some_and(|e| e.contains(INTERACTIVE));
            let known = known.iter().any(|d| d.id == entry.id && d.path == path);
            if interactive || known {
                expected.remove(&path);
            } else {
                actual.insert(path, row);
            }
        }
        for d in known.iter().filter(|d| d.id == entry.id) {
            expected.remove(&d.path);
        }
        for path in expected.keys().chain(actual.keys()).collect::<std::collections::BTreeSet<_>>() {
            match (expected.get(path), actual.get(path)) {
                (Some(e), Some(a)) if e == a => compared += 1,
                (Some(e), Some(a)) => problems.push(format!("{} / {path} : {}", entry.id, first_difference(e, a))),
                (Some(_), None) => problems.push(format!("{} / {path} : absente du run", entry.id)),
                (None, _) => problems.push(format!("{} / {path} : absente du rapport de Bruno", entry.id)),
            }
        }
    }
    assert!(problems.is_empty(), "{} écart(s) avec bru run :\n{}", problems.len(), problems.join("\n"));
    assert!(compared > 1000, "seulement {compared} requêtes comparées");
}

#[test]
fn enf_comp_03_every_corpus_collection_has_a_frozen_bru_report() {
    let Ok(entries) = manifest() else { panic!("manifest.json illisible") };
    for entry in &entries {
        let file = fixtures().join(format!("{}.json", entry.id));
        let text = fs::read_to_string(&file).unwrap_or_else(|e| panic!("{} : {e}", file.display()));
        let rows: Vec<Value> = serde_json::from_str(&text).unwrap();
        assert!(!rows.is_empty(), "{} : rapport vide", entry.id);
    }
}

#[test]
fn enf_comp_03_hosts_are_rewritten_like_the_node_harness() {
    let base = "http://127.0.0.1:4599";
    assert_eq!(
        rewrite_hosts("GET https://api.example.com:8443/v1 et http://localhost:5050/x", base),
        format!("GET {base}/v1 et {base}/x")
    );
    assert_eq!(rewrite_hosts("{{baseUrl}}/a http:// https://:80", base), "{{baseUrl}}/a http:// https://:80");
    assert_eq!(rewrite_hosts("http://host:port/x", base), format!("{base}:port/x"));
}
