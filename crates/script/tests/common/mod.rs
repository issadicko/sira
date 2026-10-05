use serde_json::{json, Map, Value};
use xc_script::{Input, Limits, Phase, ScriptRequest, ScriptResponse, Vars};

pub fn object(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

pub fn input(phase: Phase, script: &str) -> Input {
    Input {
        phase,
        script: script.to_owned(),
        request: ScriptRequest {
            name: "List users".into(),
            method: "GET".into(),
            url: "{{baseUrl}}/users/:id?x={{x}}".into(),
            headers: object(json!({ "Accept": "application/json", "content-type": "application/json" })),
            data: Some(json!(r#"{"a":1}"#)),
            tags: vec!["smoke".into()],
            ..ScriptRequest::default()
        },
        response: (phase != Phase::Pre).then(|| ScriptResponse {
            status: 200,
            status_text: "OK".into(),
            headers: object(json!({ "content-type": "application/json", "x-id": "7" })),
            data: json!({ "name": "Ada", "items": [{ "id": 1, "tags": ["a"] }, { "id": 2, "tags": ["b", "c"] }] }),
            response_time: 42,
            url: "http://x/users/1".into(),
            ..ScriptResponse::default()
        }),
        vars: Vars {
            env: object(json!({ "baseUrl": "http://x", "__name__": "dev", "retries": 3, "obj": { "k": "{{x}}" } })),
            runtime: object(json!({ "x": "1" })),
            global: object(json!({ "g": "global" })),
            process_env: object(json!({ "HOME": "/home/ada" })),
            ..Vars::default()
        },
        collection_name: "Shop".into(),
        collection_path: "/work/shop".into(),
        execution_mode: "cli".into(),
        dynamic: |name| (name == "guid").then(|| "GUID".to_owned()),
        limits: Limits::default(),
        cancel: Default::default(),
    }
}
