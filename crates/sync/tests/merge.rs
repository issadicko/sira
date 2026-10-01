//! EF-SYN-04 : le moteur de fusion à 3 voies applique, champ par champ, les règles de `docs/docs/synchro-openapi.md`
//! § 5. Chaque champ a ses quatre issues : aucun changement, spec seule, équipe seule, conflit.

use xc_core::yaml::{self, Value};
use xc_core::{Auth, Body, KeyValue, MultipartField, MultipartValue, Param, ParamKind, RequestDoc};
use xc_sync::merge::{merge, Change, Choice, Choices, Decision, Kind, MergeError, Merged};

const BASE: &str =
    "info:\n  name: Op\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/pets/:id\"\n";

fn parse(text: &str) -> RequestDoc {
    match yaml::parse(text).unwrap() {
        Value::Map(tree) => RequestDoc::from_tree(&tree),
        _ => panic!("table attendue"),
    }
}

fn doc() -> RequestDoc {
    parse(BASE)
}

fn edit(doc: &RequestDoc, change: impl FnOnce(&mut RequestDoc)) -> RequestDoc {
    let mut copy = doc.clone();
    change(&mut copy);
    copy
}

fn kv(name: &str, value: &str) -> KeyValue {
    KeyValue { name: name.into(), value: value.into(), enabled: true, description: None }
}

fn param(name: &str, value: &str, kind: ParamKind) -> Param {
    Param { name: name.into(), value: value.into(), kind, enabled: true, description: None }
}

fn query(name: &str, value: &str) -> Param {
    param(name, value, ParamKind::Query)
}

fn run(base: Option<&RequestDoc>, ours: &RequestDoc, theirs: &RequestDoc) -> Merged {
    merge("op", base, ours, theirs, &Choices::new()).unwrap()
}

fn choose(
    base: Option<&RequestDoc>,
    ours: &RequestDoc,
    theirs: &RequestDoc,
    id: &str,
    choice: Choice,
    value: Option<&str>,
) -> Result<Merged, MergeError> {
    let decision = Decision { choice, value: value.map(str::to_owned) };
    merge("op", base, ours, theirs, &Choices::from([(format!("op::{id}"), decision)]))
}

fn change<'a>(merged: &'a Merged, suffix: &str) -> &'a Change {
    let id = format!("op::{suffix}");
    merged
        .changes
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("changement {id} absent : {:?}", merged.changes))
}

fn kinds(merged: &Merged) -> Vec<(&str, Kind)> {
    merged.changes.iter().map(|c| (c.id.strip_prefix("op::").unwrap(), c.kind)).collect()
}

#[test]
fn ef_syn_04_identical_versions_change_nothing() {
    let mut full = doc();
    full.headers = vec![kv("Accept", "application/json")];
    full.params = vec![query("limit", "10"), param("id", "", ParamKind::Path)];
    full.body = Body::Json { data: "{\n  \"a\": 1\n}".into() };
    full.auth = Auth::Bearer { token: "{{token}}".into() };
    let merged = run(Some(&full), &full, &full);
    assert!(merged.changes.is_empty(), "{:?}", merged.changes);
    assert_eq!(merged.doc, full);
    let merged = run(None, &full, &full);
    assert!(merged.changes.is_empty(), "sans base non plus : {:?}", merged.changes);
}

#[test]
fn ef_syn_04_method_follows_the_three_way_rule() {
    let base = doc();
    let with = |method: &str| edit(&base, |d| d.method = method.into());

    let merged = run(Some(&base), &base, &base);
    assert!(merged.changes.is_empty());

    let merged = run(Some(&base), &base, &with("POST"));
    assert_eq!(change(&merged, "method").kind, Kind::Applied);
    assert_eq!(merged.doc.method, "POST");
    assert_eq!(change(&merged, "method").result.as_deref(), Some("POST"));

    let merged = run(Some(&base), &with("PUT"), &base);
    assert_eq!(change(&merged, "method").kind, Kind::Kept);
    assert_eq!(merged.doc.method, "PUT");

    let merged = run(Some(&base), &with("PUT"), &with("PUT"));
    assert_eq!(change(&merged, "method").kind, Kind::Same);
    assert_eq!(merged.doc.method, "PUT");

    let (ours, theirs) = (with("PUT"), with("POST"));
    let merged = run(Some(&base), &ours, &theirs);
    let conflict = change(&merged, "method");
    assert_eq!((conflict.kind, conflict.result.clone()), (Kind::Conflict, None));
    assert_eq!(conflict.choices, [Choice::Team, Choice::Spec]);
    assert_eq!(
        (conflict.base.as_deref(), conflict.ours.as_deref(), conflict.theirs.as_deref()),
        (Some("GET"), Some("PUT"), Some("POST"))
    );
    assert_eq!(merged.doc.method, "PUT", "la valeur de l'équipe reste en place");
    assert_eq!(merged.unresolved().count(), 1);

    let spec = choose(Some(&base), &ours, &theirs, "method", Choice::Spec, None).unwrap();
    assert_eq!((spec.doc.method.as_str(), spec.unresolved().count()), ("POST", 0));
    assert_eq!(change(&spec, "method").result.as_deref(), Some("POST"));
    let team = choose(Some(&base), &ours, &theirs, "method", Choice::Team, None).unwrap();
    assert_eq!((team.doc.method.as_str(), team.unresolved().count()), ("PUT", 0));
    let error = choose(Some(&base), &ours, &theirs, "method", Choice::Edit, Some("PATCH")).unwrap_err();
    assert!(matches!(error, MergeError::Unavailable { .. }), "{error}");
}

#[test]
fn ef_syn_04_address_follows_the_three_way_rule_and_keeps_the_query_bytes() {
    let base = doc();
    let with = |url: &str| edit(&base, |d| d.url = url.into());
    let renamed = "{{baseUrl}}/animals/:id";

    let merged = run(Some(&base), &base, &base);
    assert!(merged.changes.is_empty());

    let merged = run(Some(&base), &base, &with(renamed));
    assert_eq!(change(&merged, "url").kind, Kind::Applied);
    assert_eq!(merged.doc.url, renamed);

    let merged = run(Some(&base), &with("{{host}}/pets/:id"), &base);
    assert_eq!(change(&merged, "url").kind, Kind::Kept);
    assert_eq!(merged.doc.url, "{{host}}/pets/:id");

    let merged = run(Some(&base), &with(renamed), &with(renamed));
    assert_eq!(change(&merged, "url").kind, Kind::Same);

    let (ours, theirs) = (with("{{host}}/pets/:id"), with(renamed));
    let merged = run(Some(&base), &ours, &theirs);
    assert_eq!(change(&merged, "url").kind, Kind::Conflict);
    assert_eq!(change(&merged, "url").choices, [Choice::Team, Choice::Spec, Choice::Edit]);
    assert_eq!(merged.doc.url, "{{host}}/pets/:id");
    let edited =
        choose(Some(&base), &ours, &theirs, "url", Choice::Edit, Some("{{baseUrl}}/pets-v2/:id?ignored=1")).unwrap();
    assert_eq!(edited.doc.url, "{{baseUrl}}/pets-v2/:id", "la valeur saisie est une adresse, sans query");
    let missing = choose(Some(&base), &ours, &theirs, "url", Choice::Edit, None).unwrap_err();
    assert!(matches!(missing, MergeError::MissingValue { .. }), "{missing}");

    let team_query = edit(&base, |d| d.url = "{{baseUrl}}/pets/:id?b=2&a=1".into());
    let merged = run(Some(&base), &team_query, &with(renamed));
    assert_eq!(merged.doc.url, "{{baseUrl}}/animals/:id?b=2&a=1", "la query de l'équipe est conservée à l'octet");
}

#[test]
fn ef_syn_04_query_urls_are_rebuilt_only_when_the_query_params_change() {
    let mut base = doc();
    base.url = "{{baseUrl}}/pets".into();
    base.params = vec![query("limit", "10")];
    let theirs = edit(&base, |d| d.params = vec![query("limit", "10"), query("sort", "")]);
    let ours = edit(&base, |d| d.url = "{{baseUrl}}/pets?limit=10".into());
    let merged = run(Some(&base), &ours, &theirs);
    assert_eq!(merged.doc.url, "{{baseUrl}}/pets?limit=10&sort");
    assert_eq!(change(&merged, "param/query/sort").kind, Kind::Applied);

    let unchanged = edit(&base, |d| d.params = vec![query("limit", "10")]);
    let ours = edit(&unchanged, |d| d.url = "{{baseUrl}}/pets?limit=10&junk".into());
    let merged = run(Some(&base), &ours, &unchanged);
    assert_eq!(merged.doc.url, "{{baseUrl}}/pets?limit=10&junk", "l'URL de l'équipe est gardée à l'octet");

    let disabled = Param { enabled: false, ..query("trace", "1") };
    let theirs = edit(&base, |d| d.params = vec![query("limit", "10"), disabled.clone()]);
    let merged = run(Some(&base), &base, &theirs);
    assert_eq!(merged.doc.url, "{{baseUrl}}/pets", "un paramètre désactivé n'est pas dans l'URL");
    assert_eq!(merged.doc.params.len(), 2);
}

#[test]
fn ef_syn_04_query_param_keeps_the_team_value_and_follows_the_spec_description() {
    let mut base = doc();
    base.params = vec![Param { description: Some("max".into()), ..query("limit", "10") }];
    let described = |value: &str, description: Option<&str>, enabled: bool| {
        vec![Param { description: description.map(str::to_owned), enabled, ..query("limit", value) }]
    };

    let merged = run(Some(&base), &base, &base);
    assert!(merged.changes.is_empty());

    let theirs = edit(&base, |d| d.params = described("25", Some("maximum"), true));
    let merged = run(Some(&base), &base, &theirs);
    assert_eq!(change(&merged, "param/query/limit").kind, Kind::Applied, "valeur et description de la spec");
    assert_eq!(merged.doc.params, described("25", Some("maximum"), true));

    let ours = edit(&base, |d| d.params = described("50", Some("max"), true));
    let merged = run(Some(&base), &ours, &theirs);
    assert_eq!(change(&merged, "param/query/limit").kind, Kind::Merged, "valeur de l'équipe, description de la spec");
    assert_eq!(merged.doc.params, described("50", Some("maximum"), true));

    let merged = run(Some(&base), &ours, &base);
    assert_eq!(change(&merged, "param/query/limit").kind, Kind::Kept);
    assert_eq!(merged.doc.params, described("50", Some("max"), true));

    let disabled = edit(&base, |d| d.params = described("10", Some("max"), false));
    let merged = run(Some(&base), &disabled, &theirs);
    assert_eq!(merged.doc.params, described("10", Some("maximum"), false), "l'activation est celle de l'équipe");

    let (team, spec) = (
        edit(&base, |d| d.params = described("10", Some("limite"), true)),
        edit(&base, |d| d.params = described("10", Some("maximum"), true)),
    );
    let merged = run(Some(&base), &team, &spec);
    let conflict = change(&merged, "param/query/limit");
    assert_eq!(conflict.kind, Kind::Conflict);
    assert_eq!(conflict.choices, [Choice::Team, Choice::Spec]);
    assert_eq!(merged.doc.params, described("10", Some("limite"), true));
    let taken = choose(Some(&base), &team, &spec, "param/query/limit", Choice::Spec, None).unwrap();
    assert_eq!(taken.doc.params, described("10", Some("maximum"), true));
}

#[test]
fn ef_syn_04_path_params_follow_the_merged_address_and_keep_team_values() {
    let mut base = doc();
    base.params = vec![param("id", "", ParamKind::Path)];

    let merged = run(Some(&base), &base, &base);
    assert!(merged.changes.is_empty());

    let spec_described = edit(&base, |d| {
        d.params = vec![Param { description: Some("identifiant".into()), ..param("id", "", ParamKind::Path) }]
    });
    let merged = run(Some(&base), &base, &spec_described);
    assert_eq!(change(&merged, "param/path/id").kind, Kind::Applied);

    let team_value = edit(&base, |d| d.params = vec![param("id", "42", ParamKind::Path)]);
    let merged = run(Some(&base), &team_value, &base);
    assert_eq!(change(&merged, "param/path/id").kind, Kind::Kept);
    assert_eq!(merged.doc.params[0].value, "42");

    let renamed = edit(&base, |d| {
        d.url = "{{baseUrl}}/pets/:petId".into();
        d.params = vec![param("petId", "", ParamKind::Path)];
    });
    let merged = run(Some(&base), &team_value, &renamed);
    assert_eq!(merged.doc.url, "{{baseUrl}}/pets/:petId");
    assert_eq!(
        merged.doc.params,
        vec![param("petId", "42", ParamKind::Path)],
        "le paramètre suit le segment renommé avec sa valeur"
    );
    assert_eq!(change(&merged, "param/path/petId").kind, Kind::Kept);
    assert_eq!(merged.unresolved().count(), 0);

    let team_address = edit(&team_value, |d| d.url = "{{host}}/pets/:id".into());
    let merged = run(Some(&base), &team_address, &renamed);
    assert_eq!(change(&merged, "url").kind, Kind::Conflict);
    assert_eq!(
        merged.doc.params,
        vec![param("id", "42", ParamKind::Path)],
        "l'adresse de l'équipe garde son paramètre"
    );
    let spec = choose(Some(&base), &team_address, &renamed, "url", Choice::Spec, None).unwrap();
    assert_eq!(spec.doc.params, vec![param("petId", "42", ParamKind::Path)], "la valeur suit le segment renommé");

    let (team, spec) = (
        edit(&base, |d| d.params = vec![Param { description: Some("a".into()), ..param("id", "", ParamKind::Path) }]),
        edit(&base, |d| d.params = vec![Param { description: Some("b".into()), ..param("id", "", ParamKind::Path) }]),
    );
    assert_eq!(change(&run(Some(&base), &team, &spec), "param/path/id").kind, Kind::Conflict);
}

#[test]
fn ef_syn_04_header_follows_the_keyed_rules_without_case() {
    let mut base = doc();
    base.headers = vec![kv("Accept", "application/json")];

    let merged = run(Some(&base), &base, &base);
    assert!(merged.changes.is_empty());

    let theirs = edit(&base, |d| d.headers = vec![kv("accept", "application/xml")]);
    let merged = run(Some(&base), &base, &theirs);
    assert_eq!(change(&merged, "header/accept").kind, Kind::Applied);
    assert_eq!(merged.doc.headers[0].value, "application/xml");
    assert_eq!(merged.doc.headers[0].name, "Accept", "le nom de l'équipe est conservé");

    let ours = edit(&base, |d| d.headers = vec![kv("Accept", "text/plain")]);
    let merged = run(Some(&base), &ours, &base);
    assert_eq!(change(&merged, "header/accept").kind, Kind::Kept);
    assert_eq!(merged.doc.headers[0].value, "text/plain");

    let merged = run(Some(&base), &ours, &theirs);
    assert_eq!(change(&merged, "header/accept").kind, Kind::Kept, "la valeur de l'équipe gagne, jamais de conflit");
    assert_eq!(merged.doc.headers[0].value, "text/plain");
    assert_eq!(merged.unresolved().count(), 0);

    let (team, spec) = (
        edit(&base, |d| {
            d.headers = vec![KeyValue { description: Some("a".into()), ..kv("Accept", "application/json") }]
        }),
        edit(&base, |d| {
            d.headers = vec![KeyValue { description: Some("b".into()), ..kv("Accept", "application/json") }]
        }),
    );
    let merged = run(Some(&base), &team, &spec);
    assert_eq!(change(&merged, "header/accept").kind, Kind::Conflict);
    assert_eq!(merged.doc.headers[0].description.as_deref(), Some("a"));
}

#[test]
fn ef_syn_04_keyed_collections_handle_every_addition_and_removal() {
    let mut base = doc();
    base.headers = vec![kv("A", "1"), kv("B", "2")];
    let with = |headers: Vec<KeyValue>| edit(&base, |d| d.headers = headers);

    let theirs = with(vec![kv("A", "1"), kv("B", "2"), kv("C", "3")]);
    let merged = run(Some(&base), &base, &theirs);
    assert_eq!(kinds(&merged), [("header/c", Kind::Applied)]);
    assert_eq!(merged.doc.headers.last().unwrap().name, "C", "ajouté à la fin, dans l'ordre de la spec");

    let ours = with(vec![kv("A", "1"), kv("B", "2"), kv("X", "9")]);
    let merged = run(Some(&base), &ours, &base);
    assert!(merged.changes.is_empty(), "un en-tête ajouté par l'équipe n'est jamais touché");
    let merged = run(Some(&base), &ours, &theirs);
    assert_eq!(merged.doc.headers.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(), ["A", "B", "X", "C"]);

    let both = with(vec![kv("A", "1"), kv("B", "2"), kv("C", "other")]);
    let merged = run(Some(&base), &both, &theirs);
    assert_eq!(change(&merged, "header/c").kind, Kind::Kept);
    assert_eq!(merged.doc.headers[2].value, "other");
    let merged = run(Some(&base), &theirs, &theirs);
    assert_eq!(change(&merged, "header/c").kind, Kind::Same);

    let retired = with(vec![kv("A", "1")]);
    let merged = run(Some(&base), &base, &retired);
    assert_eq!(change(&merged, "header/b").kind, Kind::Applied);
    assert_eq!(merged.doc.headers, vec![kv("A", "1")]);
    let merged = run(Some(&base), &retired, &retired);
    assert_eq!(change(&merged, "header/b").kind, Kind::Same);

    let edited = with(vec![kv("A", "1"), kv("B", "changed")]);
    let merged = run(Some(&base), &edited, &retired);
    let conflict = change(&merged, "header/b");
    assert_eq!(
        (conflict.kind, conflict.choices.clone()),
        (Kind::Conflict, vec![Choice::Team, Choice::Spec, Choice::Edit])
    );
    assert_eq!(merged.doc.headers[1].value, "changed", "retrait par la spec contre modification : l'équipe reste");
    let spec = choose(Some(&base), &edited, &retired, "header/b", Choice::Spec, None).unwrap();
    assert_eq!(spec.doc.headers, vec![kv("A", "1")]);
    let typed = choose(Some(&base), &edited, &retired, "header/b", Choice::Edit, Some("typed")).unwrap();
    assert_eq!(typed.doc.headers[1].value, "typed");

    let removed_by_team = with(vec![kv("A", "1")]);
    let merged = run(Some(&base), &removed_by_team, &base);
    assert_eq!(change(&merged, "header/b").kind, Kind::Kept);
    assert_eq!(merged.doc.headers, vec![kv("A", "1")], "reste retiré quand la spec n'y a pas touché");

    let changed_by_spec = with(vec![kv("A", "1"), kv("B", "new")]);
    let merged = run(Some(&base), &removed_by_team, &changed_by_spec);
    let conflict = change(&merged, "header/b");
    assert_eq!(conflict.kind, Kind::Conflict);
    assert_eq!(merged.doc.headers, vec![kv("A", "1")]);
    let spec = choose(Some(&base), &removed_by_team, &changed_by_spec, "header/b", Choice::Spec, None).unwrap();
    assert_eq!(spec.doc.headers, vec![kv("A", "1"), kv("B", "new")]);
    let team = choose(Some(&base), &removed_by_team, &changed_by_spec, "header/b", Choice::Team, None).unwrap();
    assert_eq!(team.doc.headers, vec![kv("A", "1")]);
}

#[test]
fn ef_syn_04_without_a_base_the_team_is_kept_and_the_spec_adds() {
    let mut ours = doc();
    ours.headers = vec![kv("A", "team"), kv("Mine", "1")];
    ours.params = vec![Param { description: Some("a".into()), ..query("limit", "5") }];
    let mut theirs = doc();
    theirs.headers = vec![kv("A", "spec"), kv("Theirs", "2")];
    theirs.params = vec![Param { description: Some("b".into()), ..query("limit", "10") }, query("page", "1")];
    let merged = run(None, &ours, &theirs);
    assert_eq!(
        kinds(&merged),
        [
            ("header/a", Kind::Kept),
            ("header/theirs", Kind::Applied),
            ("param/query/limit", Kind::Kept),
            ("param/query/page", Kind::Applied)
        ]
    );
    assert_eq!(
        merged.doc.headers.iter().map(|h| (h.name.as_str(), h.value.as_str())).collect::<Vec<_>>(),
        [("A", "team"), ("Mine", "1"), ("Theirs", "2")]
    );
    assert_eq!(merged.doc.params[0], Param { description: Some("a".into()), ..query("limit", "5") });
    assert_eq!(merged.doc.params[1], query("page", "1"));
    assert_eq!(merged.doc.url, "{{baseUrl}}/pets/:id?limit=5&page=1");
    assert_eq!(merged.unresolved().count(), 0);

    let different = edit(&theirs, |d| {
        d.method = "POST".into();
        d.auth = Auth::Bearer { token: "t".into() };
        d.body = Body::Text { data: "x".into() };
    });
    let merged = run(None, &ours, &different);
    for id in ["method", "auth", "body"] {
        let conflict = change(&merged, id);
        assert_eq!((conflict.kind, conflict.base.clone()), (Kind::Conflict, None), "{id}");
        assert!(conflict.reason.contains("base"), "{}", conflict.reason);
    }
    assert!(!change(&merged, "body").choices.contains(&Choice::Both));
}

#[test]
fn ef_syn_04_text_body_follows_the_three_way_rule() {
    let mut base = doc();
    base.body = Body::Text { data: "one".into() };
    let with = |data: &str| edit(&base, |d| d.body = Body::Text { data: data.into() });

    let merged = run(Some(&base), &base, &base);
    assert!(merged.changes.is_empty());

    let merged = run(Some(&base), &base, &with("two"));
    assert_eq!(change(&merged, "body").kind, Kind::Applied);
    assert_eq!(merged.doc.body, Body::Text { data: "two".into() });

    let merged = run(Some(&base), &with("mine"), &base);
    assert_eq!(change(&merged, "body").kind, Kind::Kept);
    assert_eq!(merged.doc.body, Body::Text { data: "mine".into() });

    let merged = run(Some(&base), &with("same"), &with("same"));
    assert_eq!(change(&merged, "body").kind, Kind::Same);

    let (ours, theirs) = (with("mine"), with("two"));
    let merged = run(Some(&base), &ours, &theirs);
    let conflict = change(&merged, "body");
    assert_eq!(conflict.kind, Kind::Conflict);
    assert_eq!(conflict.choices, [Choice::Team, Choice::Spec, Choice::Edit], "pas de fusion JSON pour du texte");
    assert_eq!(merged.doc.body, Body::Text { data: "mine".into() });
    let typed = choose(Some(&base), &ours, &theirs, "body", Choice::Edit, Some("typed")).unwrap();
    assert_eq!(typed.doc.body, Body::Text { data: "typed".into() });
    let spec = choose(Some(&base), &ours, &theirs, "body", Choice::Spec, None).unwrap();
    assert_eq!(spec.doc.body, Body::Text { data: "two".into() });

    let other_type = edit(&base, |d| d.body = Body::FormUrlEncoded { fields: vec![kv("a", "1")] });
    let merged = run(Some(&base), &ours, &other_type);
    assert_eq!(change(&merged, "body").choices, [Choice::Team, Choice::Spec], "pas d'édition d'un contenu non textuel");
}

fn json_body(data: &str) -> Body {
    Body::Json { data: data.into() }
}

#[test]
fn ef_syn_04_json_body_merges_key_by_key_and_masks_variables() {
    let mut base = doc();
    base.body = json_body("{\n  \"id\": {{petId}},\n  \"name\": \"\",\n  \"tag\": \"x\"\n}");
    let with = |data: &str| edit(&base, |d| d.body = json_body(data));
    let ours = with("{\n  \"id\": {{petId}},\n  \"name\": \"Rex\",\n  \"tag\": \"x\"\n}");
    let theirs = with("{\n  \"id\": {{petId}},\n  \"name\": \"\",\n  \"tag\": \"x\",\n  \"age\": {{age}}\n}");

    let merged = run(Some(&base), &ours, &theirs);
    let body = change(&merged, "body");
    assert_eq!(body.kind, Kind::Merged);
    assert!(body.choices.is_empty());
    assert_eq!(
        merged.doc.body,
        json_body("{\n  \"id\": {{petId}},\n  \"name\": \"Rex\",\n  \"tag\": \"x\",\n  \"age\": {{age}}\n}")
    );
    assert_eq!(
        body.result.as_deref(),
        Some("{\n  \"id\": {{petId}},\n  \"name\": \"Rex\",\n  \"tag\": \"x\",\n  \"age\": {{age}}\n}")
    );

    let reformatted = with("{\"id\":{{petId}},\"name\":\"\",\"tag\":\"x\"}");
    let merged = run(Some(&base), &reformatted, &theirs);
    assert_eq!(change(&merged, "body").kind, Kind::Merged, "un corps reformaté sans changement de fond prend la spec");
    assert_eq!(
        merged.doc.body,
        json_body("{\n  \"id\": {{petId}},\n  \"name\": \"\",\n  \"tag\": \"x\",\n  \"age\": {{age}}\n}")
    );

    let in_string = with("{\"url\": \"{{base}}/a\"}");
    let both_changed = edit(&base, |d| d.body = json_body("{\"url\": \"{{base}}/b\", \"n\": 1}"));
    let merged = run(
        Some(&in_string),
        &both_changed,
        &edit(&base, |d| d.body = json_body("{\"url\": \"{{base}}/a\", \"m\": 2}")),
    );
    assert_eq!(
        merged.doc.body,
        json_body("{\n  \"url\": \"{{base}}/b\",\n  \"n\": 1,\n  \"m\": 2\n}"),
        "{{}} dans une chaîne reste du texte"
    );
}

#[test]
fn ef_syn_04_json_body_conflicts_offer_the_four_choices() {
    let mut base = doc();
    base.body = json_body("{\n  \"name\": \"\",\n  \"tag\": \"x\",\n  \"owner\": {\n    \"id\": 0\n  }\n}");
    let with = |data: &str| edit(&base, |d| d.body = json_body(data));
    let ours = with("{\"name\":\"Rex\",\"tag\":\"x\",\"owner\":{\"id\":7}}");
    let theirs = with("{\"name\":\"Felix\",\"tag\":\"x\",\"owner\":{\"id\":0,\"kind\":\"\"},\"age\":0}");

    let merged = run(Some(&base), &ours, &theirs);
    let conflict = change(&merged, "body");
    assert_eq!(conflict.kind, Kind::Conflict);
    assert_eq!(conflict.choices, [Choice::Team, Choice::Spec, Choice::Both, Choice::Edit]);
    assert_eq!(merged.doc.body, ours.body, "tant que le conflit n'est pas arbitré, le corps de l'équipe reste");

    let both = choose(Some(&base), &ours, &theirs, "body", Choice::Both, None).unwrap();
    assert_eq!(
        both.doc.body,
        json_body("{\n  \"name\": \"Rex\",\n  \"tag\": \"x\",\n  \"owner\": {\n    \"id\": 7,\n    \"kind\": \"\"\n  },\n  \"age\": 0\n}"),
        "l'équipe gagne sur les chemins en conflit, les ajouts de la spec sont repris"
    );
    let spec = choose(Some(&base), &ours, &theirs, "body", Choice::Spec, None).unwrap();
    assert_eq!(spec.doc.body, theirs.body);
    let typed = choose(Some(&base), &ours, &theirs, "body", Choice::Edit, Some("{}")).unwrap();
    assert_eq!(typed.doc.body, json_body("{}"));

    let retired = with("{\"name\":\"\",\"owner\":{\"id\":0}}");
    let tag_edited = with("{\"name\":\"\",\"tag\":\"mine\",\"owner\":{\"id\":0}}");
    let merged = run(Some(&base), &tag_edited, &retired);
    assert_eq!(change(&merged, "body").kind, Kind::Conflict, "clé retirée d'un côté et modifiée de l'autre");
    let both = choose(Some(&base), &tag_edited, &retired, "body", Choice::Both, None).unwrap();
    assert_eq!(
        both.doc.body,
        json_body("{\n  \"name\": \"\",\n  \"tag\": \"mine\",\n  \"owner\": {\n    \"id\": 0\n  }\n}")
    );

    let untouched = with("{\"name\":\"Rex\",\"tag\":\"x\",\"owner\":{\"id\":0}}");
    let merged = run(Some(&base), &untouched, &retired);
    assert_eq!(change(&merged, "body").kind, Kind::Merged);
    assert_eq!(merged.doc.body, json_body("{\n  \"name\": \"Rex\",\n  \"owner\": {\n    \"id\": 0\n  }\n}"));

    let arrays_base = edit(&base, |d| d.body = json_body("{\"tags\":[\"a\"]}"));
    let (ours, theirs) = (
        edit(&base, |d| d.body = json_body("{\"tags\":[\"a\",\"b\"]}")),
        edit(&base, |d| d.body = json_body("{\"tags\":[\"a\",\"c\"]}")),
    );
    let merged = run(Some(&arrays_base), &ours, &theirs);
    assert_eq!(change(&merged, "body").kind, Kind::Conflict, "les tableaux sont fusionnés comme des valeurs");

    let broken = edit(&base, |d| d.body = json_body("not json {{"));
    let merged = run(Some(&base), &broken, &theirs);
    assert_eq!(change(&merged, "body").choices, [Choice::Team, Choice::Spec, Choice::Edit]);
}

fn field(name: &str, value: &str) -> MultipartField {
    MultipartField {
        name: name.into(),
        value: MultipartValue::Text(value.into()),
        enabled: true,
        content_type: None,
        description: None,
    }
}

#[test]
fn ef_syn_04_form_bodies_merge_by_field_name() {
    let mut base = doc();
    base.body = Body::FormUrlEncoded { fields: vec![kv("user", ""), kv("pass", "")] };
    let with = |fields: Vec<KeyValue>| edit(&base, |d| d.body = Body::FormUrlEncoded { fields });
    let fields = |merged: &Merged| match &merged.doc.body {
        Body::FormUrlEncoded { fields } => fields.iter().map(|f| format!("{}={}", f.name, f.value)).collect::<Vec<_>>(),
        other => panic!("{other:?}"),
    };

    let merged = run(Some(&base), &base, &base);
    assert!(merged.changes.is_empty());

    let merged = run(Some(&base), &base, &with(vec![kv("user", ""), kv("pass", ""), kv("otp", "")]));
    assert_eq!(kinds(&merged), [("body/form/otp", Kind::Applied)]);
    assert_eq!(fields(&merged), ["user=", "pass=", "otp="]);

    let merged = run(Some(&base), &with(vec![kv("user", "ada"), kv("pass", "")]), &base);
    assert_eq!(kinds(&merged), [("body/form/user", Kind::Kept)]);
    assert_eq!(fields(&merged), ["user=ada", "pass="]);

    let merged = run(
        Some(&base),
        &with(vec![kv("user", "ada"), kv("pass", ""), kv("mine", "1")]),
        &with(vec![kv("user", ""), kv("pass", ""), kv("otp", "")]),
    );
    assert_eq!(fields(&merged), ["user=ada", "pass=", "mine=1", "otp="]);

    let (retired, modified) = (with(vec![kv("user", "")]), with(vec![kv("user", ""), kv("pass", "secret")]));
    let merged = run(Some(&base), &modified, &retired);
    assert_eq!(change(&merged, "body/form/pass").kind, Kind::Conflict);
    assert_eq!(fields(&merged), ["user=", "pass=secret"]);
    let spec = choose(Some(&base), &modified, &retired, "body/form/pass", Choice::Spec, None).unwrap();
    assert_eq!(fields(&spec), ["user="]);
    let merged = run(Some(&base), &retired, &base);
    assert_eq!(change(&merged, "body/form/pass").kind, Kind::Kept);

    let mut multipart = doc();
    multipart.body = Body::MultipartForm { fields: vec![field("file", ""), field("title", "")] };
    let theirs = edit(&multipart, |d| {
        d.body = Body::MultipartForm { fields: vec![field("file", ""), field("title", ""), field("lang", "fr")] }
    });
    let ours =
        edit(&multipart, |d| d.body = Body::MultipartForm { fields: vec![field("file", ""), field("title", "mine")] });
    let merged = run(Some(&multipart), &ours, &theirs);
    assert_eq!(kinds(&merged), [("body/form/title", Kind::Kept), ("body/form/lang", Kind::Applied)]);
    match &merged.doc.body {
        Body::MultipartForm { fields } => {
            assert_eq!(fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["file", "title", "lang"]);
            assert_eq!(fields[1].value, MultipartValue::Text("mine".into()));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ef_syn_04_auth_follows_the_three_way_rule_on_the_whole_object() {
    let mut base = doc();
    base.auth = Auth::Bearer { token: "{{token}}".into() };
    let with = |auth: Auth| edit(&base, |d| d.auth = auth);

    let merged = run(Some(&base), &base, &base);
    assert!(merged.changes.is_empty());

    let merged = run(Some(&base), &base, &with(Auth::Inherit));
    assert_eq!(change(&merged, "auth").kind, Kind::Applied);
    assert_eq!(merged.doc.auth, Auth::Inherit);

    let basic = Auth::Basic { username: "ada".into(), password: "pw".into() };
    let merged = run(Some(&base), &with(basic.clone()), &base);
    assert_eq!(change(&merged, "auth").kind, Kind::Kept);
    assert_eq!(merged.doc.auth, basic);

    let merged = run(Some(&base), &with(basic.clone()), &with(basic.clone()));
    assert_eq!(change(&merged, "auth").kind, Kind::Same);

    let (ours, theirs) = (with(basic.clone()), with(Auth::None));
    let merged = run(Some(&base), &ours, &theirs);
    let conflict = change(&merged, "auth");
    assert_eq!((conflict.kind, conflict.choices.clone()), (Kind::Conflict, vec![Choice::Team, Choice::Spec]));
    assert_eq!(conflict.ours.as_deref(), Some("basic : ada"), "le mot de passe n'est pas affiché");
    assert_eq!(merged.doc.auth, basic, "l'auth de la spec est proposée, jamais imposée");
    let spec = choose(Some(&base), &ours, &theirs, "auth", Choice::Spec, None).unwrap();
    assert_eq!(spec.doc.auth, Auth::None);
}

#[test]
fn ef_syn_04_fields_owned_by_the_team_are_never_touched() {
    let base = doc();
    let mut ours = doc();
    ours.name = "Mon nom".into();
    ours.seq = Some(9);
    ours.docs = Some("mes notes".into());
    ours.timeout_ms = Some(500);
    let mut theirs = doc();
    theirs.name = "Nom de la spec".into();
    theirs.seq = Some(2);
    theirs.docs = Some("doc de la spec".into());
    theirs.method = "POST".into();
    let merged = run(Some(&base), &ours, &theirs);
    assert_eq!(kinds(&merged), [("method", Kind::Applied)]);
    assert_eq!(
        (merged.doc.name.as_str(), merged.doc.seq, merged.doc.docs.as_deref()),
        ("Mon nom", Some(9), Some("mes notes"))
    );
    assert_eq!(merged.doc.timeout_ms, Some(500));
}

#[test]
fn ef_syn_04_decisions_on_unknown_changes_are_ignored_and_unavailable_choices_refused() {
    let mut base = doc();
    base.method = "GET".into();
    let ours = edit(&base, |d| d.method = "PUT".into());
    let theirs = edit(&base, |d| d.method = "POST".into());
    let unknown = choose(Some(&base), &ours, &theirs, "header/none", Choice::Spec, None).unwrap();
    assert_eq!(unknown.unresolved().count(), 1);
    let both = choose(Some(&base), &ours, &theirs, "method", Choice::Both, None).unwrap_err();
    assert!(both.to_string().contains("method") && both.to_string().contains("both"), "{both}");
}

fn oauth(client: &str, token_url: &str) -> Auth {
    let config = format!("accessTokenUrl: {token_url}\ncredentials:\n  clientId: {client}\nflow: client_credentials");
    Auth::Other { label: "oauth2".into(), config }
}

#[test]
fn ef_syn_04_an_untyped_auth_is_compared_by_its_whole_configuration_not_by_its_type() {
    let mut base = doc();
    base.auth = oauth("{{id}}", "https://auth.test/token");
    let with = |auth: Auth| edit(&base, |d| d.auth = auth);
    let team = with(oauth("mine", "https://auth.test/token"));
    let spec = with(oauth("{{id}}", "https://auth.test/v2/token"));

    let merged = run(Some(&base), &base, &base);
    assert!(merged.changes.is_empty());

    let merged = run(Some(&base), &team, &base);
    assert_eq!(change(&merged, "auth").kind, Kind::Kept);
    assert_eq!(merged.doc.auth, oauth("mine", "https://auth.test/token"));

    let merged = run(Some(&base), &base, &spec);
    assert_eq!(change(&merged, "auth").kind, Kind::Applied);
    assert_eq!(merged.doc.auth, oauth("{{id}}", "https://auth.test/v2/token"));

    let merged = run(Some(&base), &team, &spec);
    let conflict = change(&merged, "auth");
    assert_eq!((conflict.kind, conflict.choices.clone()), (Kind::Conflict, vec![Choice::Team, Choice::Spec]));
    assert_eq!(merged.doc.auth, oauth("mine", "https://auth.test/token"), "la valeur de l'équipe reste en place");
    assert_eq!(
        conflict.ours.as_deref(),
        Some(
            "oauth2\naccessTokenUrl: https://auth.test/token\ncredentials:\n  clientId: mine\nflow: client_credentials"
        )
    );
    assert!(conflict.theirs.as_deref().is_some_and(|t| t.starts_with("oauth2\n") && t.contains("/v2/token")));
    let chosen = choose(Some(&base), &team, &spec, "auth", Choice::Spec, None).unwrap();
    assert_eq!(chosen.doc.auth, oauth("{{id}}", "https://auth.test/v2/token"));

    let merged = run(None, &team, &spec);
    assert_eq!(change(&merged, "auth").kind, Kind::Conflict, "sans base non plus");
    let merged = run(None, &team, &team);
    assert!(merged.changes.is_empty());

    let digest = with(Auth::Other { label: "digest".into(), config: "password: x\nusername: u".into() });
    assert_eq!(change(&run(Some(&base), &base, &digest), "auth").kind, Kind::Applied, "un autre type de la spec");
}

#[test]
fn ef_syn_04_a_body_of_an_untyped_kind_is_compared_by_its_whole_configuration() {
    let file = |path: &str| Body::Other { label: "file".into(), config: format!("data:\n  - filePath: {path}") };
    let mut base = doc();
    base.body = file("a.bin");
    let with = |body: Body| edit(&base, |d| d.body = body);
    let merged = run(Some(&base), &base, &with(file("b.bin")));
    assert_eq!(change(&merged, "body").kind, Kind::Applied);
    assert_eq!(merged.doc.body, file("b.bin"));
    let merged = run(Some(&base), &with(file("mine.bin")), &with(file("b.bin")));
    assert_eq!(change(&merged, "body").kind, Kind::Conflict);
    assert_eq!(change(&merged, "body").ours.as_deref(), Some("file\ndata:\n  - filePath: mine.bin"));
}

#[test]
fn ef_syn_04_a_path_param_the_spec_drops_with_its_segment_is_a_conflict_when_the_team_typed_its_value() {
    let mut base = doc();
    base.params = vec![param("id", "", ParamKind::Path)];
    let team = edit(&base, |d| d.params = vec![param("id", "42", ParamKind::Path)]);
    let spec = edit(&base, |d| {
        d.url = "{{baseUrl}}/pets/by-id".into();
        d.params = vec![];
    });

    let merged = run(Some(&base), &team, &spec);
    let conflict = change(&merged, "param/path/id");
    assert_eq!((conflict.kind, conflict.choices.clone()), (Kind::Conflict, vec![Choice::Team, Choice::Spec]));
    assert_eq!((conflict.ours.as_deref(), conflict.result.clone()), (Some("42"), None));
    assert_eq!(merged.doc.url, "{{baseUrl}}/pets/by-id");
    assert_eq!(merged.doc.params, vec![param("id", "42", ParamKind::Path)], "sans choix, la valeur reste");
    assert_eq!(merged.unresolved().count(), 1);

    let dropped = choose(Some(&base), &team, &spec, "param/path/id", Choice::Spec, None).unwrap();
    assert!(dropped.doc.params.is_empty() && dropped.unresolved().count() == 0);
    let kept = choose(Some(&base), &team, &spec, "param/path/id", Choice::Team, None).unwrap();
    assert_eq!(kept.doc.params, vec![param("id", "42", ParamKind::Path)]);
    assert_eq!(change(&kept, "param/path/id").result.as_deref(), Some("42"));
    let refused = choose(Some(&base), &team, &spec, "param/path/id", Choice::Edit, Some("1")).unwrap_err();
    assert!(matches!(refused, MergeError::Unavailable { .. }), "{refused}");

    let untouched = run(Some(&base), &base, &spec);
    assert!(untouched.doc.params.is_empty() && untouched.unresolved().count() == 0, "pas de valeur saisie");
    let no_base = choose(None, &team, &spec, "url", Choice::Spec, None).unwrap();
    assert_eq!(change(&no_base, "param/path/id").kind, Kind::Conflict, "sans base non plus");
}

#[test]
fn ef_syn_04_only_a_single_renamed_segment_carries_the_path_param_value() {
    let mut base = doc();
    base.url = "{{baseUrl}}/a/:x/b/:y".into();
    base.params = vec![param("x", "", ParamKind::Path), param("y", "", ParamKind::Path)];
    let team = edit(&base, |d| d.params = vec![param("x", "1", ParamKind::Path), param("y", "2", ParamKind::Path)]);
    let one = edit(&base, |d| {
        d.url = "{{baseUrl}}/a/:x/b/:z".into();
        d.params = vec![param("x", "", ParamKind::Path), param("z", "", ParamKind::Path)];
    });
    let merged = run(Some(&base), &team, &one);
    assert_eq!(merged.doc.params, vec![param("x", "1", ParamKind::Path), param("z", "2", ParamKind::Path)]);
    assert_eq!(merged.unresolved().count(), 0);

    let two = edit(&base, |d| {
        d.url = "{{baseUrl}}/a/:p/b/:q".into();
        d.params = vec![param("p", "", ParamKind::Path), param("q", "", ParamKind::Path)];
    });
    let merged = run(Some(&base), &team, &two);
    let ids: Vec<_> = merged.unresolved().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["op::param/path/x", "op::param/path/y"], "deux segments renommés : chaque valeur est signalée");
    assert_eq!(merged.doc.params.len(), 4, "les valeurs de l'équipe restent tant qu'il n'y a pas de choix");

    let only_y = edit(&base, |d| d.params = vec![param("x", "", ParamKind::Path), param("y", "2", ParamKind::Path)]);
    let merged = run(Some(&base), &only_y, &two);
    assert_eq!(merged.unresolved().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["op::param/path/y"]);
}

fn json_doc(data: &str) -> RequestDoc {
    edit(&doc(), |d| d.body = Body::Json { data: data.into() })
}

fn merged_json(merged: &Merged) -> &str {
    match &merged.doc.body {
        Body::Json { data } => data,
        other => panic!("{other:?}"),
    }
}

#[test]
fn ef_syn_04_json_numbers_keep_the_token_the_team_wrote() {
    let base = json_doc("{\"a\": 1}");
    let team = json_doc("{\"a\": 1, \"id\": 12345678901234567890123, \"qty\": 1.50, \"big\": 1E3, \"neg\": -0.0e-5, \"price\": 0.30000000000000004}");
    let spec = json_doc("{\"a\": 2}");
    let merged = run(Some(&base), &team, &spec);
    assert_eq!(change(&merged, "body").kind, Kind::Merged);
    assert_eq!(
        merged_json(&merged),
        "{\n  \"a\": 2,\n  \"id\": 12345678901234567890123,\n  \"qty\": 1.50,\n  \"big\": 1E3,\n  \"neg\": -0.0e-5,\n  \"price\": 0.30000000000000004\n}"
    );

    let base = json_doc("{\"n\": 1, \"m\": 2}");
    let (team, spec) = (json_doc("{\"n\": 1.0, \"m\": 2}"), json_doc("{\"n\": 1, \"m\": 3}"));
    let merged = run(Some(&base), &team, &spec);
    assert_eq!(
        merged_json(&merged),
        "{\n  \"n\": 1.0,\n  \"m\": 3\n}",
        "1.0 est un autre jeton que 1 : modifié par l'équipe"
    );
    let conflicting = run(Some(&json_doc("{\"n\": 1}")), &json_doc("{\"n\": 1.0}"), &json_doc("{\"n\": 1.00}"));
    assert_eq!(change(&conflicting, "body").kind, Kind::Conflict);
    let kept = choose(
        Some(&json_doc("{\"n\": 1}")),
        &json_doc("{\"n\": 1.0}"),
        &json_doc("{\"n\": 1.00}"),
        "body",
        Choice::Both,
        None,
    )
    .unwrap();
    assert_eq!(merged_json(&kept), "{\n  \"n\": 1.0\n}", "combiner : l'équipe gagne, avec son jeton");

    let not_json = run(Some(&json_doc("{\"a\": 1}")), &json_doc("{\"a\": 01}"), &json_doc("{\"a\": 2}"));
    assert_eq!(change(&not_json, "body").kind, Kind::Conflict, "01 n'est pas un nombre JSON : pas de fusion JSON");
}

#[test]
fn ef_syn_04_new_query_params_are_appended_to_the_query_the_team_typed() {
    let mut base = doc();
    base.url = "{{baseUrl}}/pets".into();
    base.params = vec![Param { enabled: false, ..query("limit", "") }];
    let ours = edit(&base, |d| d.url = "{{baseUrl}}/pets?debug=1&token={{tok}}".into());
    let theirs = edit(&base, |d| d.params = vec![Param { enabled: false, ..query("limit", "") }, query("offset", "5")]);
    let merged = run(Some(&base), &ours, &theirs);
    assert_eq!(
        merged.doc.url, "{{baseUrl}}/pets?debug=1&token={{tok}}&offset=5",
        "ce qu'aucun paramètre ne représente est gardé"
    );

    let mut base = doc();
    base.url = "{{baseUrl}}/pets".into();
    base.params = vec![query("limit", "10"), query("sort", "name")];
    let team = edit(&base, |d| d.url = "{{baseUrl}}/pets?limit=10&debug=1&sort=name".into());
    let changed = edit(&base, |d| d.params = vec![query("limit", "25"), query("sort", "name"), query("page", "2")]);
    let merged = run(Some(&base), &team, &changed);
    assert_eq!(
        merged.doc.url, "{{baseUrl}}/pets?limit=25&debug=1&sort=name&page=2",
        "remplacé sur place, ajouté à la fin"
    );

    let retired = edit(&base, |d| d.params = vec![query("sort", "name")]);
    let merged = run(Some(&base), &team, &retired);
    assert_eq!(merged.doc.url, "{{baseUrl}}/pets?debug=1&sort=name", "le segment du paramètre retiré disparaît");

    let team_disabled = edit(&base, |d| {
        d.url = "{{baseUrl}}/pets?sort=name".into();
        d.params = vec![Param { enabled: false, ..query("limit", "10") }, query("sort", "name")];
    });
    let merged = run(Some(&base), &team_disabled, &changed);
    assert!(merged.doc.url.starts_with("{{baseUrl}}/pets?sort=name"), "{}", merged.doc.url);
}

#[test]
fn ef_syn_04_an_edit_conflict_on_a_keyed_element_carries_the_raw_value() {
    let described =
        |value: &str, enabled: bool| Param { description: Some("max".into()), enabled, ..query("limit", value) };
    let mut base = doc();
    base.params = vec![described("10", true)];
    let team = edit(&base, |d| d.params = vec![described("20", false)]);
    let retired = edit(&base, |d| d.params = vec![]);
    let merged = run(Some(&base), &team, &retired);
    let conflict = change(&merged, "param/query/limit");
    assert_eq!(conflict.choices, [Choice::Team, Choice::Spec, Choice::Edit]);
    assert_eq!(
        (conflict.base.as_deref(), conflict.ours.as_deref(), conflict.theirs.as_deref()),
        (Some("10"), Some("20"), None),
        "la valeur brute, sans « (désactivé) » ni description"
    );
    let typed =
        choose(Some(&base), &team, &retired, "param/query/limit", Choice::Edit, Some(&conflict.ours.clone().unwrap()))
            .unwrap();
    assert_eq!(typed.doc.params[0].value, "20", "valider sans retaper n'ajoute rien à la valeur");
    assert_eq!(
        change(&typed, "param/query/limit").result.as_deref(),
        Some("20 (désactivé) — max"),
        "le résultat reste le texte affiché"
    );

    let mut base = doc();
    base.headers = vec![kv("B", "2")];
    let removed_by_team = edit(&base, |d| d.headers = vec![]);
    let changed_by_spec = edit(&base, |d| {
        d.headers = vec![KeyValue { description: Some("corr".into()), enabled: false, ..kv("B", "3") }]
    });
    let merged = run(Some(&base), &removed_by_team, &changed_by_spec);
    let conflict = change(&merged, "header/b");
    assert_eq!(conflict.choices, [Choice::Team, Choice::Spec, Choice::Edit]);
    assert_eq!(
        (conflict.base.as_deref(), conflict.theirs.as_deref(), conflict.ours.as_deref()),
        (Some("2"), Some("3"), None)
    );

    let mut base = doc();
    base.body = Body::MultipartForm { fields: vec![field("file", "")] };
    let team = edit(&base, |d| {
        d.body = Body::MultipartForm {
            fields: vec![MultipartField {
                value: MultipartValue::File(vec!["a.png".into(), "b.png".into()]),
                ..field("file", "")
            }],
        }
    });
    let retired = edit(&base, |d| d.body = Body::MultipartForm { fields: vec![] });
    let merged = run(Some(&base), &team, &retired);
    assert_eq!(change(&merged, "body/form/file").ours.as_deref(), Some("a.png, b.png"));

    let mut base = doc();
    base.params = vec![Param { description: Some("a".into()), ..query("limit", "10") }];
    let (team, spec) = (
        edit(&base, |d| d.params = vec![Param { description: Some("b".into()), ..query("limit", "10") }]),
        edit(&base, |d| d.params = vec![Param { description: Some("c".into()), ..query("limit", "10") }]),
    );
    let merged = run(Some(&base), &team, &spec);
    let conflict = change(&merged, "param/query/limit");
    assert_eq!(conflict.choices, [Choice::Team, Choice::Spec]);
    assert_eq!(conflict.ours.as_deref(), Some("10 — b"), "pas d'édition proposée : le texte affiché");
}
