use std::time::{Duration, Instant};

use xc_core::pretty::pretty_json;

#[test]
fn enf_perf_04_pretty_json_indents_like_json_stringify() {
    let compact = r#"{"id":"TX-1","montant":15000,"vide":{},"liste":[],"tags":["a","b"],"client":{"nom":"Aminata","adresses":[{"ville":"Ouagadougou"}]},"ok":true,"rien":null}"#;
    let expected = "{\n  \"id\": \"TX-1\",\n  \"montant\": 15000,\n  \"vide\": {},\n  \"liste\": [],\n  \"tags\": [\n    \"a\",\n    \"b\"\n  ],\n  \"client\": {\n    \"nom\": \"Aminata\",\n    \"adresses\": [\n      {\n        \"ville\": \"Ouagadougou\"\n      }\n    ]\n  },\n  \"ok\": true,\n  \"rien\": null\n}";
    assert_eq!(pretty_json(compact).as_deref(), Some(expected));
    assert_eq!(pretty_json(expected).as_deref(), Some(expected));

    let spaced = " [ [ ] , { } , [ { \"a\" : \"{[,:]} \\\" x\" } ] ]\n";
    let expected = "[\n  [],\n  {},\n  [\n    {\n      \"a\": \"{[,:]} \\\" x\"\n    }\n  ]\n]";
    assert_eq!(pretty_json(spaced).as_deref(), Some(expected));
    assert_eq!(pretty_json(" 42 ").as_deref(), Some("42"));
    assert_eq!(pretty_json("\"texte\"").as_deref(), Some("\"texte\""));
}

#[test]
fn enf_perf_04_pretty_json_keeps_tokens_verbatim() {
    let text = r#"{"grand":12345678901234567890,"flottant":1.0,"exp":1E+2,"neg":-0,"e":"\u00e9\/\"\\\n","é":"ü"}"#;
    let expected = "{\n  \"grand\": 12345678901234567890,\n  \"flottant\": 1.0,\n  \"exp\": 1E+2,\n  \"neg\": -0,\n  \"e\": \"\\u00e9\\/\\\"\\\\\\n\",\n  \"é\": \"ü\"\n}";
    assert_eq!(pretty_json(text).as_deref(), Some(expected));
}

#[test]
fn enf_perf_04_pretty_json_ignores_what_is_not_json() {
    for text in ["", "  ", "<html></html>", "{\"a\":1", "{\"a\":1} x", "{'a':1}", "NaN", "[1,]"] {
        assert_eq!(pretty_json(text), None, "{text:?}");
    }
}

#[test]
fn enf_perf_04_ten_megabytes_are_formatted_quickly() {
    let item = r#"{"id":"TX-2026-000000","montant":15000.5,"devise":"XOF","client":{"nom":"Aminata Ouédraogo","tags":["a","b"]},"ok":true}"#;
    let count = 10 * 1024 * 1024 / (item.len() + 1);
    let text = format!("[{}]", vec![item; count].join(","));
    let start = Instant::now();
    let pretty = pretty_json(&text).expect("JSON valide");
    let elapsed = start.elapsed();
    assert_eq!(pretty.lines().count(), count * 13 + 2);
    assert!(elapsed < Duration::from_secs(1), "10 Mo formatés en {elapsed:?}");
}
