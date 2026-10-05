use serde_json::{json, Map, Value};
use xc_script::{run, Input, Limits, NextRequest, Output, Phase, ScriptRequest, ScriptResponse, Vars};

fn object(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

fn input(phase: Phase, script: &str) -> Input {
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

fn exec(phase: Phase, script: &str) -> Output {
    run(input(phase, script)).unwrap()
}

fn pre(script: &str) -> Output {
    exec(Phase::Pre, script)
}

fn post(script: &str) -> Output {
    exec(Phase::Post, script)
}

fn failed(out: &Output) -> String {
    out.error.as_ref().map(|e| e.message.clone()).unwrap_or_default()
}

#[test]
fn ef_scr_02_runtime_variables_round_trip_and_flag_only_real_changes() {
    let out = pre("bru.setVar('token', 'abc'); bru.setVar('x', '1'); bru.setVar('n', 2);");
    assert_eq!(out.error, None);
    assert_eq!(out.vars.runtime["token"], "abc");
    assert_eq!(out.vars.runtime["n"], 2);
    assert!(out.dirty.runtime && !out.dirty.env && !out.dirty.global && !out.dirty.collection);

    let unchanged = pre("bru.setVar('x', '1'); bru.deleteVar('nope');");
    assert!(!unchanged.dirty.runtime);
}

#[test]
fn ef_scr_02_getters_interpolate_each_read_and_keep_types() {
    let out = pre("
        bru.setVar('seen', JSON.stringify([bru.getEnvVar('baseUrl'), bru.getEnvVar('retries'), bru.getEnvVar('obj'),
            bru.getGlobalEnvVar('g'), bru.getProcessEnv('HOME'), bru.interpolate('{{baseUrl}}/{{$guid}}/{{nope}}')]));
    ");
    assert_eq!(out.error, None);
    let seen: Value = serde_json::from_str(out.vars.runtime["seen"].as_str().unwrap()).unwrap();
    assert_eq!(seen, json!(["http://x", 3, { "k": "1" }, "global", "/home/ada", "http://x/GUID/{{nope}}"]));
}

#[test]
fn ef_scr_02_invalid_names_are_refused_with_brunos_messages() {
    let out = pre("
        const errors = [];
        for (const f of [() => bru.setVar('a b', 1), () => bru.setEnvVar('', 1), () => bru.setVar('', 1), () => bru.getVar('a b')]) {
            try { f(); } catch (e) { errors.push(e.message); }
        }
        bru.setVar('errors', errors);
    ");
    assert_eq!(
        out.vars.runtime["errors"],
        json!([
            "Variable name: \"a b\" contains invalid characters! Names must only contain alpha-numeric characters, \"-\", \"_\", \".\"",
            "Creating a env variable without specifying a name is not allowed.",
            "Creating a variable without specifying a name is not allowed.",
            "Variable name: \"a b\" contains invalid characters! Names must only contain alpha-numeric characters, \"-\", \"_\", \".\"",
        ])
    );
}

#[test]
fn ef_scr_02_environment_writes_are_flagged_and_the_environment_name_survives() {
    let out = pre("bru.setEnvVar('token', 't'); bru.deleteAllEnvVars(); bru.setEnvVar('after', 1);");
    assert!(out.dirty.env);
    assert_eq!(Value::Object(out.vars.env.clone()), json!({ "__name__": "dev", "after": 1 }));
    let named = pre("bru.setVar('n', bru.getEnvName()); bru.setVar('all', bru.getAllEnvVars());");
    assert_eq!(named.vars.runtime["n"], "dev");
    assert!(named.vars.runtime["all"].get("__name__").is_none());
}

#[test]
fn ef_scr_02_the_request_is_seen_uninterpolated_and_changed_through_setters() {
    let out = pre("
        const before = [req.getUrl(), req.getMethod(), req.getHeader('Accept'), req.getHeader('accept'), req.getName(), req.getTags(), req.getBody()];
        req.setUrl('{{baseUrl}}/other');
        req.setMethod('POST');
        req.setHeader('X-Trace', 't1');
        req.deleteHeader('Accept');
        req.setBody({ b: 2 });
        bru.setVar('seen', [before, req.url, req.getUrl(), req.getHost(), req.getPath()]);
    ");
    assert_eq!(out.error, None);
    assert_eq!(
        out.vars.runtime["seen"],
        json!([
            ["{{baseUrl}}/users/:id?x={{x}}", "GET", "application/json", null, "List users", ["smoke"], { "a": 1 }],
            "{{baseUrl}}/users/:id?x={{x}}",
            "{{baseUrl}}/other",
            "{{baseUrl}}",
            "/other"
        ])
    );
    assert_eq!(out.request.url, "{{baseUrl}}/other");
    assert_eq!(out.request.method, "POST");
    assert_eq!(
        Value::Object(out.request.headers.clone()),
        json!({ "content-type": "application/json", "X-Trace": "t1" })
    );
    assert_eq!(out.request.data, Some(json!(r#"{"b":2}"#)));
    assert_eq!(out.headers_to_delete, vec!["Accept"]);
}

#[test]
fn ef_scr_02_properties_of_req_are_snapshots_that_setters_do_not_refresh() {
    let out = pre("req.headers['X-A'] = '1'; req.body = { z: 1 };");
    assert_eq!(
        Value::Object(out.request.headers.clone()),
        json!({ "Accept": "application/json", "content-type": "application/json" })
    );
    assert_eq!(out.request.data, Some(json!(r#"{"a":1}"#)));
}

#[test]
fn ef_scr_02_res_is_absent_before_the_request_and_complete_after_it() {
    let out = pre("res.status");
    assert_eq!(failed(&out), "res is not defined");

    let out = post("
        bru.setVar('seen', [res.status, res.statusText, res.getStatus(), res.headers['x-id'], res.getHeader('X-Id'), res.getHeader('nope'),
            res.body.name, res.getBody().items.length, res.responseTime, res.getUrl(), res('name'), res('items.id'), res('..tags'),
            res('items[?].id', (i) => i.id > 1), res('items[1].id')]);
    ");
    assert_eq!(out.error, None);
    assert_eq!(
        out.vars.runtime["seen"],
        json!([
            200,
            "OK",
            200,
            "7",
            "7",
            null,
            "Ada",
            2,
            42,
            "http://x/users/1",
            "Ada",
            [1, 2],
            ["a", "b", "c"],
            [2],
            2
        ])
    );
}

#[test]
fn ef_scr_02_set_body_replaces_the_response_body_for_what_follows() {
    let out = post("res.setBody({ replaced: true }); bru.setVar('now', res.getBody());");
    assert_eq!(out.response_data, Some(json!({ "replaced": true })));
    assert_eq!(out.vars.runtime["now"], json!({ "replaced": true }));
    assert_eq!(post("1").response_data, None);
}

#[test]
fn ef_tst_01_tests_use_chai_and_report_each_outcome() {
    let out = post(
        "
        test('status is 200', () => { expect(res.status).to.equal(200); });
        test('wrong', () => { expect(res.status).to.equal(404); });
        await test('async passes', async () => { await bru.sleep(1); expect(res.body.name).to.be.a('string'); });
        test('throws', () => { throw new Error('boom'); });
        test('assert style', () => { assert.deepEqual(res.body.items[0], { id: 1, tags: ['a'] }); });
    ",
    );
    assert_eq!(out.error, None);
    let summary: Vec<(String, String, Option<String>)> =
        out.results.iter().map(|r| (r.description.clone(), r.status.clone(), r.error.clone())).collect();
    assert_eq!(
        summary,
        vec![
            ("wrong".into(), "fail".into(), Some("expected 200 to equal 404".into())),
            ("status is 200".into(), "pass".into(), None),
            ("async passes".into(), "pass".into(), None),
            ("throws".into(), "fail".into(), Some("boom".into())),
            ("assert style".into(), "pass".into(), None),
        ]
    );
    assert_eq!(out.results[0].actual, Some(json!(200)));
    assert_eq!(out.results[0].expected, Some(json!(404)));
}

#[test]
fn ef_tst_01_results_come_in_settlement_order_like_in_bruno() {
    let out = post("test('A', () => {}); test('B', () => { throw new Error('x'); });");
    let names: Vec<_> = out.results.iter().map(|r| r.description.as_str()).collect();
    assert_eq!(names, ["B", "A"]);
}

#[test]
fn ef_tst_01_bruno_chai_extensions_json_json_body_and_json_schema() {
    let out = post("
        test('json', () => { expect(res.body).to.be.json; expect('x').to.not.be.json; });
        test('jsonBody', () => {
            expect(res.body).to.have.jsonBody();
            expect(res.body).to.have.jsonBody('items[0].id');
            expect(res.body).to.have.jsonBody('items[1].tags', ['b', 'c']);
            expect(res.body).to.not.have.jsonBody('items[5]');
        });
        test('schema ok', () => {
            expect(res.body).to.have.jsonSchema({ type: 'object', required: ['name'], properties: { name: { type: 'string' } } });
        });
        test('schema ko', () => { expect(res.body).to.have.jsonSchema({ type: 'object', properties: { name: { type: 'number' } } }); });
        test('schema version', () => { expect({}).to.have.jsonSchema({ $schema: 'http://json-schema.org/draft-04/schema#' }); });
    ");
    assert_eq!(out.error, None);
    let by_name = |n: &str| out.results.iter().find(|r| r.description == n).unwrap();
    assert_eq!(by_name("json").status, "pass");
    assert_eq!(by_name("jsonBody").status, "pass");
    assert_eq!(by_name("schema ok").status, "pass");
    let ko = by_name("schema ko");
    assert_eq!(ko.status, "fail");
    assert!(ko.error.as_ref().unwrap().contains("to match JSON schema, validation errors: [{"), "{ko:?}");
    assert!(by_name("schema version")
        .error
        .as_ref()
        .unwrap()
        .starts_with("Unsupported JSON Schema version: \"http://json-schema.org/draft-04/schema#\""));
}

#[test]
fn ef_scr_01_a_failing_script_keeps_what_it_did_before_failing() {
    let out = post(
        "bru.setVar('before', 1); test('first', () => {}); await test('done', () => {}); throw new Error('late');",
    );
    assert_eq!(failed(&out), "late");
    assert_eq!(out.vars.runtime["before"], 1);
    assert_eq!(out.results.len(), 2);
    assert!(out.dirty.runtime);
}

#[test]
fn ef_scr_01_runner_controls_are_reported() {
    let out = pre("bru.setNextRequest('Login'); bru.runner.skipRequest();");
    assert_eq!(out.next_request, NextRequest::Named("Login".into()));
    assert!(out.skip_request && !out.stop_execution);
    assert_eq!(post("bru.setNextRequest(null); bru.runner.stopExecution();").next_request, NextRequest::Stop);
    assert_eq!(post("1").next_request, NextRequest::Unset);
    assert!(post("bru.runner.stopExecution()").stop_execution);
}

#[test]
fn ef_scr_02_console_lines_are_collected_with_their_level() {
    let out = pre(
        "console.log('hi', 1, { a: [1] }); console.error(new Error('bad')); console.info(undefined, null, () => 1);",
    );
    let lines: Vec<_> = out.logs.iter().map(|l| (l.level.as_str(), l.args.clone())).collect();
    assert_eq!(
        lines,
        vec![
            ("log", json!(["hi", 1, { "a": [1] }])),
            ("error", json!(["Error: bad"])),
            ("info", json!([null, null, "function anonymous() {\n    [native code]\n}"])),
        ]
    );
}

#[test]
fn ef_scr_02_scripts_run_in_sloppy_mode_with_brunos_set_timeout_and_sleep() {
    let out = pre("
        leaked = 5;
        await setTimeout(() => bru.setVar('timer', 'fired'), 5);
        bru.setVar('slept', await bru.sleep(5));
        bru.setVar('leaked', leaked);
        bru.setVar('cwd', [bru.cwd(), bru.getCollectionName(), bru.isSafeMode(), req.getExecutionMode()]);
    ");
    assert_eq!(out.error, None);
    assert_eq!(out.vars.runtime["timer"], "fired");
    assert_eq!(out.vars.runtime["slept"], "slept");
    assert_eq!(out.vars.runtime["leaked"], 5);
    assert_eq!(out.vars.runtime["cwd"], json!(["/work/shop", "Shop", true, "cli"]));
}

#[test]
fn ef_scr_02_require_serves_the_embedded_libraries_and_refuses_the_rest() {
    let out = pre("
        const { expect: e } = require('chai');
        e(1).to.equal(1);
        let message;
        try { require('fs'); } catch (err) { message = err.message; }
        bru.setVar('m', message);
    ");
    assert_eq!(out.error, None);
    assert_eq!(out.vars.runtime["m"], "Cannot find module fs");
}

#[test]
fn enf_sec_02_a_script_cannot_run_forever() {
    let mut i = input(Phase::Pre, "while (true) {}");
    i.limits.compute = std::time::Duration::from_millis(200);
    let out = run(i).unwrap();
    assert!(failed(&out).contains("interrompu"), "{}", failed(&out));
}

#[test]
fn ef_scr_01_an_empty_script_changes_nothing() {
    let out = pre("  \n ");
    assert_eq!(out.error, None);
    assert_eq!(out.request, input(Phase::Pre, "").request);
    assert!(out.results.is_empty() && !out.dirty.runtime);
}

#[test]
fn ef_scr_03_embedded_libraries_are_reachable_by_global_and_by_require() {
    let out = pre("
        const CryptoJS = require('crypto-js');
        const { v4, validate } = require('uuid');
        const { nanoid } = require('nanoid');
        bru.setVar('seen', [
            CryptoJS.SHA256('abc').toString(),
            CryptoJS.MD5('abc').toString(),
            CryptoJS.HmacSHA256('msg', 'key').toString(CryptoJS.enc.Hex),
            CryptoJS.enc.Base64.stringify(CryptoJS.enc.Utf8.parse('hello')),
            btoa('hello'), atob('aGVsbG8='),
            moment('2026-10-05T12:00:00Z').utc().format('YYYY/MM/DD'),
            Buffer.from('hello').toString('base64'), Buffer.from('68656c6c6f', 'hex').toString(),
            validate(v4()), nanoid().length, crypto.randomBytes(8).length,
            uuid === require('uuid'), tv4.validate({ a: 1 }, { type: 'object' }),
        ]);
    ");
    assert_eq!(out.error, None);
    assert_eq!(
        out.vars.runtime["seen"],
        json!([
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            "900150983cd24fb0d6963f7d28e17f72",
            "2d93cbc1be167bcb1637a4a23cbff01a7878f0c50ee833954ea5221bb1b8c628",
            "aGVsbG8=",
            "aGVsbG8=",
            "hello",
            "2026/10/05",
            "aGVsbG8=",
            "hello",
            true,
            21,
            8,
            true,
            true
        ])
    );
}

#[test]
fn ef_scr_03_local_modules_resolve_inside_the_collection_only() {
    let dir = std::env::temp_dir().join(format!("xc-script-modules-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("lib")).unwrap();
    std::fs::write(
        dir.join("lib/util.js"),
        "const base = require('./base'); module.exports = { twice: (n) => base.one(n) * 2 };",
    )
    .unwrap();
    std::fs::write(dir.join("lib/base.js"), "module.exports = { one: (n) => n + bru.getVar('offset') };").unwrap();
    let outside = std::env::temp_dir().join(format!("xc-script-outside-{}.js", std::process::id()));
    std::fs::write(&outside, "module.exports = 1").unwrap();

    let script = format!(
        "bru.setVar('offset', 1);
         const util = require('./lib/util');
         let refused, missing;
         try {{ require('../{}'); }} catch (e) {{ refused = e.message; }}
         try {{ require('./lib/none'); }} catch (e) {{ missing = e.message; }}
         bru.setVar('seen', [util.twice(4), require('./lib/util') === util, refused, missing]);",
        outside.file_name().unwrap().to_string_lossy()
    );
    let mut i = input(Phase::Pre, &script);
    i.collection_path = dir.to_string_lossy().into_owned();
    let out = run(i).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_file(&outside).ok();
    assert_eq!(out.error, None);
    assert_eq!(
        out.vars.runtime["seen"],
        json!([
            10,
            true,
            "Access to files outside of the collectionPath is not allowed.",
            "Cannot find module ./lib/none"
        ])
    );
}
