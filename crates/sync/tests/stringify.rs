//! ENF-COMP-02 : `xc_sync::stringify` écrit, octet pour octet, les fichiers YAML qu'écrit Bruno.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use xc_sync::stringify;

fn json(path: PathBuf) -> Value {
    serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap_or_else(|e| panic!("{} : {e}", path.display()))
}

fn diff(expected: &str, got: &str) -> String {
    let (e, g): (Vec<_>, Vec<_>) = (expected.split('\n').collect(), got.split('\n').collect());
    let first = e.iter().zip(&g).position(|(a, b)| a != b).unwrap_or(e.len().min(g.len()));
    let mut out = format!("  première différence ligne {} (- attendu, + obtenu)\n", first + 1);
    for i in first.saturating_sub(3)..first + 4 {
        match (e.get(i), g.get(i)) {
            (Some(a), Some(b)) if a == b => out.push_str(&format!("    {a}\n")),
            (a, b) => {
                a.into_iter().for_each(|a| out.push_str(&format!("  - {a:?}\n")));
                b.into_iter().for_each(|b| out.push_str(&format!("  + {b:?}\n")));
            }
        }
    }
    out
}

/// Compare chaque `<cas>.yml` de la famille au rendu de son entrée JSON.
fn check(family: &str, min_cases: usize, render: impl Fn(&Path) -> String) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stringify").join(family);
    let mut cases: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml"))
        .collect();
    cases.sort();
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|yml| {
            let expected = fs::read_to_string(yml).unwrap();
            let got = render(yml);
            (got != expected).then(|| format!("{}\n{}", yml.display(), diff(&expected, &got)))
        })
        .collect();
    assert!(cases.len() >= min_cases, "{family} : {} cas, au moins {min_cases} attendus", cases.len());
    assert!(
        failures.is_empty(),
        "{} cas sur {} diffèrent de Bruno :\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}

#[test]
fn enf_comp_02_stringify_items_match_bruno() {
    check("items", 70, |yml| stringify::item(&json(yml.with_extension("json"))));
}

#[test]
fn enf_comp_02_stringify_folders_match_bruno() {
    check("folders", 10, |yml| stringify::folder(&json(yml.with_extension("json"))));
}

#[test]
fn enf_comp_02_stringify_collections_match_bruno() {
    check("collections", 8, |yml| {
        stringify::collection(&json(yml.with_extension("root.json")), &json(yml.with_extension("config.json")))
    });
}

#[test]
fn enf_comp_02_stringify_environments_match_bruno() {
    check("environments", 8, |yml| stringify::environment(&json(yml.with_extension("json"))));
}
