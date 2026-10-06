//! Rapports d'un run : les résultats au format de `bru run` (JSON), JUnit et une page HTML autonome.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Map, Value};

use crate::pipeline::{AssertionResult, PhaseReport};
use crate::run::{Halt, RequestResult, RunReport, Skip};

#[derive(Debug, Clone, Serialize)]
pub struct TestFile {
    pub filename: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RequestEntry {
    pub method: Option<String>,
    pub url: Option<String>,
    pub headers: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseEntry {
    /// Le code HTTP, ou `skipped` / `error` quand il n'y a pas eu de réponse.
    pub status: Value,
    pub status_text: Option<String>,
    pub headers: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    pub url: Option<String>,
    /// En millisecondes.
    pub response_time: f64,
    pub duration: f64,
    pub size: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestEntry {
    pub status: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_script_error: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssertionEntry {
    pub lhs_expr: String,
    /// L'opérateur et l'opérande tels qu'écrits : `eq 200`.
    pub rhs_expr: String,
    pub rhs_operand: String,
    pub operator: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Le résultat d'une requête au format de `bru run --format json`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub iteration_index: usize,
    pub test: TestFile,
    pub request: RequestEntry,
    pub response: ResponseEntry,
    /// `pass`, `error` ou `skipped` : un test ou une assertion en échec ne change pas ce statut.
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<String>,
    pub error: Option<String>,
    pub assertion_results: Vec<AssertionEntry>,
    pub test_results: Vec<TestEntry>,
    pub pre_request_test_results: Vec<TestEntry>,
    pub post_response_test_results: Vec<TestEntry>,
    /// En secondes.
    pub run_duration: f64,
    pub suitename: String,
    pub name: String,
    pub path: String,
}

fn without_extension(path: &str) -> &str {
    path.strip_suffix(".yml").unwrap_or(path)
}

fn header_map(pairs: &[(String, String)]) -> Map<String, Value> {
    let mut map = Map::new();
    for (name, value) in pairs {
        match map.get_mut(name) {
            Some(Value::String(existing)) => {
                existing.push_str(", ");
                existing.push_str(value);
            }
            _ => {
                map.insert(name.clone(), Value::String(value.clone()));
            }
        }
    }
    map
}

fn body_value(bytes: &[u8]) -> Value {
    match std::str::from_utf8(bytes) {
        Ok(text) => serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.to_owned())),
        Err(_) => Value::String(format!("<{} octets binaires>", bytes.len())),
    }
}

fn is_json(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v.to_ascii_lowercase().contains("json"))
}

fn tests(phase: &PhaseReport, script_error: Option<&str>) -> Vec<TestEntry> {
    let mut out: Vec<TestEntry> = phase
        .results
        .iter()
        .map(|r| TestEntry {
            status: r.status.clone(),
            description: r.description.clone(),
            error: r.error.clone(),
            is_script_error: None,
        })
        .collect();
    if let (Some(label), Some(error)) = (script_error, &phase.error) {
        out.push(TestEntry {
            status: "fail".into(),
            description: label.into(),
            error: Some(error.clone()),
            is_script_error: Some(true),
        });
    }
    out
}

fn assertion(a: &AssertionResult) -> AssertionEntry {
    let operand = a.expected.clone().unwrap_or_default();
    AssertionEntry {
        lhs_expr: a.expression.clone(),
        rhs_expr: if operand.is_empty() { a.operator.clone() } else { format!("{} {operand}", a.operator) },
        rhs_operand: operand,
        operator: a.operator.clone(),
        status: if a.passed { "pass" } else { "fail" }.into(),
        error: a.error.clone(),
    }
}

impl Entry {
    pub(crate) fn of(r: &RequestResult) -> Self {
        let o = &r.outcome;
        let skip_reason = match &r.skip {
            Some(Skip::Bail) => Some("bail".to_owned()),
            Some(Skip::StopExecution) => Some("stopExecution".to_owned()),
            _ => None,
        };
        let (status, error, status_text) = match (&r.skip, &o.error) {
            (Some(Skip::Unreadable(why)), _) => ("skipped", Some(why.clone()), Some(why.clone())),
            (Some(Skip::Script), _) => ("skipped", None, Some("request skipped via pre-request script".to_owned())),
            (Some(_), _) => ("skipped", None, None),
            (None, Some(e)) => ("error", Some(e.message.clone()), None),
            (None, None) => ("pass", None, o.response.as_ref().map(|res| res.reason.clone())),
        };
        let sent = !matches!(&r.skip, Some(Skip::Bail | Skip::StopExecution | Skip::Unreadable(_)));
        let request = RequestEntry {
            method: sent.then(|| o.method.clone()),
            url: sent.then(|| o.url.clone()),
            headers: sent.then(|| header_map(&o.sent_headers)),
            data: o.sent_body.as_ref().map(|body| {
                if is_json(&o.sent_headers) {
                    serde_json::from_str(body).unwrap_or_else(|_| Value::String(body.clone()))
                } else {
                    Value::String(body.clone())
                }
            }),
        };
        let response = match &o.response {
            Some(res) if status == "pass" => ResponseEntry {
                status: Value::from(res.status),
                status_text,
                headers: Some(header_map(&res.headers)),
                data: Some(body_value(&res.body)),
                url: Some(o.url.clone()),
                response_time: res.timings.total_ms,
                duration: res.timings.total_ms,
                size: res.body.len(),
            },
            _ => ResponseEntry {
                status: Value::String(status.into()),
                status_text,
                headers: None,
                data: None,
                url: None,
                response_time: 0.0,
                duration: 0.0,
                size: 0,
            },
        };
        Self {
            iteration_index: r.iteration,
            test: TestFile { filename: r.path.clone() },
            request,
            response,
            status: status.into(),
            skipped: (status == "skipped").then_some(true),
            skip_reason,
            error,
            assertion_results: o.assertions.iter().map(assertion).collect(),
            test_results: tests(&o.tests, Some("Test Script Error")),
            pre_request_test_results: tests(&o.pre, None),
            post_response_test_results: tests(&o.post, Some("Post-Response Script Error")),
            run_duration: r.duration.as_secs_f64(),
            suitename: without_extension(&r.path).to_owned(),
            name: r.name.clone(),
            path: r.path.clone(),
        }
    }

    fn all_tests(&self) -> impl Iterator<Item = &TestEntry> {
        self.pre_request_test_results.iter().chain(&self.test_results).chain(&self.post_response_test_results)
    }

    /// Un test, une assertion ou un script a échoué, ou la requête n'a pas abouti.
    pub fn failed(&self) -> bool {
        self.status == "error"
            || self.assertion_results.iter().any(|a| a.status != "pass")
            || self.all_tests().any(|t| t.is_script_error == Some(true) || t.status != "pass")
    }
}

/// Les compteurs d'un run, comme `getRunnerSummary` de Bruno.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub total_requests: usize,
    pub passed_requests: usize,
    pub failed_requests: usize,
    pub error_requests: usize,
    pub skipped_requests: usize,
    pub skipped_by_bail: usize,
    pub total_assertions: usize,
    pub passed_assertions: usize,
    pub failed_assertions: usize,
    pub total_tests: usize,
    pub passed_tests: usize,
    pub failed_tests: usize,
    pub total_pre_request_tests: usize,
    pub passed_pre_request_tests: usize,
    pub failed_pre_request_tests: usize,
    pub total_post_response_tests: usize,
    pub passed_post_response_tests: usize,
    pub failed_post_response_tests: usize,
}

/// (total, réussis, échoués) des tests qui ne sont pas des erreurs de script ; `broken` : une erreur de script.
fn tally(list: &[TestEntry]) -> (usize, usize, usize, bool) {
    let real = || list.iter().filter(|t| t.is_script_error != Some(true));
    let passed = real().filter(|t| t.status == "pass").count();
    (real().count(), passed, real().count() - passed, list.iter().any(|t| t.is_script_error == Some(true)))
}

impl Summary {
    pub fn of(entries: &[Entry]) -> Self {
        let mut s = Self::default();
        for e in entries {
            s.total_requests += 1;
            let (tests, pre, post) =
                (tally(&e.test_results), tally(&e.pre_request_test_results), tally(&e.post_response_test_results));
            s.total_tests += tests.0;
            s.total_assertions += e.assertion_results.len();
            s.total_pre_request_tests += pre.0;
            s.total_post_response_tests += post.0;
            if e.status == "skipped" {
                s.skipped_requests += 1;
                s.skipped_by_bail += usize::from(e.skip_reason.as_deref() == Some("bail"));
                continue;
            }
            let failed_assertions = e.assertion_results.iter().filter(|a| a.status != "pass").count();
            s.passed_tests += tests.1;
            s.failed_tests += tests.2;
            s.passed_pre_request_tests += pre.1;
            s.failed_pre_request_tests += pre.2;
            s.passed_post_response_tests += post.1;
            s.failed_post_response_tests += post.2;
            s.passed_assertions += e.assertion_results.len() - failed_assertions;
            s.failed_assertions += failed_assertions;
            let any_failed = failed_assertions > 0 || tests.2 + pre.2 + post.2 > 0 || tests.3 || pre.3 || post.3;
            if any_failed {
                s.failed_requests += 1;
            } else if e.status == "error" {
                s.error_requests += 1;
            } else {
                s.passed_requests += 1;
            }
        }
        s
    }

    /// Le code de sortie du CLI : une requête a échoué ou sorti en erreur.
    pub fn failed(&self) -> bool {
        self.failed_requests + self.error_requests + self.failed_assertions > 0
            || self.failed_tests + self.failed_pre_request_tests + self.failed_post_response_tests > 0
    }
}

/// Ce qu'un rapport laisse de côté, pour ne pas y écrire de secrets ni de gros corps.
#[derive(Debug, Clone, Default)]
pub struct Redact {
    pub all_headers: bool,
    /// Noms d'en-têtes à retirer, sans tenir compte de la casse.
    pub headers: Vec<String>,
    pub request_body: bool,
    pub response_body: bool,
}

impl Redact {
    pub fn apply(&self, entries: &mut [Entry]) {
        for entry in entries {
            for headers in [&mut entry.request.headers, &mut entry.response.headers] {
                let Some(map) = headers else { continue };
                if self.all_headers {
                    map.clear();
                }
                map.retain(|name, _| !self.headers.iter().any(|h| h.eq_ignore_ascii_case(name)));
            }
            if self.request_body {
                entry.request.data = None;
            }
            if self.response_body {
                entry.response.data = None;
            }
        }
    }
}

fn entries(report: &RunReport, redact: &Redact) -> Vec<Entry> {
    let mut entries = report.entries();
    redact.apply(&mut entries);
    entries
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IterationEntry<'a> {
    iteration_index: usize,
    iteration_data: &'a Option<crate::data::Row>,
    summary: Summary,
}

#[derive(Serialize)]
struct Document<'a> {
    summary: Summary,
    results: &'a [Entry],
    #[serde(skip_serializing_if = "Option::is_none")]
    iterations: Option<Vec<IterationEntry<'a>>>,
}

/// `{ summary, results }` comme `bru run --format json` ; avec des données d'itération, `iterations` détaille chacune.
pub fn json(report: &RunReport, redact: &Redact) -> String {
    let entries = entries(report, redact);
    let with_data = report.iterations.iter().any(|i| i.row.is_some());
    let iterations = with_data.then(|| {
        report
            .iterations
            .iter()
            .map(|it| {
                let own: Vec<Entry> = entries.iter().filter(|e| e.iteration_index == it.index).cloned().collect();
                IterationEntry { iteration_index: it.index, iteration_data: &it.row, summary: Summary::of(&own) }
            })
            .collect()
    });
    let document = Document { summary: Summary::of(&entries), results: &entries, iterations };
    serde_json::to_string_pretty(&document).unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------------------------------
// JUnit

fn xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c if (c as u32) < 0x20 => {}
            c => out.push(c),
        }
    }
    out
}

fn hostname() -> String {
    ["HOSTNAME", "COMPUTERNAME"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname").ok().map(|h| h.trim().to_owned()).filter(|h| !h.is_empty())
        })
        .unwrap_or_else(|| "localhost".into())
}

/// Un `<testcase>` : `failure` (message d'un test ou d'une assertion en échec) ou rien.
fn testcase(name: &str, status: &str, class: &str, time: f64, failure: Option<&str>) -> String {
    let head = format!(
        "    <testcase name=\"{}\" status=\"{}\" classname=\"{}\" time=\"{time:.3}\"",
        xml(name),
        xml(status),
        xml(class)
    );
    match failure {
        Some(message) => {
            format!("{head}>\n      <failure type=\"failure\" message=\"{}\"/>\n    </testcase>\n", xml(message))
        }
        None => format!("{head}/>\n"),
    }
}

/// Un `<testsuite>` par requête, comme `bru run --format junit` : les assertions, puis les tests de pré-requête, de la
/// requête et de post-réponse. Une requête en erreur n'a qu'un cas, en échec ; une requête ignorée porte `<skipped>`.
pub fn junit(report: &RunReport, redact: &Redact) -> String {
    let entries = entries(report, redact);
    let several = report.iterations.len() > 1;
    let (host, stamp) = (hostname(), now_iso());
    let timestamp = stamp.trim_end_matches('Z');
    let mut out = String::from("<?xml version=\"1.0\"?>\n<testsuites>\n");
    for e in &entries {
        let class = without_extension(&e.path);
        let total = e.assertion_results.len()
            + e.pre_request_test_results.len()
            + e.test_results.len()
            + e.post_response_test_results.len();
        let each = if total == 0 { 0.0 } else { e.run_duration / total as f64 };
        let mut cases = String::new();
        let mut failures = 0;
        for a in &e.assertion_results {
            let message = (a.status == "fail").then(|| a.error.clone().unwrap_or_default());
            failures += usize::from(message.is_some());
            cases.push_str(&testcase(
                &format!("{} {}", a.lhs_expr, a.rhs_expr),
                &a.status,
                class,
                each,
                message.as_deref(),
            ));
        }
        for t in e.all_tests() {
            let message = (t.status == "fail").then(|| t.error.clone().unwrap_or_default());
            failures += usize::from(message.is_some());
            cases.push_str(&testcase(&t.description, &t.status, class, each, message.as_deref()));
        }
        let (mut errors, mut tests, mut skipped) = (0, total, String::new());
        if e.status == "skipped" {
            let why = if e.skip_reason.as_deref() == Some("bail") {
                "Request skipped due to bail"
            } else {
                "Request Skipped"
            };
            skipped = format!("    <skipped message=\"{}\"/>\n", xml(why));
        } else if e.status == "error" {
            (errors, tests, failures) = (1, 1, 0);
            cases = format!(
                "    <testcase name=\"Test suite has no errors\" status=\"fail\" classname=\"{}\" time=\"{:.3}\">\n      \
                 <error type=\"error\" message=\"{}\"/>\n    </testcase>\n",
                xml(class),
                e.run_duration,
                xml(e.error.as_deref().unwrap_or_default())
            );
        }
        let name = if several { format!("{} [itération {}]", e.name, e.iteration_index + 1) } else { e.name.clone() };
        out.push_str(&format!(
            "  <testsuite name=\"{}\" file=\"{}\" errors=\"{errors}\" failures=\"{failures}\" skipped=\"{}\" tests=\"{tests}\" \
             timestamp=\"{timestamp}\" hostname=\"{}\" time=\"{:.3}\">\n{cases}{skipped}  </testsuite>\n",
            xml(&name),
            xml(&e.test.filename),
            usize::from(e.status == "skipped"),
            xml(&host),
            e.run_duration
        ));
    }
    out.push_str("</testsuites>\n");
    out
}

// ---------------------------------------------------------------------------------------------------------------------
// HTML

/// Ce que la page HTML affiche en plus des résultats.
pub struct Meta {
    pub collection: String,
    /// Date de fin du run, ISO 8601 ([`now_iso`]).
    pub completed_at: String,
}

fn html(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;")
}

const MAX_BODY_CHARS: usize = 100_000;

fn pretty(value: &Value) -> String {
    let text = match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    match text.char_indices().nth(MAX_BODY_CHARS) {
        Some((at, _)) => format!("{}\n… (tronqué, {} caractères de plus)", &text[..at], text[at..].chars().count()),
        None => text,
    }
}

fn headers_block(title: &str, headers: &Option<Map<String, Value>>) -> String {
    match headers {
        Some(map) if !map.is_empty() => {
            let rows: String = map
                .iter()
                .map(|(k, v)| format!("<tr><th>{}</th><td>{}</td></tr>", html(k), html(&pretty(v))))
                .collect();
            format!(
                "<details class=\"sub\"><summary>{title} ({})</summary><table class=\"kv\">{rows}</table></details>",
                map.len()
            )
        }
        _ => String::new(),
    }
}

fn body_block(title: &str, data: &Option<Value>) -> String {
    match data {
        Some(value) if value != &Value::String(String::new()) => {
            format!("<details class=\"sub\"><summary>{title}</summary><pre>{}</pre></details>", html(&pretty(value)))
        }
        _ => String::new(),
    }
}

fn checks(title: &str, tests: &[TestEntry]) -> String {
    if tests.is_empty() {
        return String::new();
    }
    let rows: String = tests
        .iter()
        .map(|t| {
            let ok = t.status == "pass";
            let error = t.error.as_ref().map(|e| format!("<pre class=\"err\">{}</pre>", html(e))).unwrap_or_default();
            format!(
                "<li class=\"{}\"><span class=\"mark\">{}</span><div>{}{error}</div></li>",
                if ok { "ok" } else { "ko" },
                if ok { "✓" } else { "✗" },
                html(&t.description)
            )
        })
        .collect();
    format!("<h4>{title}</h4><ul class=\"checks\">{rows}</ul>")
}

fn assertion_rows(list: &[AssertionEntry]) -> String {
    let tests: Vec<TestEntry> = list
        .iter()
        .map(|a| TestEntry {
            status: a.status.clone(),
            description: format!("{} {}", a.lhs_expr, a.rhs_expr),
            error: a.error.clone(),
            is_script_error: None,
        })
        .collect();
    checks("Assertions", &tests)
}

fn entry_html(e: &Entry) -> String {
    let failed = e.failed();
    let (tone, badge) = match (e.status.as_str(), failed) {
        ("skipped", _) => ("skip", "IGNORÉE"),
        ("error", _) => ("ko", "ERREUR"),
        (_, true) => ("ko", "ÉCHEC"),
        _ => ("ok", "RÉUSSIE"),
    };
    let method = e.request.method.clone().unwrap_or_default();
    let http = match &e.response.status {
        Value::Number(n) => format!("{n} {}", e.response.status_text.clone().unwrap_or_default()),
        _ => String::new(),
    };
    let error = e.error.as_ref().map(|m| format!("<pre class=\"err\">{}</pre>", html(m))).unwrap_or_default();
    let url = e.request.url.as_ref().map(|u| format!("<p class=\"url\">{}</p>", html(u))).unwrap_or_default();
    let body = format!(
        "<p class=\"path\">{}</p>{url}{error}{}{}{}{}{}{}{}{}",
        html(&e.path),
        assertion_rows(&e.assertion_results),
        checks("Tests de pré-requête", &e.pre_request_test_results),
        checks("Tests", &e.test_results),
        checks("Tests de post-réponse", &e.post_response_test_results),
        headers_block("En-têtes de la requête", &e.request.headers),
        body_block("Corps de la requête", &e.request.data),
        headers_block("En-têtes de la réponse", &e.response.headers),
        body_block("Corps de la réponse", &e.response.data),
    );
    format!(
        "<details class=\"req {tone}\"{}><summary><span class=\"badge {tone}\">{badge}</span><span class=\"method\">{}</span>\
         <span class=\"name\">{}</span><span class=\"http\">{}</span><span class=\"time\">{}</span></summary>\
         <div class=\"body\">{body}</div></details>",
        if failed || e.status == "error" { " open" } else { "" },
        html(&method),
        html(&e.name),
        html(&http),
        if e.response.response_time > 0.0 { format!("{:.0} ms", e.response.response_time) } else { String::new() },
    )
}

const STYLE: &str = r#"
:root{color-scheme:light dark;--bg:#f6f7f9;--panel:#fff;--ink:#1c2230;--muted:#667085;--line:#e3e6ec;--ok:#16794a;--ok-bg:#e6f4ec;--ko:#b42318;--ko-bg:#fdecea;--skip:#8a5a00;--skip-bg:#fff3d6;--code:#f1f3f7}
@media (prefers-color-scheme:dark){:root{--bg:#12151c;--panel:#1a1e27;--ink:#e6e9f0;--muted:#98a2b3;--line:#2a3040;--ok:#52c98a;--ok-bg:#12301f;--ko:#ff8a80;--ko-bg:#3a1a18;--skip:#f5c04a;--skip-bg:#3a2e10;--code:#12151c}}
*{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:14px/1.5 system-ui,-apple-system,"Segoe UI",sans-serif}
main{max-width:980px;margin:0 auto;padding:32px 16px 64px}
header{display:flex;flex-wrap:wrap;gap:8px 16px;align-items:baseline;justify-content:space-between;margin-bottom:20px}
h1{font-size:22px;margin:0}h2{font-size:16px;margin:28px 0 10px}h3{font-size:13px;margin:22px 0 8px;color:var(--muted);text-transform:uppercase;letter-spacing:.04em}h4{font-size:12px;margin:14px 0 6px;color:var(--muted)}
.meta{color:var(--muted);margin:4px 0 0}.verdict{font-weight:700;padding:4px 12px;border-radius:999px}
.verdict.ok,.badge.ok{background:var(--ok-bg);color:var(--ok)}.verdict.ko,.badge.ko{background:var(--ko-bg);color:var(--ko)}.badge.skip{background:var(--skip-bg);color:var(--skip)}
.sum{margin:0 0 4px;color:var(--muted)}.sum b{color:var(--ink)}.sum b.bad{color:var(--ko)}
.req{background:var(--panel);border:1px solid var(--line);border-radius:10px;margin:8px 0}.req>summary{display:flex;gap:10px;align-items:center;padding:10px 14px;cursor:pointer;list-style:none}
.req>summary::-webkit-details-marker{display:none}.req .name{flex:1;font-weight:600;overflow-wrap:anywhere}.req .method{font:600 12px ui-monospace,monospace;color:var(--muted);min-width:48px}
.req .http,.req .time{color:var(--muted);font-size:12px;white-space:nowrap}.badge{font-size:11px;font-weight:700;padding:2px 8px;border-radius:999px;white-space:nowrap}
.req .body{padding:2px 14px 14px;border-top:1px solid var(--line)}.path,.url{margin:10px 0 0;color:var(--muted);font:12px ui-monospace,monospace;overflow-wrap:anywhere}
.checks{list-style:none;margin:0;padding:0}.checks li{display:flex;gap:8px;padding:3px 0}.checks .mark{font-weight:700;width:14px}.checks .ok .mark{color:var(--ok)}.checks .ko .mark{color:var(--ko)}
pre{background:var(--code);border-radius:6px;padding:10px 12px;overflow:auto;font:12px/1.5 ui-monospace,monospace;margin:6px 0 0;max-height:420px;white-space:pre-wrap;overflow-wrap:anywhere}pre.err{color:var(--ko)}
.sub{margin-top:10px}.sub>summary{cursor:pointer;color:var(--muted);font-size:12px}table.kv{border-collapse:collapse;margin-top:6px;font:12px ui-monospace,monospace;width:100%}
.kv th{text-align:left;color:var(--muted);padding:2px 12px 2px 0;vertical-align:top;white-space:nowrap}.kv td{overflow-wrap:anywhere}
.halt{background:var(--skip-bg);color:var(--skip);border-radius:8px;padding:10px 14px;margin:16px 0}
"#;

fn halt_note(halt: &Halt) -> String {
    match halt {
        Halt::Bail { request, reason, remaining } => {
            format!("Arrêt au premier échec : {reason} dans « {request} ». {remaining} requête(s) ignorée(s).")
        }
        Halt::StopExecution { request, remaining } => {
            format!("Run arrêté par un script dans « {request} ». {remaining} requête(s) ignorée(s).")
        }
        Halt::Loop => "Run arrêté : trop de sauts (setNextRequest), probablement une boucle sans fin.".into(),
        Halt::Cancelled => "Run annulé.".into(),
    }
}

/// Une page HTML autonome (ni script ni ressource externe) : verdict, compteurs, puis chaque requête dépliable.
pub fn html_page(report: &RunReport, redact: &Redact, meta: &Meta) -> String {
    let entries = entries(report, redact);
    let s = Summary::of(&entries);
    let failed = report.failed();
    let failures = s.failed_requests + s.error_requests;
    let tests = s.passed_tests + s.passed_pre_request_tests + s.passed_post_response_tests;
    let total = s.total_tests + s.total_pre_request_tests + s.total_post_response_tests;
    let plural = |n: usize, one: &str, many: &str| format!("<b>{n}</b> {}", if n > 1 { many } else { one });
    let mut counts = vec![plural(s.passed_requests, "réussie", "réussies")];
    if failures > 0 {
        counts.push(format!("<b class=\"bad\">{failures}</b> en échec"));
    }
    if s.skipped_requests > 0 {
        counts.push(plural(s.skipped_requests, "ignorée", "ignorées"));
    }
    if total + s.total_assertions > 0 {
        counts.push(format!("<b>{}</b> sur {} vérifications", tests + s.passed_assertions, total + s.total_assertions));
    }
    counts.push(format!("{:.2} s", report.elapsed.as_secs_f64()));
    let counts = format!("<p class=\"sum\">{}</p>", counts.join(" · "));
    let environment =
        report.environment.as_deref().map(|e| format!(" · environnement {}", html(e))).unwrap_or_default();
    let several = report.iterations.len() > 1;
    let mut list = String::new();
    let mut last = None;
    for e in &entries {
        if several && last != Some(e.iteration_index) {
            let row = report
                .iterations
                .get(e.iteration_index)
                .and_then(|it| it.row.as_ref())
                .map(|row| format!(" — {}", html(&Value::Object(row.clone()).to_string())))
                .unwrap_or_default();
            list.push_str(&format!("<h3>Itération {}{row}</h3>", e.iteration_index + 1));
            last = Some(e.iteration_index);
        }
        list.push_str(&entry_html(e));
    }
    let halt =
        report.halt.as_ref().map(|h| format!("<p class=\"halt\">{}</p>", html(&halt_note(h)))).unwrap_or_default();
    format!(
        "<!doctype html>\n<html lang=\"fr\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Rapport d'exécution — {name}</title><style>{STYLE}</style></head><body><main>\
         <header><div><h1>{name}</h1><p class=\"meta\">Sira {version}{environment} · {when}</p></div>\
         <span class=\"verdict {tone}\">{verdict}</span></header>\
         {counts}{halt}<h2>Requêtes</h2>{list}</main></body></html>\n",
        name = html(&meta.collection),
        version = env!("CARGO_PKG_VERSION"),
        when = html(&meta.completed_at),
        tone = if failed { "ko" } else { "ok" },
        verdict = if failed { "✗ Échec" } else { "✓ Réussi" },
    )
}

// ---------------------------------------------------------------------------------------------------------------------
// Date

/// Jour civil (année, mois, jour) à `days` jours du 1er janvier 1970.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// `2026-10-06T12:34:56.789Z` pour un instant exprimé en millisecondes depuis l'époque Unix.
pub fn iso_from_millis(millis: u64) -> String {
    let (days, rest) = ((millis / 86_400_000) as i64, millis % 86_400_000);
    let (year, month, day) = civil(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rest / 3_600_000,
        rest / 60_000 % 60,
        rest / 1000 % 60,
        rest % 1000
    )
}

/// L'heure courante en ISO 8601 (UTC).
pub fn now_iso() -> String {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
    iso_from_millis(u64::try_from(since.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;
    use xc_engine::{HttpResponse, Timings};
    use xc_script::NextRequest;

    use super::*;
    use crate::data::Row;
    use crate::pipeline::{Outcome, RunError, Stage};
    use crate::run::{Iteration, RunReport};

    fn passing(path: &str) -> RequestResult {
        let mut outcome = Outcome::placeholder("get", "http://x/");
        outcome.sent_headers = vec![("accept".into(), "*/*".into())];
        outcome.response = Some(HttpResponse {
            status: 200,
            reason: "OK".into(),
            http_version: "HTTP/1.1".into(),
            remote_addr: String::new(),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("x-a".into(), "1".into()),
                ("x-a".into(), "2".into()),
            ],
            body: br#"{"ok":true}"#.to_vec(),
            timings: Timings { total_ms: 12.0, ..Timings::default() },
        });
        RequestResult {
            iteration: 0,
            name: path.trim_end_matches(".yml").into(),
            path: path.into(),
            skip: None,
            duration: Duration::from_millis(40),
            outcome,
        }
    }

    fn with_test(mut r: RequestResult, description: &str, pass: bool) -> RequestResult {
        r.outcome.tests.results.push(xc_script::TestResult {
            description: description.into(),
            status: if pass { "pass" } else { "fail" }.into(),
            error: (!pass).then(|| "expected 1 to equal 2".into()),
            actual: None,
            expected: None,
        });
        r
    }

    fn report(results: Vec<RequestResult>) -> RunReport {
        RunReport {
            results,
            iterations: vec![Iteration { index: 0, row: None }],
            halt: None,
            elapsed: Duration::from_millis(100),
            environment: Some("prod".into()),
        }
    }

    #[test]
    fn ef_cli_02_an_entry_has_bruno_s_json_shape() {
        let entry = passing("a.yml").entry();
        let value = serde_json::to_value(&entry).unwrap();
        assert_eq!(value["status"], "pass");
        assert_eq!(value["test"]["filename"], "a.yml");
        assert_eq!(value["request"]["method"], "GET");
        assert_eq!(value["response"]["status"], 200);
        assert_eq!(value["response"]["statusText"], "OK");
        assert_eq!(value["response"]["data"], json!({ "ok": true }));
        assert_eq!(value["response"]["headers"]["x-a"], "1, 2");
        assert_eq!(value["error"], Value::Null);
        assert_eq!(value["runDuration"], 0.04);
        assert_eq!(value["suitename"], "a");
        assert!(value.get("skipped").is_none());
    }

    #[test]
    fn ef_cli_02_the_summary_counts_like_get_runner_summary() {
        let mut errored = passing("b.yml");
        errored.outcome.error = Some(RunError { stage: Stage::Send, message: "connexion refusée".into() });
        let mut skipped = passing("c.yml");
        skipped.skip = Some(Skip::Bail);
        let results =
            vec![with_test(passing("a.yml"), "ok", true), with_test(passing("d.yml"), "ko", false), errored, skipped];
        let s = Summary::of(&report(results).entries());
        assert_eq!((s.total_requests, s.passed_requests, s.failed_requests, s.error_requests), (4, 1, 1, 1));
        assert_eq!((s.skipped_requests, s.skipped_by_bail), (1, 1));
        assert_eq!((s.total_tests, s.passed_tests, s.failed_tests), (2, 1, 1));
        assert!(s.failed());
    }

    #[test]
    fn ef_cli_02_a_script_error_fails_the_request_without_counting_as_a_test() {
        let mut r = passing("a.yml");
        r.outcome.post.error = Some("boom".into());
        let entries = report(vec![r]).entries();
        assert_eq!(entries[0].post_response_test_results[0].description, "Post-Response Script Error");
        let s = Summary::of(&entries);
        assert_eq!((s.failed_requests, s.total_post_response_tests, s.failed_post_response_tests), (1, 0, 0));
        assert!(s.failed());
    }

    #[test]
    fn ef_cli_02_redaction_drops_headers_and_bodies() {
        let mut entries = vec![passing("a.yml").entry()];
        Redact { headers: vec!["X-A".into()], response_body: true, ..Redact::default() }.apply(&mut entries);
        let headers = entries[0].response.headers.as_ref().unwrap();
        assert!(headers.get("x-a").is_none() && headers.get("content-type").is_some());
        assert!(entries[0].response.data.is_none());
        Redact { all_headers: true, ..Redact::default() }.apply(&mut entries);
        assert!(entries[0].request.headers.as_ref().unwrap().is_empty());
    }

    #[test]
    fn ef_cli_02_json_report_lists_iterations_only_with_data() {
        let plain: Value = serde_json::from_str(&json(&report(vec![passing("a.yml")]), &Redact::default())).unwrap();
        assert!(plain.get("iterations").is_none());
        assert_eq!(plain["summary"]["totalRequests"], 1);

        let row: Row = json!({ "id": 7 }).as_object().cloned().unwrap();
        let mut with = report(vec![passing("a.yml")]);
        with.iterations = vec![Iteration { index: 0, row: Some(row) }];
        let doc: Value = serde_json::from_str(&json(&with, &Redact::default())).unwrap();
        assert_eq!(doc["iterations"][0]["iterationData"], json!({ "id": 7 }));
        assert_eq!(doc["iterations"][0]["summary"]["passedRequests"], 1);
    }

    #[test]
    fn ef_cli_02_junit_has_one_suite_per_request_with_failures_errors_and_skips() {
        let mut errored = passing("b.yml");
        errored.outcome.error = Some(RunError { stage: Stage::Send, message: "a < b & \"c\"".into() });
        let mut skipped = passing("c.yml");
        skipped.skip = Some(Skip::Bail);
        let xml =
            junit(&report(vec![with_test(passing("a.yml"), "adds up", false), errored, skipped]), &Redact::default());
        assert!(xml.starts_with("<?xml version=\"1.0\"?>\n<testsuites>"), "{xml}");
        assert!(
            xml.contains("<testsuite name=\"a\" file=\"a.yml\" errors=\"0\" failures=\"1\" skipped=\"0\" tests=\"1\""),
            "{xml}"
        );
        assert!(xml.contains("<failure type=\"failure\" message=\"expected 1 to equal 2\"/>"), "{xml}");
        assert!(xml.contains("errors=\"1\" failures=\"0\" skipped=\"0\" tests=\"1\""), "{xml}");
        assert!(xml.contains("<error type=\"error\" message=\"a &lt; b &amp; &quot;c&quot;\"/>"), "{xml}");
        assert!(xml.contains("<skipped message=\"Request skipped due to bail\"/>"), "{xml}");
        assert_eq!(xml.matches("<testsuite ").count(), 3);
    }

    #[test]
    fn ef_cli_02_junit_numbers_the_suites_of_a_multi_iteration_run() {
        let mut r = report(vec![passing("a.yml"), passing("a.yml")]);
        r.results[1].iteration = 1;
        r.iterations.push(Iteration { index: 1, row: None });
        let xml = junit(&r, &Redact::default());
        assert!(xml.contains("name=\"a [itération 1]\"") && xml.contains("name=\"a [itération 2]\""), "{xml}");
    }

    #[test]
    fn ef_cli_02_junit_removes_characters_xml_cannot_hold() {
        assert_eq!(xml("a\u{1}b\nc"), "ab&#10;c");
    }

    #[test]
    fn ef_cli_02_html_is_standalone_escaped_and_opens_failures() {
        let mut bad = with_test(passing("b.yml"), "<script>alert(1)</script>", false);
        bad.outcome.next_request = NextRequest::Unset;
        let page = html_page(
            &report(vec![passing("a.yml"), bad]),
            &Redact::default(),
            &Meta { collection: "Démo <1>".into(), completed_at: "2026-10-06T00:00:00.000Z".into() },
        );
        assert!(page.starts_with("<!doctype html>"));
        assert!(!page.contains("<script"), "pas de script, ni injecté ni embarqué");
        assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(page.contains("Démo &lt;1&gt;"));
        assert!(page.contains("✗ Échec"));
        assert_eq!(page.matches("<details class=\"req ko\" open>").count(), 1);
        assert!(page.contains("<details class=\"req ok\"><summary>"));
        assert!(!page.contains("src=") && !page.contains("<link"), "aucune ressource externe");
    }

    #[test]
    fn ef_cli_02_html_bodies_are_cut_when_huge() {
        let mut r = passing("a.yml");
        r.outcome.response.as_mut().unwrap().body = "x".repeat(MAX_BODY_CHARS + 50).into_bytes();
        let page = html_page(
            &report(vec![r]),
            &Redact::default(),
            &Meta { collection: "c".into(), completed_at: String::new() },
        );
        assert!(page.contains("tronqué, 50 caractères de plus"));
    }

    #[test]
    fn ef_cli_02_dates_are_iso_8601_in_utc() {
        assert_eq!(iso_from_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_from_millis(1_700_000_000_123), "2023-11-14T22:13:20.123Z");
        assert_eq!(iso_from_millis(951_782_400_000), "2000-02-29T00:00:00.000Z");
        assert!(now_iso().ends_with('Z'));
    }
}
