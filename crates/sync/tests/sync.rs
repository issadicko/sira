//! EF-SYN-01 à EF-SYN-06 et ENF-PERF-07 : plan et application de la synchro OpenAPI à 3 voies, de bout en bout.
//!
//! La v1 est la fixture `openapi/specs/oai-petstore-3.0.yaml` (importée), la v2 `fixtures/sync/petstore-v2.yaml` est
//! écrite à la main : elle ajoute trois opérations (dont un nouveau tag), retire `showPetById` et modifie `listPets` et
//! `createPets`. Les tests ne lancent jamais Node.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use xc_core::collection::write_atomic;
use xc_core::request::{BLANK_BEFORE, HTTP_ORDER, TOP_ORDER};
use xc_core::yaml::{self, emit, Map, Value};
use xc_core::{normalize, open_collection, read_request, Body, TreeItem};
use xc_sync::import::import_spec;
use xc_sync::merge::{Choice, Decision, Kind};
use xc_sync::openapi::GroupBy;
use xc_sync::store;
use xc_sync::sync::{self, Decisions, OpStatus, Operation, Plan, SyncError};

fn fixture(relative: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(relative)).unwrap()
}

fn v1() -> String {
    fixture("openapi/specs/oai-petstore-3.0.yaml")
}

fn v2() -> String {
    fixture("sync/petstore-v2.yaml")
}

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

fn world(spec: &str) -> World {
    world_grouped(spec, GroupBy::Tags)
}

fn world_grouped(spec: &str, group_by: GroupBy) -> World {
    let dir = tempfile::tempdir().unwrap();
    let specs = dir.path().join("specs");
    let out = dir.path().join("out");
    fs::create_dir_all(&specs).unwrap();
    fs::create_dir_all(&out).unwrap();
    let path = specs.join("api.yaml");
    fs::write(&path, spec).unwrap();
    let root = import_spec(spec, path.to_str().unwrap(), &out, group_by).unwrap();
    World { _dir: dir, root }
}

fn files_under(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

fn read(root: &Path, relative: &str) -> String {
    fs::read_to_string(root.join(relative)).unwrap()
}

fn recorded(root: &Path) -> String {
    sync::status(root).unwrap().source.unwrap()
}

fn plan_of(world: &World, spec: &str) -> Plan {
    sync::plan(&world.root, spec, recorded(&world.root), &[]).unwrap()
}

fn op<'a>(plan: &'a Plan, key: &str) -> &'a Operation {
    plan.operations.iter().find(|o| o.key == key).unwrap_or_else(|| panic!("opération {key} absente"))
}

fn statuses(plan: &Plan) -> Vec<(&str, OpStatus)> {
    plan.operations.iter().map(|o| (o.key.as_str(), o.status)).collect()
}

fn entry<const N: usize>(pairs: [(&str, &str); N]) -> Value {
    Value::Map(Map(pairs.iter().map(|(k, v)| ((*k).to_owned(), Value::str(*v))).collect()))
}

fn edit_file(root: &Path, relative: &str, change: impl FnOnce(&mut Map)) {
    let path = root.join(relative);
    let Value::Map(mut tree) = yaml::parse(&fs::read_to_string(&path).unwrap()).unwrap() else {
        panic!("table attendue")
    };
    change(&mut tree);
    fs::write(&path, emit(&Value::Map(tree), BLANK_BEFORE)).unwrap();
}

fn child<'a>(map: &'a mut Map, key: &str) -> &'a mut Map {
    match map.get_mut(key) {
        Some(Value::Map(inner)) => inner,
        _ => panic!("{key} absent"),
    }
}

fn script() -> Value {
    let script = Value::Map(Map(vec![
        ("type".into(), Value::str("before-request")),
        ("code".into(), Value::str("bru.setVar(\"team\", 1);")),
    ]));
    Value::Map(Map(vec![("scripts".into(), Value::Seq(vec![script]))]))
}

const LIST_PETS: &str = "pets/List all pets.yml";
const CREATE_PET: &str = "pets/Create a pet.yml";
const SHOW_PET: &str = "pets/Info for a specific pet.yml";

fn team_works(root: &Path) {
    edit_file(root, LIST_PETS, |tree| {
        let http = child(tree, "http");
        http.set("url", Value::str("{{baseUrl}}/pets?limit=20"), HTTP_ORDER);
        http.set("headers", Value::Seq(vec![entry([("name", "X-Team"), ("value", "1")])]), HTTP_ORDER);
        if let Some(Value::Seq(params)) = http.get_mut("params") {
            if let Value::Map(limit) = &mut params[0] {
                limit.set("value", Value::str("20"), &[]);
                limit.remove("disabled");
            }
        }
        tree.set("runtime", script(), TOP_ORDER);
        tree.set("x-team", Value::str("note de l'équipe"), &[]);
    });
    edit_file(root, CREATE_PET, |tree| {
        let http = child(tree, "http");
        child(http, "body").set(
            "data",
            Value::str("{\n  \"id\": 0,\n  \"name\": \"Rex\",\n  \"tag\": \"friendly\"\n}"),
            &[],
        );
        http.set("x-extra", Value::str("kept"), &[]);
        tree.set("runtime", script(), TOP_ORDER);
    });
}

fn section(text: &str, key: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.iter().position(|l| l.starts_with(&format!("{key}:"))).unwrap_or_else(|| panic!("{key} absent"));
    let end = (start + 1..lines.len())
        .find(|&i| lines[i].chars().next().is_some_and(|c| !c.is_whitespace()))
        .unwrap_or(lines.len());
    lines[start..end].join("\n")
}

fn lines(text: &str, range: [usize; 2]) -> Vec<&str> {
    text.lines().skip(range[0] - 1).take(range[1] - range[0] + 1).collect()
}

fn choice(id: &str, choice: Choice) -> Decisions {
    let decision = Decision { choice, value: None };
    Decisions { choices: [(id.to_owned(), decision)].into(), ..Decisions::default() }
}

#[test]
fn ef_syn_01_status_tells_whether_a_collection_is_connected() {
    let world = world(&v1());
    let status = sync::status(&world.root).unwrap();
    assert!(status.connected);
    assert_eq!((status.group_by, status.operation_count, status.removed_count), (Some(GroupBy::Tags), 3, 0));
    assert_eq!(status.source.as_deref(), Some("../../specs/api.yaml"));
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(json["groupBy"], "tags");
    assert_eq!((json["operationCount"].as_u64(), json["removedCount"].as_u64()), (Some(3), Some(0)));

    fs::remove_dir_all(world.root.join(".oc-sync")).unwrap();
    let status = sync::status(&world.root).unwrap();
    assert!(!status.connected);
    assert_eq!((status.source, status.group_by, status.operation_count), (None, None, 0));
    let json = serde_json::to_value(sync::status(&world.root).unwrap()).unwrap();
    assert!(json["source"].is_null() && json["groupBy"].is_null());
}

#[test]
fn ef_syn_05_plan_previews_every_change_without_writing_anything() {
    let world = world(&v1());
    team_works(&world.root);
    let before = files_under(&world.root);

    let plan = plan_of(&world, &v2());
    assert_eq!(files_under(&world.root), before, "le plan n'écrit rien");
    assert!(plan.has_base);
    assert_eq!((plan.from.as_ref().unwrap().version.as_str(), plan.to.version.as_str()), ("1.0.0", "1.1.0"));
    assert_eq!(plan.to.title, "Swagger Petstore");
    assert_eq!(plan.source, "../../specs/api.yaml");
    assert_eq!(
        statuses(&plan),
        [
            ("listPets", OpStatus::Merged),
            ("createPets", OpStatus::Conflict),
            ("deletePet", OpStatus::New),
            ("listPhotos", OpStatus::New),
            ("listOwners", OpStatus::New),
            ("showPetById", OpStatus::Removed),
        ]
    );
    let summary = &plan.summary;
    assert_eq!(
        (summary.unchanged, summary.updated, summary.kept, summary.merged, summary.conflicts, summary.conflict_fields),
        (0, 0, 0, 1, 1, 1)
    );
    assert_eq!((summary.created, summary.removed, summary.restored, summary.missing), (3, 1, 0, 0));
    assert!(plan.diverged());

    let list = op(&plan, "listPets");
    assert_eq!((list.name.as_str(), list.method.as_str(), list.path.as_str()), ("List all pets", "GET", "/pets"));
    assert_eq!(list.file.as_deref(), Some(LIST_PETS));
    let kinds: Vec<(&str, Kind)> = list.changes.iter().map(|c| (c.id.as_str(), c.kind)).collect();
    assert_eq!(
        kinds,
        [
            ("listPets::header/x-request-id", Kind::Applied),
            ("listPets::param/query/limit", Kind::Merged),
            ("listPets::param/query/offset", Kind::Applied),
        ]
    );
    let create = op(&plan, "createPets");
    let conflict = &create.changes[0];
    assert_eq!((conflict.id.as_str(), conflict.kind), ("createPets::body", Kind::Conflict));
    assert_eq!(conflict.choices, [Choice::Team, Choice::Spec, Choice::Both, Choice::Edit]);
    assert!(
        conflict.result.is_none() && conflict.base.is_some() && conflict.ours.is_some() && conflict.theirs.is_some()
    );
    assert!(op(&plan, "listPhotos").file.is_none() && op(&plan, "listPhotos").changes.is_empty());
    assert_eq!(op(&plan, "showPetById").file.as_deref(), Some(SHOW_PET));
    assert_eq!(op(&plan, "listPhotos").path, "/pets/{petId}/photos");
    assert_eq!(plan.suggestions.len(), 1, "mêmes paramètres : {:?}", plan.suggestions);
    assert_eq!(
        (plan.suggestions[0].removed.as_str(), plan.suggestions[0].added.as_str()),
        ("showPetById", "listPhotos")
    );
}

#[test]
fn ef_syn_05_plan_serializes_as_the_ipc_contract() {
    let world = world(&v1());
    team_works(&world.root);
    let plan = plan_of(&world, &v2());
    let json = serde_json::to_value(&plan).unwrap();
    assert_eq!(json["groupBy"], "tags");
    assert_eq!(json["hasBase"], true);
    assert_eq!(json["from"]["version"], "1.0.0");
    assert_eq!(json["to"]["title"], "Swagger Petstore");
    assert_eq!(json["summary"]["conflictFields"], 1);
    assert!(json.get("state").is_none() && json.get("id").is_none());
    let operations = json["operations"].as_array().unwrap();
    assert_eq!(operations[1]["status"], "conflict");
    assert_eq!(operations[1]["changes"][0]["field"], "body");
    assert_eq!(operations[1]["changes"][0]["kind"], "conflict");
    assert_eq!(operations[1]["changes"][0]["choices"], serde_json::json!(["team", "spec", "both", "edit"]));
    assert!(operations[1]["changes"][0].get("decided").is_none());
    assert_eq!(operations[0]["changes"][0]["field"], "header");
    assert_eq!(json["suggestions"][0]["removed"], "showPetById");
    assert_eq!(json["suggestions"][0]["added"], "listPhotos");

    let decisions: Decisions = serde_json::from_str(
        r#"{"choices": {"a::body": {"choice": "edit", "value": "{}"}, "b::method": {"choice": "team"}}, "skip": ["x"]}"#,
    )
    .unwrap();
    assert_eq!(decisions.choices["a::body"].value.as_deref(), Some("{}"));
    assert_eq!((decisions.skip, decisions.recreate), (vec!["x".to_owned()], Vec::<String>::new()));
}

#[test]
fn ef_syn_06_apply_refuses_an_unarbitrated_conflict_and_writes_nothing() {
    let world = world(&v1());
    team_works(&world.root);
    let before = files_under(&world.root);
    let plan = plan_of(&world, &v2());
    let error = plan.apply(&Decisions::default()).unwrap_err();
    assert!(matches!(error, SyncError::Unresolved { count: 1, .. }), "{error}");
    assert!(error.to_string().contains("createPets::body"), "{error}");
    assert_eq!(files_under(&world.root), before);

    let wrong = choice("createPets::url", Choice::Team);
    assert!(plan.apply(&wrong).is_err(), "une décision sur un autre changement ne tranche pas le conflit");
    let team = choice("createPets::body", Choice::Team);
    assert!(plan.apply(&team).is_ok());
}

#[test]
fn ef_syn_04_sync_to_a_new_spec_version_keeps_the_team_work() {
    let world = world(&v1());
    let imported = files_under(&world.root);
    team_works(&world.root);
    let team = files_under(&world.root);

    let plan = plan_of(&world, &v2());
    let report = plan.apply(&choice("createPets::body", Choice::Both)).unwrap();
    assert_eq!(report.written, [LIST_PETS, CREATE_PET]);
    assert_eq!(
        report.created,
        ["pets/Delete a pet.yml", "pets/List the photos of a pet.yml", "owners/List the owners.yml"]
    );
    assert_eq!(report.removed, [SHOW_PET]);
    assert!(report.ignored.is_empty());
    let after = files_under(&world.root);

    let list = read(&world.root, LIST_PETS);
    let before = String::from_utf8(team[LIST_PETS].clone()).unwrap();
    for key in ["info", "settings", "runtime", "x-team"] {
        assert_eq!(section(&list, key), section(&before, key), "section {key} identique à l'octet");
    }
    assert!(section(&before, "http").contains("url: \"{{baseUrl}}/pets?limit=20\""));
    assert!(
        section(&list, "http").contains("url: \"{{baseUrl}}/pets?limit=20\""),
        "l'URL de l'équipe est conservée à l'octet"
    );
    let doc = read_request(&world.root, LIST_PETS).unwrap();
    let headers: Vec<(&str, &str)> = doc.headers.iter().map(|h| (h.name.as_str(), h.value.as_str())).collect();
    assert_eq!(headers, [("X-Team", "1"), ("X-Request-Id", "")]);
    let limit = &doc.params[0];
    assert_eq!((limit.value.as_str(), limit.enabled), ("20", true), "la valeur et l'activation de l'équipe");
    assert_eq!(limit.description.as_deref(), Some("How many pets to return at one time (max 200)"));
    assert_eq!((doc.params[1].name.as_str(), doc.params[1].enabled), ("offset", false));
    assert_eq!(doc.scripts.len(), 1);

    let create = read_request(&world.root, CREATE_PET).unwrap();
    assert_eq!(
        create.body,
        Body::Json {
            data: "{\n  \"id\": 0,\n  \"name\": \"Rex\",\n  \"tag\": \"friendly\",\n  \"birthday\": \"\"\n}".into()
        }
    );
    let create_before = String::from_utf8(team[CREATE_PET].clone()).unwrap();
    assert_eq!(
        read(&world.root, CREATE_PET),
        create_before
            .replace("        \"tag\": \"friendly\"\n", "        \"tag\": \"friendly\",\n        \"birthday\": \"\"\n"),
        "seul le corps change, le script et le reste sont identiques à l'octet"
    );

    assert_eq!(after[SHOW_PET], team[SHOW_PET], "la requête retirée est intacte");
    assert_eq!(after[SHOW_PET], imported[SHOW_PET]);
    let source = read(&world.root, ".oc-sync/openapi/source.yml");
    assert!(
        source.contains("  - key: showPetById\n    file: pets/Info for a specific pet.yml\n    removed: true\n"),
        "{source}"
    );
    assert_eq!(after[".oc-sync/openapi/spec.yaml"], v2().into_bytes(), "copie brute de la v2, octet pour octet");
    assert!(
        source.contains("spec: spec.yaml\n")
            && source.contains("  - key: listOwners\n    file: owners/List the owners.yml\n")
    );

    let modified = |relative: &str| fs::metadata(world.root.join(relative)).unwrap().modified().unwrap();
    let source_time = modified(".oc-sync/openapi/source.yml");
    for relative in after.keys() {
        assert!(modified(relative) <= source_time, "{relative} écrit après source.yml");
    }

    let again = plan_of(&world, &v2());
    assert!(!again.diverged(), "{:?}", statuses(&again));
    assert_eq!(
        statuses(&again),
        [
            ("listPets", OpStatus::Kept),
            ("createPets", OpStatus::Kept),
            ("deletePet", OpStatus::Unchanged),
            ("listPhotos", OpStatus::Unchanged),
            ("listOwners", OpStatus::Unchanged),
        ],
        "la synchro rejouée n'a plus rien à faire ; `kept` : valeurs de l'équipe conservées"
    );
    assert_eq!((again.summary.created, again.summary.removed, again.summary.conflicts), (0, 0, 0));
    let replay = again.apply(&Decisions::default()).unwrap();
    assert!(replay.written.is_empty() && replay.created.is_empty() && replay.removed.is_empty());
    assert_eq!(files_under(&world.root), after, "rien ne bouge");
}

#[test]
fn ef_syn_02_new_operations_go_to_the_folder_of_their_tag_with_the_next_seq() {
    let world = world(&v1());
    fs::write(
        world.root.join("pets/Mine.yml"),
        "info:\n  name: Mine\n  type: http\n  seq: 4\n\nhttp:\n  method: GET\n  url: http://x.test\n",
    )
    .unwrap();
    plan_of(&world, &v2()).apply(&Decisions::default()).unwrap();

    let delete = read_request(&world.root, "pets/Delete a pet.yml").unwrap();
    let photos = read_request(&world.root, "pets/List the photos of a pet.yml").unwrap();
    assert_eq!(
        (delete.seq, photos.seq),
        (Some(5), Some(6)),
        "un de plus que les éléments du dossier, ajoutés dans l'ordre"
    );
    assert_eq!((delete.method.as_str(), delete.url.as_str()), ("DELETE", "{{baseUrl}}/pets/:petId"));
    let owners = read_request(&world.root, "owners/List the owners.yml").unwrap();
    assert_eq!(owners.seq, Some(1));
    let folder = read(&world.root, "owners/folder.yml");
    assert!(
        folder.starts_with("info:\n  name: owners\n  type: folder\n") && folder.contains("Propriétaires des animaux"),
        "{folder}"
    );
    assert_eq!(
        read(&world.root, "pets/folder.yml"),
        "info:\n  name: pets\n  type: folder\n\nrequest:\n  auth: inherit\n",
        "le dossier du tag existe déjà"
    );
    assert!(!world.root.join("pets 1").exists(), "pas de dossier en double");

    let info = open_collection(&world.root).unwrap();
    assert_eq!(info.request_count, 7, "les trois anciennes, la requête de l'équipe et les trois nouvelles");
}

#[test]
fn ef_syn_02_new_files_take_import_names_and_never_overwrite() {
    let world = world(&v1());
    fs::write(
        world.root.join("pets/Delete a pet.yml"),
        "info:\n  name: Mine\n  type: http\n\nhttp:\n  method: GET\n  url: http://x.test\n",
    )
    .unwrap();
    let report = plan_of(&world, &v2()).apply(&Decisions::default()).unwrap();
    assert_eq!(report.created[0], "pets/Delete a pet 1.yml");
    assert!(read(&world.root, "pets/Delete a pet.yml").contains("name: Mine"), "le fichier de l'équipe est intact");
    let source = read(&world.root, ".oc-sync/openapi/source.yml");
    assert!(source.contains("  - key: deletePet\n    file: pets/Delete a pet 1.yml\n"), "{source}");
}

#[test]
fn ef_syn_02_a_skipped_new_operation_is_ignored_from_then_on() {
    let world = world(&v1());
    let decisions = Decisions { skip: vec!["listOwners".into(), "deletePet".into()], ..Decisions::default() };
    let report = plan_of(&world, &v2()).apply(&decisions).unwrap();
    assert_eq!(report.ignored, ["deletePet", "listOwners"]);
    assert_eq!(report.created, ["pets/List the photos of a pet.yml"]);
    assert!(!world.root.join("owners").exists() && !world.root.join("pets/Delete a pet.yml").exists());
    let source = read(&world.root, ".oc-sync/openapi/source.yml");
    assert!(
        source.contains("  - key: deletePet\n    ignored: true\n")
            && source.contains("  - key: listOwners\n    ignored: true\n")
    );

    let again = plan_of(&world, &v2());
    assert!(again.operations.iter().all(|o| o.key != "deletePet" && o.key != "listOwners"), "{:?}", statuses(&again));
    assert!(!again.diverged());
    again.apply(&Decisions::default()).unwrap();
    assert!(read(&world.root, ".oc-sync/openapi/source.yml").contains("  - key: deletePet\n    ignored: true\n"));
}

#[test]
fn ef_syn_03_a_removed_operation_is_flagged_and_its_file_is_never_touched() {
    let world = world(&v1());
    let before = files_under(&world.root)[SHOW_PET].clone();
    plan_of(&world, &v2()).apply(&Decisions::default()).unwrap();
    assert_eq!(files_under(&world.root)[SHOW_PET], before);
    assert_eq!(store::removed_files(&world.root).into_iter().collect::<Vec<_>>(), [SHOW_PET]);
    assert_eq!(sync::status(&world.root).unwrap().removed_count, 1);

    let still_gone = plan_of(&world, &v2());
    assert!(still_gone.operations.iter().all(|o| o.key != "showPetById"), "déjà dépréciée : plus rien à signaler");
    assert!(!still_gone.diverged());
    still_gone.apply(&Decisions::default()).unwrap();
    assert!(read(&world.root, ".oc-sync/openapi/source.yml")
        .contains("showPetById\n    file: pets/Info for a specific pet.yml\n    removed: true"));
    assert_eq!(files_under(&world.root)[SHOW_PET], before);
}

#[test]
fn ef_syn_03_an_operation_back_in_the_spec_is_restored() {
    let world = world(&v1());
    plan_of(&world, &v2()).apply(&Decisions::default()).unwrap();
    let plan = plan_of(&world, &v1());
    assert_eq!(op(&plan, "showPetById").status, OpStatus::Restored);
    assert_eq!(op(&plan, "showPetById").file.as_deref(), Some(SHOW_PET));
    assert!(plan.diverged());
    assert_eq!(plan.summary.restored, 1);
    plan.apply(&Decisions::default()).unwrap();
    let removed = store::removed_files(&world.root);
    assert!(!removed.contains(SHOW_PET), "le drapeau est retiré : {removed:?}");
    assert_eq!(removed.len(), 3, "les trois opérations de la v2 absentes de la v1 sont à leur tour retirées");
    let source = read(&world.root, ".oc-sync/openapi/source.yml");
    assert!(source.contains("  - key: showPetById\n    file: pets/Info for a specific pet.yml\n"), "{source}");
    assert!(!source.contains("showPetById\n    file: pets/Info for a specific pet.yml\n    removed"));
}

#[test]
fn ef_syn_01_a_missing_file_is_forgotten_unless_it_is_recreated() {
    let world = world(&v1());
    fs::remove_file(world.root.join(SHOW_PET)).unwrap();
    let spec = v1();
    let plan = plan_of(&world, &spec);
    assert_eq!(op(&plan, "showPetById").status, OpStatus::Missing);
    assert_eq!(plan.summary.missing, 1);
    let report = plan.apply(&Decisions::default()).unwrap();
    assert_eq!(report.ignored, ["showPetById"]);
    assert!(!world.root.join(SHOW_PET).exists());
    assert!(read(&world.root, ".oc-sync/openapi/source.yml").contains("  - key: showPetById\n    ignored: true\n"));

    let world = self::world(&spec);
    fs::remove_file(world.root.join(SHOW_PET)).unwrap();
    let decisions = Decisions { recreate: vec!["showPetById".into()], ..Decisions::default() };
    let report = plan_of(&world, &spec).apply(&decisions).unwrap();
    assert_eq!(report.created, [SHOW_PET]);
    assert_eq!(read_request(&world.root, SHOW_PET).unwrap().url, "{{baseUrl}}/pets/:petId");
    assert!(!plan_of(&world, &spec).diverged());
}

#[test]
fn ef_syn_06_an_interrupted_sync_replays_without_duplicates_or_conflicts() {
    let clean = world(&v1());
    team_works(&clean.root);
    let spec_choice = choice("createPets::body", Choice::Spec);
    plan_of(&clean, &v2()).apply(&spec_choice).unwrap();
    let expected = files_under(&clean.root);

    let world = world(&v1());
    team_works(&world.root);
    let store_dir = world.root.join(".oc-sync/openapi");
    let old_source = fs::read(store_dir.join("source.yml")).unwrap();
    let old_spec = fs::read(store_dir.join("spec.yaml")).unwrap();
    plan_of(&world, &v2()).apply(&spec_choice).unwrap();
    let written = files_under(&world.root);
    fs::write(store_dir.join("source.yml"), old_source).unwrap();
    fs::write(store_dir.join("spec.yaml"), old_spec).unwrap();

    let replay = plan_of(&world, &v2());
    assert_eq!(replay.summary.conflicts, 0, "{:?}", statuses(&replay));
    assert_eq!(
        statuses(&replay),
        [
            ("listPets", OpStatus::Kept),
            ("createPets", OpStatus::Unchanged),
            ("deletePet", OpStatus::New),
            ("listPhotos", OpStatus::New),
            ("listOwners", OpStatus::New),
            ("showPetById", OpStatus::Removed),
        ]
    );
    let list = op(&replay, "listPets");
    assert!(list
        .changes
        .iter()
        .filter(|c| c.id.contains("x-request-id") || c.id.contains("offset"))
        .all(|c| c.kind == Kind::Same));
    let report = replay.apply(&Decisions::default()).unwrap();
    assert!(report.written.is_empty(), "les champs déjà écrits ne sont pas réécrits : {:?}", report.written);
    assert_eq!(report.created.len(), 3, "les fichiers déjà créés sont réutilisés");
    let after = files_under(&world.root);
    assert_eq!(after.keys().collect::<Vec<_>>(), written.keys().collect::<Vec<_>>(), "aucun doublon");
    for (path, bytes) in &after {
        assert_eq!(bytes, &expected[path], "{path} : même état qu'une synchro menée sans interruption");
    }
    assert_eq!(after.keys().collect::<Vec<_>>(), expected.keys().collect::<Vec<_>>());
}

#[test]
fn ef_syn_06_apply_refuses_a_file_that_changed_since_the_plan() {
    let world = world(&v1());
    let plan = plan_of(&world, &v2());
    let path = world.root.join(LIST_PETS);
    let edited = fs::read_to_string(&path).unwrap().replace("name: List all pets", "name: Autre nom");
    fs::write(&path, &edited).unwrap();
    let before = files_under(&world.root);
    let error = plan.apply(&Decisions::default()).unwrap_err();
    assert!(matches!(&error, SyncError::Stale(file) if file == LIST_PETS), "{error}");
    assert_eq!(files_under(&world.root), before, "rien n'est écrit, la base n'a pas bougé");
    assert!(!world.root.join("owners").exists());
}

#[test]
fn ef_syn_06_apply_without_conflict_updates_files_then_store() {
    let world = world(&v1());
    let plan = plan_of(&world, &v2());
    assert_eq!(plan.summary.conflicts, 0);
    assert_eq!(statuses(&plan)[..2], [("listPets", OpStatus::Updated), ("createPets", OpStatus::Updated)]);
    let report = plan.apply(&Decisions::default()).unwrap();
    assert_eq!(report.written, [LIST_PETS, CREATE_PET]);
    let create = read_request(&world.root, CREATE_PET).unwrap();
    assert_eq!(create.body, Body::Json { data: "{\n  \"id\": 0,\n  \"name\": \"\",\n  \"birthday\": \"\"\n}".into() });
    let spec = read(&world.root, ".oc-sync/openapi/spec.yaml");
    assert_eq!(spec, v2());
    let again = plan_of(&world, &v2());
    assert!(again.operations.iter().all(|o| o.status == OpStatus::Unchanged), "{:?}", statuses(&again));
}

#[test]
fn ef_syn_01_a_collection_without_base_is_connected_by_method_and_normalized_path() {
    let world = world(&v1());
    fs::remove_dir_all(world.root.join(".oc-sync")).unwrap();
    team_works(&world.root);
    let before = files_under(&world.root);

    let plan = sync::plan(&world.root, &v2(), "../../specs/api.yaml".into(), &[]).unwrap();
    assert_eq!(files_under(&world.root), before);
    assert!(!plan.has_base && plan.from.is_none());
    assert_eq!(
        statuses(&plan),
        [
            ("listPets", OpStatus::Merged),
            ("createPets", OpStatus::Conflict),
            ("deletePet", OpStatus::New),
            ("listPhotos", OpStatus::New),
            ("listOwners", OpStatus::New),
        ],
        "showPetById n'est pas dans la spec : sans base, c'est une requête de l'équipe, que la synchro ne lit pas"
    );
    let conflict = &op(&plan, "createPets").changes[0];
    assert_eq!(conflict.choices, [Choice::Team, Choice::Spec, Choice::Edit], "pas de fusion JSON sans base");
    assert!(conflict.base.is_none());
    let list: Vec<_> = op(&plan, "listPets").changes.iter().map(|c| (c.id.as_str(), c.kind)).collect();
    assert_eq!(
        list,
        [
            ("listPets::header/x-request-id", Kind::Applied),
            ("listPets::param/query/limit", Kind::Kept),
            ("listPets::param/query/offset", Kind::Applied)
        ]
    );

    let report = plan.apply(&Decisions::uniform(&plan, Choice::Team)).unwrap();
    assert_eq!(report.written, [LIST_PETS]);
    assert_eq!(report.created.len(), 3);
    assert!(report.removed.is_empty());
    let after = files_under(&world.root);
    assert_eq!(after[CREATE_PET], before[CREATE_PET], "le conflit résolu pour l'équipe ne change pas le fichier");
    assert_eq!(after[SHOW_PET], before[SHOW_PET]);
    let status = sync::status(&world.root).unwrap();
    assert_eq!((status.connected, status.operation_count, status.removed_count), (true, 5, 0));
    assert_eq!(status.group_by, Some(GroupBy::Tags));
    let source = read(&world.root, ".oc-sync/openapi/source.yml");
    assert!(
        source.starts_with("source: ../../specs/api.yaml\ngroupBy: tags\nspec: spec.yaml\noperations:\n"),
        "{source}"
    );
    assert!(!source.contains("showPetById"));

    let next = plan_of(&world, &v2());
    assert!(!next.diverged(), "{:?}", statuses(&next));
    assert!(next.has_base);
}

#[test]
fn ef_syn_01_an_ambiguous_match_is_not_made() {
    let world = world(&v1());
    fs::remove_dir_all(world.root.join(".oc-sync")).unwrap();
    fs::copy(world.root.join(LIST_PETS), world.root.join("pets/Copy of list.yml")).unwrap();
    let plan = sync::plan(&world.root, &v1(), "api.yaml".into(), &[]).unwrap();
    assert_eq!(
        op(&plan, "listPets").status,
        OpStatus::New,
        "deux requêtes pour une opération : traitée comme nouvelle"
    );
    assert_eq!(op(&plan, "createPets").status, OpStatus::Unchanged);
}

const USERS_V1: &str = "openapi: 3.0.0
info: {title: Users, version: '1'}
paths:
  /users/{id}:
    get:
      summary: Get user
      tags: [users]
      parameters:
        - {name: id, in: path, required: true, schema: {type: string}}
      responses: {'200': {description: ok}}
  /health:
    get:
      summary: Health
      responses: {'200': {description: ok}}
";

fn users_v2(path: &str, summary: &str) -> String {
    USERS_V1.replace("/users/{id}", path).replace("Get user", summary).replace("version: '1'", "version: '2'")
}

#[test]
fn ef_syn_01_a_changed_path_without_operation_id_is_proposed_for_pairing() {
    let world = world(USERS_V1);
    let status = sync::status(&world.root).unwrap();
    assert_eq!(status.operation_count, 2);
    let renamed = users_v2("/people/{id}", "Get user");

    let plan = plan_of(&world, &renamed);
    assert_eq!(
        statuses(&plan),
        [("GET /people/{}", OpStatus::New), ("GET /health", OpStatus::Unchanged), ("GET /users/{}", OpStatus::Removed)]
    );
    assert_eq!(plan.suggestions.len(), 1);
    let suggestion = &plan.suggestions[0];
    assert_eq!((suggestion.removed.as_str(), suggestion.added.as_str()), ("GET /users/{}", "GET /people/{}"));
    assert!(suggestion.reason.contains("même nom"), "{}", suggestion.reason);
    assert_eq!(serde_json::to_value(&plan).unwrap()["suggestions"][0]["removed"], "GET /users/{}");

    let pairings = [("GET /users/{}".to_owned(), "GET /people/{}".to_owned())];
    let paired = sync::plan(&world.root, &renamed, recorded(&world.root), &pairings).unwrap();
    assert_eq!(statuses(&paired), [("GET /people/{}", OpStatus::Updated), ("GET /health", OpStatus::Unchanged)]);
    assert!(paired.suggestions.is_empty());
    let moved = op(&paired, "GET /people/{}");
    assert_eq!(moved.file.as_deref(), Some("users/Get user.yml"));
    assert_eq!(
        moved.changes.iter().map(|c| (c.id.as_str(), c.kind)).collect::<Vec<_>>(),
        [("GET /people/{}::url", Kind::Applied)]
    );

    let report = paired.apply(&Decisions::default()).unwrap();
    assert_eq!(
        (report.written.as_slice(), report.created.len(), report.removed.len()),
        (&["users/Get user.yml".to_owned()][..], 0, 0)
    );
    assert_eq!(read_request(&world.root, "users/Get user.yml").unwrap().url, "{{baseUrl}}/people/:id");
    let source = read(&world.root, ".oc-sync/openapi/source.yml");
    assert!(
        source.contains("  - key: GET /people/{}\n    file: users/Get user.yml\n") && !source.contains("/users/"),
        "{source}"
    );
    assert!(!plan_of(&world, &renamed).diverged());
}

#[test]
fn ef_syn_01_pairing_suggestions_follow_the_documented_criteria() {
    let cases = [
        (users_v2("/members/{id}", "Fetch member"), Some("mêmes paramètres")),
        (users_v2("/people/{id}", "Get user"), Some("même nom")),
        (users_v2("/a/b", "Anything").replace("name: id", "name: x"), None),
    ];
    for (spec, expected) in cases {
        let world = world(USERS_V1);
        let plan = plan_of(&world, &spec);
        let found = plan.suggestions.first().map(|s| s.reason.as_str());
        match expected {
            Some(reason) => assert!(found.is_some_and(|f| f.contains(reason)), "{spec} : {found:?}"),
            None => assert!(found.is_none(), "{found:?}"),
        }
    }

    let base = "openapi: 3.0.0
info: {title: T, version: '1'}
paths:
  /api/v1/users/list:
    get: {summary: Alpha, responses: {'200': {description: ok}}}
";
    let world = world(base);
    let close = base.replace("/api/v1/users/list", "/api/v2/users/list").replace("Alpha", "Beta");
    let plan = plan_of(&world, &close);
    assert!(plan.suggestions.first().is_some_and(|s| s.reason.contains("proches")), "{:?}", plan.suggestions);
    let far = base.replace("/api/v1/users/list", "/x/y/z").replace("Alpha", "Beta");
    assert!(plan_of(&world, &far).suggestions.is_empty());
    let other_method = base.replace("get:", "post:").replace("/api/v1/users/list", "/api/v2/users/list");
    assert!(plan_of(&world, &other_method).suggestions.is_empty(), "la méthode doit être la même");
}

#[test]
fn ef_syn_01_invalid_pairings_are_refused() {
    let world = world(USERS_V1);
    let renamed = users_v2("/people/{id}", "Get user");
    let source = recorded(&world.root);
    let attempt = |pairing: &[(&str, &str)]| {
        let pairings: Vec<(String, String)> = pairing.iter().map(|(a, b)| ((*a).to_owned(), (*b).to_owned())).collect();
        sync::plan(&world.root, &renamed, source.clone(), &pairings).unwrap_err().to_string()
    };
    assert!(attempt(&[("GET /health", "GET /people/{}")]).contains("n'est pas une opération retirée"));
    assert!(attempt(&[("GET /users/{}", "GET /health")]).contains("n'est pas une nouvelle opération"));
    assert!(attempt(&[("GET /unknown", "GET /people/{}")]).contains("rapprochement impossible"));
    assert!(attempt(&[("GET /users/{}", "GET /people/{}"), ("GET /users/{}", "GET /people/{}")]).contains("deux fois"));
}

#[test]
fn ef_syn_05_op_view_gives_the_four_files_and_the_lines_of_each_change() {
    let world = world(&v1());
    let imported = files_under(&world.root);
    team_works(&world.root);
    let plan = plan_of(&world, &v2());

    let view = plan.op_view("listPets", &Decisions::default()).unwrap();
    assert_eq!(view.ours, read(&world.root, LIST_PETS));
    assert_eq!(view.base.as_deref(), Some(String::from_utf8(imported[LIST_PETS].clone()).unwrap().as_str()));
    assert!(view.theirs.contains("name: X-Request-Id") && view.theirs.contains("name: offset"));
    let list_result = view.result.clone();
    let hunk = |id: &str| view.hunks.iter().find(|h| h.change_id == format!("listPets::{id}")).unwrap();

    let limit = hunk("param/query/limit");
    assert_eq!(lines(&view.ours, limit.ours.unwrap())[0], "    - name: limit");
    assert_eq!(lines(&view.ours, limit.ours.unwrap()).len(), 4, "name, value, type, description");
    assert!(lines(view.base.as_ref().unwrap(), limit.base.unwrap()).iter().any(|l| l.contains("disabled: true")));
    assert!(lines(&view.theirs, limit.theirs.unwrap()).iter().any(|l| l.contains("max 200")));
    let result = lines(&view.result, limit.result.unwrap());
    assert!(
        result.iter().any(|l| l.contains("value: \"20\"")) && result.iter().any(|l| l.contains("max 200")),
        "{result:?}"
    );

    let header = hunk("header/x-request-id");
    assert!(header.ours.is_none() && header.base.is_none(), "absent de l'équipe et de la base");
    assert_eq!(lines(&view.theirs, header.theirs.unwrap())[0], "    - name: X-Request-Id");
    assert!(lines(&view.result, header.result.unwrap()).iter().any(|l| l.contains("Correlation identifier")));
    let offset = hunk("param/query/offset");
    assert!(offset.ours.is_none() && offset.theirs.is_some() && offset.result.is_some());

    let create = plan.op_view("createPets", &Decisions::default()).unwrap();
    let body = &create.hunks[0];
    assert_eq!(body.change_id, "createPets::body");
    for (text, range) in
        [(&create.ours, body.ours), (&create.theirs, body.theirs), (create.base.as_ref().unwrap(), body.base)]
    {
        let range = lines(text, range.unwrap());
        assert_eq!((range[0], range.last().copied()), ("  body:", Some("      }")), "{range:?}");
    }
    assert_eq!(create.result, create.ours, "tant que le conflit n'est pas arbitré, le résultat est celui de l'équipe");
    assert_eq!(body.result, body.ours);

    let both = plan.op_view("createPets", &choice("createPets::body", Choice::Both)).unwrap();
    assert!(both.result.contains("\"birthday\": \"\"") && both.result.contains("\"tag\": \"friendly\""));
    assert_eq!(both.ours, create.ours);
    let spec = plan.op_view("createPets", &choice("createPets::body", Choice::Spec)).unwrap();
    assert!(!spec.result.contains("friendly") && spec.result.contains("birthday"));
    let after = plan.apply(&choice("createPets::body", Choice::Both)).unwrap();
    assert_eq!(read(&world.root, CREATE_PET), both.result, "la vue montre ce qui sera écrit");
    assert_eq!(read(&world.root, LIST_PETS), list_result);
    assert_eq!(after.written.len(), 2);

    let new = plan.op_view("listOwners", &Decisions::default()).unwrap();
    assert!(new.ours.is_empty() && new.base.is_none() && new.hunks.is_empty() && new.result == new.theirs);
    let removed = plan.op_view("showPetById", &Decisions::default()).unwrap();
    assert!(removed.theirs.is_empty() && removed.result == removed.ours && removed.base.is_some());
    assert!(matches!(plan.op_view("nope", &Decisions::default()), Err(SyncError::Unknown(_))));
}

const LOGIN_V1: &str = "openapi: 3.0.0
info: {title: T, version: '1'}
paths:
  /login:
    post:
      operationId: login
      summary: Login
      requestBody:
        content:
          application/x-www-form-urlencoded:
            schema:
              type: object
              properties:
                user: {type: string}
                pass: {type: string}
      responses: {'200': {description: ok}}
";

#[test]
fn ef_syn_05_op_view_locates_method_address_and_form_fields() {
    let login_v2 = LOGIN_V1.replace("/login", "/signin").replace("pass:", "otp:");
    let world = world(LOGIN_V1);
    edit_file(&world.root, "Login.yml", |tree| {
        child(tree, "http").set("method", Value::str("PUT"), HTTP_ORDER);
    });
    let plan = plan_of(&world, &login_v2);
    let view = plan.op_view("login", &Decisions::default()).unwrap();
    let ids: Vec<(&str, Kind)> = plan.operations[0].changes.iter().map(|c| (c.id.as_str(), c.kind)).collect();
    assert_eq!(
        ids,
        [
            ("login::method", Kind::Kept),
            ("login::url", Kind::Applied),
            ("login::body/form/pass", Kind::Applied),
            ("login::body/form/otp", Kind::Applied)
        ]
    );
    let at =
        |id: &str| view.hunks.iter().find(|h| h.change_id == id).unwrap_or_else(|| panic!("{id} : {:?}", view.hunks));
    let line = |text: &str, range: [usize; 2]| text.lines().nth(range[0] - 1).unwrap().to_owned();
    assert_eq!(line(&view.ours, at("login::method").ours.unwrap()), "  method: PUT");
    assert_eq!(line(&view.theirs, at("login::method").theirs.unwrap()), "  method: POST");
    assert_eq!(line(&view.theirs, at("login::url").theirs.unwrap()), "  url: \"{{baseUrl}}/signin\"");
    assert_eq!(line(&view.result, at("login::url").result.unwrap()), "  url: \"{{baseUrl}}/signin\"");
    let otp = at("login::body/form/otp");
    assert_eq!(line(&view.theirs, otp.theirs.unwrap()).trim(), "- name: otp");
    assert!(otp.ours.is_none() && otp.result.is_some());
    let pass = at("login::body/form/pass");
    assert_eq!(line(&view.ours, pass.ours.unwrap()).trim(), "- name: pass");
    assert!(pass.theirs.is_none() && pass.result.is_none(), "retiré par la spec");
}

#[test]
fn ef_syn_06_entries_cannot_point_outside_the_collection_or_into_hidden_folders() {
    let world = world(&v1());
    let source = world.root.join(".oc-sync/openapi/source.yml");
    let text = fs::read_to_string(&source).unwrap();
    for file in ["../outside.yml", ".oc-sync/openapi/source.yml", "pets/../../x.yml"] {
        let hostile = text.replace("pets/List all pets.yml", file);
        write_atomic(&source, &hostile).unwrap();
        let error = sync::plan(&world.root, &v2(), "x".into(), &[]).unwrap_err();
        assert!(error.to_string().contains("chemin"), "{file} : {error}");
    }
    write_atomic(&source, &text.replace("spec: spec.yaml", "spec: ../../etc/passwd")).unwrap();
    assert!(sync::plan(&world.root, &v2(), "x".into(), &[]).is_err());
    write_atomic(&source, &text.replace("groupBy: tags", "groupBy: folders")).unwrap();
    assert!(sync::plan(&world.root, &v2(), "x".into(), &[]).unwrap_err().is_input());
    write_atomic(&source, &text.replace("  - key: createPets\n    file: pets/Create a pet.yml", "  - key: createPets"))
        .unwrap();
    assert!(sync::plan(&world.root, &v2(), "x".into(), &[]).is_err(), "une entrée sans fichier ni ignored est refusée");
}

#[test]
fn ef_syn_06_inputs_are_checked_before_anything_is_compared() {
    let dir = tempfile::tempdir().unwrap();
    let error = sync::plan(dir.path(), &v2(), "x".into(), &[]).unwrap_err();
    assert!(error.is_input() && error.to_string().contains("n'est pas une collection"), "{error}");
    let world = world(&v1());
    let error = sync::plan(&world.root, "{{{ pas du yaml", "x".into(), &[]).unwrap_err();
    assert!(error.is_input(), "{error}");
    let before = files_under(&world.root);
    assert!(sync::plan(&world.root, "", "x".into(), &[]).is_err());
    assert_eq!(files_under(&world.root), before);
}

#[tokio::test]
async fn ef_syn_06_read_source_uses_the_recorded_source_or_the_given_one() {
    let world = world(&v1());
    let (text, value) = sync::read_source(&world.root, None).await.unwrap();
    assert_eq!((text.as_str(), value.as_str()), (v1().as_str(), "../../specs/api.yaml"));
    let other = world.root.parent().unwrap().parent().unwrap().join("specs/other.yaml");
    fs::write(&other, v2()).unwrap();
    let (text, value) = sync::read_source(&world.root, Some(other.to_str().unwrap())).await.unwrap();
    assert_eq!((text, value.as_str()), (v2(), "../../specs/other.yaml"));
    fs::remove_dir_all(world.root.join(".oc-sync")).unwrap();
    let error = sync::read_source(&world.root, None).await.unwrap_err();
    assert!(matches!(error, SyncError::NotConnected) && error.is_input(), "{error}");
}

#[test]
fn enf_comp_02_files_written_by_the_sync_rewrite_without_diff() {
    let world = world(&v1());
    team_works(&world.root);
    plan_of(&world, &v2()).apply(&choice("createPets::body", Choice::Both)).unwrap();
    let info = open_collection(&world.root).unwrap();
    assert_eq!(info.request_count, 6);
    for (path, bytes) in files_under(&world.root) {
        if !path.ends_with(".yml") || path.starts_with("environments/") {
            continue;
        }
        let text = String::from_utf8(bytes).unwrap();
        let blank: &[&str] = if path.ends_with("source.yml") { &[] } else { BLANK_BEFORE };
        assert_eq!(normalize(&text, blank).unwrap(), text, "{path} est renormalisé");
    }
    let deprecated = store::removed_files(&world.root);
    let flat = |items: &[TreeItem]| -> Vec<String> {
        fn collect(items: &[TreeItem], out: &mut Vec<String>) {
            for item in items {
                match item {
                    TreeItem::Folder { children, .. } => collect(children, out),
                    TreeItem::Request { path, .. } => out.push(path.clone()),
                }
            }
        }
        let mut out = Vec::new();
        collect(items, &mut out);
        out
    };
    assert!(flat(&info.items).iter().any(|p| deprecated.contains(p)), "la requête retirée reste dans l'arbre");
}

#[test]
fn ef_syn_02_new_files_land_where_an_import_of_the_new_spec_would_put_them() {
    for group_by in [GroupBy::Tags, GroupBy::Path] {
        let synced = world_grouped(&v1(), group_by);
        plan_of(&synced, &v2()).apply(&Decisions::default()).unwrap();
        let fresh = world_grouped(&v2(), group_by);
        let entries = |world: &World| -> BTreeMap<String, String> {
            let store = store::read(&world.root).unwrap().unwrap();
            store.operations.into_iter().filter_map(|e| e.file.map(|f| (e.key, f))).collect()
        };
        let (synced_files, fresh_files) = (entries(&synced), entries(&fresh));
        for key in ["deletePet", "listPhotos", "listOwners"] {
            assert_eq!(synced_files[key], fresh_files[key], "{key} ({group_by:?})");
            assert!(synced.root.join(&synced_files[key]).is_file());
        }
        let info = open_collection(&synced.root).unwrap();
        assert_eq!(info.request_count, 6, "{group_by:?}");
        assert!(!plan_of(&synced, &v2()).diverged(), "{group_by:?}");
    }
}

const CONNECT_SPEC: &str = "openapi: 3.0.0
info: {title: T, version: '1'}
paths:
  /users/{userId}:
    get: {summary: One user, responses: {'200': {description: ok}}}
    post: {summary: Update user, responses: {'200': {description: ok}}}
  /orders/{orderId}:
    get: {summary: One order, responses: {'200': {description: ok}}}
  /items:
    get: {summary: Items, responses: {'200': {description: ok}}}
  /other:
    get: {summary: Other, responses: {'200': {description: ok}}}
";

#[test]
fn ef_syn_01_urls_are_normalized_before_matching_without_a_base() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("opencollection.yml"), "opencollection: 1.0.0\n\ninfo:\n  name: Team\n").unwrap();
    let request = |name: &str, method: &str, url: &str| {
        let text = format!("info:\n  name: {name}\n  type: http\n\nhttp:\n  method: {method}\n  url: \"{url}\"\n");
        fs::write(root.join(format!("{name}.yml")), text).unwrap();
    };
    request("user", "GET", "{{baseUrl}}/users/:id");
    request("order", "GET", "https://api.example.com/orders/{id}?expand=1");
    request("items", "GET", "{{host}}{{prefix}}/items/");
    request("update", "POST", "http://localhost:8080/users/:uid");
    request("other-get", "GET", "{{baseUrl}}/other");
    request("other-copy", "GET", "{{baseUrl}}/other/");
    let plan = sync::plan(root, CONNECT_SPEC, "spec.yaml".into(), &[]).unwrap();
    let files: Vec<(&str, Option<&str>)> =
        plan.operations.iter().map(|o| (o.key.as_str(), o.file.as_deref())).collect();
    assert_eq!(
        files,
        [
            ("GET /users/{}", Some("user.yml")),
            ("POST /users/{}", Some("update.yml")),
            ("GET /orders/{}", Some("order.yml")),
            ("GET /items", Some("items.yml")),
            ("GET /other", None),
        ],
        "un rapprochement ambigu (deux requêtes pour GET /other) n'est pas fait"
    );
    assert_eq!(op(&plan, "GET /other").status, OpStatus::New);
    assert_eq!(
        op(&plan, "GET /orders/{}").status,
        OpStatus::Conflict,
        "sans base, deux adresses différentes sont un conflit"
    );
}

fn generated(count: usize, version: &str, suffix: &str) -> String {
    let mut spec = format!("openapi: 3.0.0\ninfo: {{title: Générée, version: '{version}'}}\npaths:\n");
    for i in 0..count {
        let tag = format!("tag{}", i % 10);
        spec.push_str(&format!(
            "  /r{i}/items/{{id}}:
    get:
      operationId: get{i}
      summary: Lire {i}
      tags: [{tag}]
      parameters:
        - {{name: id, in: path, required: true, schema: {{type: string}}}}
        - {{name: limit, in: query, required: true, description: 'Limite{suffix}', schema: {{type: integer}}}}
      responses: {{'200': {{description: ok}}}}
    post:
      operationId: post{i}
      summary: Écrire {i}
      tags: [{tag}]
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                nom: {{type: string}}
                valeur: {{type: integer}}
      responses: {{'201': {{description: ok}}}}
"
        ));
    }
    spec
}

#[test]
fn enf_perf_07_plan_and_apply_of_500_operations() {
    let (before, after) = (generated(225, "1", ""), generated(250, "2", " (v2)"));
    let world = world(&before);
    assert_eq!(sync::status(&world.root).unwrap().operation_count, 450);
    for i in (0..225).step_by(5) {
        let file = format!("tag{}/Lire {i}.yml", i % 10);
        edit_file(&world.root, &file, |tree| {
            let Some(Value::Seq(params)) = child(tree, "http").get_mut("params") else { panic!("params absents") };
            for param in params.iter_mut().filter_map(|p| if let Value::Map(m) = p { Some(m) } else { None }) {
                if param.str("name") == Some("limit") {
                    param.set("value", Value::str("10"), &[]);
                }
            }
        });
    }

    let start = Instant::now();
    let plan = plan_of(&world, &after);
    let planned = start.elapsed();
    assert_eq!(plan.operations.len(), 500);
    assert_eq!((plan.summary.updated, plan.summary.merged, plan.summary.created), (180, 45, 50));
    let report = plan.apply(&Decisions::default()).unwrap();
    let total = start.elapsed();
    assert_eq!((report.written.len(), report.created.len()), (225, 50));
    let again = Instant::now();
    let unchanged = plan_of(&world, &after);
    eprintln!(
        "ENF-PERF-07 : plan {planned:?}, plan + application {total:?} pour 500 opérations, plan sans changement {:?}",
        again.elapsed()
    );
    assert!(!unchanged.diverged());
    let budget = if cfg!(debug_assertions) { 10.0 } else { 2.0 };
    assert!(total.as_secs_f64() < budget, "plan + application : {total:?} (budget {budget} s)");
}

const PETS: &str = "openapi: 3.0.0
info: {title: T, version: 1.0.0}
servers: [{url: http://api.test/v1}]
paths:
  /pets:
    get:
      summary: List pets
      operationId: listPets
      tags: [pets]
      parameters:
        - {name: limit, in: query, description: \"How many\", schema: {type: integer}}
        - {name: X-Req, in: header, description: \"corr\", schema: {type: string}}
      responses: {'200': {description: ok}}
    post:
      summary: Create pet
      operationId: createPet
      tags: [pets]
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                id: {type: integer}
                name: {type: string}
      responses: {'201': {description: ok}}
  /pets/{petId}:
    get:
      summary: Show pet
      operationId: showPet
      tags: [pets]
      parameters:
        - {name: petId, in: path, required: true, schema: {type: string}}
      security:
        - oauth: [read]
      responses: {'200': {description: ok}}
components:
  securitySchemes:
    oauth:
      type: oauth2
      flows:
        clientCredentials:
          tokenUrl: https://auth.test/token
          scopes: {read: r}
";

const PETS_LIST: &str = "pets/List pets.yml";
const PETS_SHOW: &str = "pets/Show pet.yml";

fn replace_in(world: &World, relative: &str, from: &str, to: &str) {
    let text = read(&world.root, relative);
    assert!(text.contains(from), "{relative} ne contient pas {from:?} :\n{text}");
    fs::write(world.root.join(relative), text.replacen(from, to, 1)).unwrap();
}

fn pets_with_header() -> String {
    PETS.replace(
        "{name: X-Req, in: header, description: \"corr\", schema: {type: string}}",
        "{name: X-Req, in: header, description: \"corr\", schema: {type: string}}\n        - {name: X-New, in: header, schema: {type: string}}",
    )
}

fn pets_without_show() -> String {
    let (start, end) = (PETS.find("  /pets/{petId}:").unwrap(), PETS.find("components:").unwrap());
    format!("{}{}", &PETS[..start], &PETS[end..])
}

fn source_keys(root: &Path) -> Vec<String> {
    let text = read(root, ".oc-sync/openapi/source.yml");
    text.lines().filter_map(|l| l.strip_prefix("  - key: ")).map(|k| k.trim_matches('"').to_owned()).collect()
}

#[test]
fn ef_syn_04_an_untyped_auth_changed_by_the_team_or_the_spec_is_never_overwritten_silently() {
    let world = world(PETS);
    replace_in(&world, PETS_SHOW, "clientId: \"{{oauth_client_id}}\"", "clientId: real-team-client-id");
    let token_url = PETS.replace("tokenUrl: https://auth.test/token", "tokenUrl: https://auth.test/v2/token");
    let plan = plan_of(&world, &token_url);
    assert_eq!(op(&plan, "showPet").status, OpStatus::Conflict, "les deux ont changé la même auth");
    let change = &op(&plan, "showPet").changes[0];
    assert_eq!((change.id.as_str(), change.kind), ("showPet::auth", Kind::Conflict));
    assert!(change.ours.as_deref().is_some_and(|s| s.starts_with("oauth2\n") && s.contains("real-team-client-id")));
    assert!(change.theirs.as_deref().is_some_and(|s| s.contains("auth.test/v2/token")));
    let before = files_under(&world.root);
    assert!(plan.apply(&Decisions::default()).is_err(), "un conflit sans choix est refusé");
    assert_eq!(files_under(&world.root), before);
    plan.apply(&Decisions::uniform(&plan, Choice::Team)).unwrap();
    let kept = read(&world.root, PETS_SHOW);
    assert!(kept.contains("clientId: real-team-client-id") && kept.contains("accessTokenUrl: https://auth.test/token"));

    let world = self::world(PETS);
    replace_in(&world, PETS_SHOW, "clientId: \"{{oauth_client_id}}\"", "clientId: real-team-client-id");
    let unchanged_by_spec = plan_of(&world, &PETS.replace("version: 1.0.0", "version: 1.0.1"));
    assert_eq!(op(&unchanged_by_spec, "showPet").status, OpStatus::Kept, "la config de l'équipe est conservée");

    let world = self::world(PETS);
    let plan = plan_of(&world, &token_url);
    assert_eq!(op(&plan, "showPet").status, OpStatus::Updated, "l'équipe n'a rien changé : la spec s'applique");
    plan.apply(&Decisions::default()).unwrap();
    assert!(read(&world.root, PETS_SHOW).contains("accessTokenUrl: https://auth.test/v2/token"));
}

#[test]
fn ef_syn_04_numbers_of_the_team_body_keep_their_tokens_through_the_merge() {
    let world = world(PETS);
    let body = "      {\n        \"id\": 12345678901234567890123,\n        \"price\": 0.30000000000000004,\n        \"qty\": 1.50,\n        \"big\": 1E3,\n        \"name\": \"Rex\"\n      }";
    replace_in(&world, "pets/Create pet.yml", "      {\n        \"id\": 0,\n        \"name\": \"\"\n      }", body);
    let spec = PETS.replace(
        "                name: {type: string}\n",
        "                name: {type: string}\n                tag: {type: string}\n",
    );
    let plan = plan_of(&world, &spec);
    assert_eq!(op(&plan, "createPet").status, OpStatus::Merged);
    plan.apply(&Decisions::default()).unwrap();
    let written = read(&world.root, "pets/Create pet.yml");
    for token in ["12345678901234567890123", "0.30000000000000004", "1.50", "1E3", "\"tag\": \"\""] {
        assert!(written.contains(token), "{token} absent :\n{written}");
    }
}

#[test]
fn ef_syn_04_unknown_keys_inside_the_rewritten_lists_survive_the_sync() {
    let world = world(PETS);
    replace_in(
        &world,
        PETS_LIST,
        "      description: corr\n      disabled: true\n",
        "      description: corr\n      disabled: true\n      x-team-note: important\n",
    );
    replace_in(
        &world,
        PETS_LIST,
        "      description: How many\n      disabled: true\n",
        "      description: How many\n      disabled: true\n      x-param-note: keep me\n",
    );
    let plan = plan_of(&world, &pets_with_header());
    assert_eq!(op(&plan, "listPets").status, OpStatus::Updated);
    plan.apply(&Decisions::default()).unwrap();
    let written = read(&world.root, PETS_LIST);
    assert!(written.contains("x-team-note: important") && written.contains("x-param-note: keep me"), "{written}");
    assert!(written.contains("    - name: X-New\n"), "{written}");
}

#[test]
fn ef_syn_04_a_hand_typed_query_survives_a_new_query_parameter() {
    let world = world(PETS);
    replace_in(&world, PETS_LIST, "url: \"{{baseUrl}}/pets\"", "url: \"{{baseUrl}}/pets?debug=1&token={{tok}}\"");
    let spec = PETS.replace(
        "{name: limit, in: query, description: \"How many\", schema: {type: integer}}",
        "{name: limit, in: query, description: \"How many\", schema: {type: integer}}\n        - {name: offset, in: query, required: true, schema: {type: integer, default: 5}}",
    );
    plan_of(&world, &spec).apply(&Decisions::default()).unwrap();
    let doc = read_request(&world.root, PETS_LIST).unwrap();
    assert_eq!(doc.url, "{{baseUrl}}/pets?debug=1&token={{tok}}&offset=5");
}

#[test]
fn ef_syn_04_a_path_param_value_typed_by_the_team_follows_a_renamed_segment() {
    let world = world(PETS);
    replace_in(&world, PETS_SHOW, "      value: \"\"\n      type: path", "      value: \"42\"\n      type: path");
    let renamed =
        PETS.replace("/pets/{petId}:", "/pets/{id}:").replace("{name: petId, in: path", "{name: id, in: path");
    let plan = plan_of(&world, &renamed);
    assert_eq!(op(&plan, "showPet").status, OpStatus::Merged);
    plan.apply(&Decisions::default()).unwrap();
    let doc = read_request(&world.root, PETS_SHOW).unwrap();
    assert_eq!(doc.url, "{{baseUrl}}/pets/:id");
    assert_eq!((doc.params[0].name.as_str(), doc.params[0].value.as_str()), ("id", "42"));
}

#[test]
fn ef_syn_04_a_path_param_value_typed_by_the_team_is_a_conflict_when_its_segment_disappears() {
    let world = world(PETS);
    replace_in(&world, PETS_SHOW, "      value: \"\"\n      type: path", "      value: \"42\"\n      type: path");
    let spec = PETS
        .replace("/pets/{petId}:", "/pets/by-id:")
        .replace("        - {name: petId, in: path, required: true, schema: {type: string}}\n", "");
    let plan = plan_of(&world, &spec);
    let show = op(&plan, "showPet");
    assert_eq!(show.status, OpStatus::Conflict);
    let conflict = show.changes.iter().find(|c| c.id == "showPet::param/path/petId").expect("conflit visible");
    assert_eq!((conflict.kind, conflict.choices.clone()), (Kind::Conflict, vec![Choice::Team, Choice::Spec]));
    assert_eq!(conflict.ours.as_deref(), Some("42"));
    plan.apply(&choice("showPet::param/path/petId", Choice::Spec)).unwrap();
    assert!(read_request(&world.root, PETS_SHOW).unwrap().params.is_empty());

    let world = self::world(PETS);
    plan_of(&world, &spec).apply(&Decisions::default()).unwrap();
    assert!(
        read_request(&world.root, PETS_SHOW).unwrap().params.is_empty(),
        "sans valeur saisie : retiré sans conflit"
    );
}

#[test]
fn ef_syn_01_a_renamed_operation_id_takes_over_the_untouched_file() {
    let world = world(PETS);
    let renamed = PETS.replace("operationId: listPets", "operationId: listAllPets");
    let plan = plan_of(&world, &renamed);
    assert_eq!((op(&plan, "listAllPets").status, op(&plan, "listPets").status), (OpStatus::New, OpStatus::Removed));
    let before = files_under(&world.root);
    let report = plan.apply(&Decisions::default()).unwrap();
    assert!(report.created.is_empty() && report.removed.is_empty(), "rien n'est créé ni retiré : {report:?}");
    assert_eq!(source_keys(&world.root), ["listAllPets", "createPet", "showPet"]);
    assert!(store::removed_files(&world.root).is_empty(), "le fichier vivant n'est pas dépréciée");
    let after = files_under(&world.root);
    assert_eq!(
        after.keys().filter(|k| k.starts_with("pets/")).collect::<Vec<_>>(),
        before.keys().filter(|k| k.starts_with("pets/")).collect::<Vec<_>>(),
        "aucun fichier de requête créé"
    );
    assert!(!after.keys().any(|k| k.contains("removed/")), "{:?}", after.keys().collect::<Vec<_>>());
    let again = plan_of(&world, &renamed);
    assert!(!again.diverged(), "{:?}", statuses(&again));
    assert_eq!(op(&again, "listAllPets").file.as_deref(), Some(PETS_LIST));
}

#[test]
fn ef_syn_01_a_renamed_operation_id_does_not_take_over_a_file_the_team_edited() {
    let world = world(PETS);
    replace_in(&world, PETS_LIST, "name: List pets", "name: List pets (equipe)");
    let renamed = PETS
        .replace("operationId: listPets", "operationId: listAllPets")
        .replace("summary: List pets", "summary: List pets (equipe)");
    let plan = plan_of(&world, &renamed);
    let report = plan.apply(&Decisions::default()).unwrap();
    assert_eq!(report.removed, [PETS_LIST]);
    assert_eq!(report.created, ["pets/List pets (equipe).yml"], "le fichier de l'équipe garde son ancienne opération");
    assert!(store::removed_files(&world.root).contains(PETS_LIST));
}

#[test]
fn ef_syn_01_a_new_operation_never_reuses_a_file_another_operation_tracks() {
    let world = world(PETS);
    let start = PETS.find("  /pets:").unwrap();
    let end = PETS.find("    post:").unwrap();
    let copy = PETS[start..end].replace("  /pets:", "  /pets2:").replace("listPets", "listPets2");
    let spec = PETS.replacen("components:", &format!("{copy}components:"), 1);
    let created = plan_of(&world, &spec).apply(&Decisions::default()).unwrap().created;
    assert_eq!(created.len(), 1);
    let twin = &created[0];

    let store = world.root.join(".oc-sync/openapi/source.yml");
    let text = read(&world.root, ".oc-sync/openapi/source.yml");
    let shared = text
        .replace(&format!("  - key: listPets2\n    file: {twin}\n"), "")
        .replace("key: listPets\n    file: pets/List pets.yml", &format!("key: listPets\n    file: {twin}"));
    assert_ne!(shared, text);
    fs::write(&store, shared).unwrap();
    let plan = plan_of(&world, &spec);
    assert_eq!(op(&plan, "listPets2").status, OpStatus::New, "{:?}", statuses(&plan));
    let report = plan.apply(&Decisions::default()).unwrap();
    assert_eq!(report.created, [twin.replace(".yml", " 1.yml")], "le fichier suivi par listPets n'est pas partagé");
    let source = read(&world.root, ".oc-sync/openapi/source.yml");
    assert!(source.contains(&format!("key: listPets\n    file: {twin}\n")), "{source}");
    assert!(source.contains(&format!("key: listPets2\n    file: {}\n", twin.replace(".yml", " 1.yml"))), "{source}");
}

#[test]
fn ef_syn_01_a_moved_or_renamed_file_is_found_again_by_method_and_path() {
    let world = world(PETS);
    fs::create_dir_all(world.root.join("mes-trucs")).unwrap();
    fs::rename(world.root.join(PETS_LIST), world.root.join("mes-trucs/Ma liste.yml")).unwrap();
    let plan = plan_of(&world, &pets_with_header());
    assert_eq!(plan.summary.missing, 0);
    assert_eq!(op(&plan, "listPets").status, OpStatus::Updated);
    assert_eq!(op(&plan, "listPets").file.as_deref(), Some("mes-trucs/Ma liste.yml"));
    let report = plan.apply(&Decisions::default()).unwrap();
    assert_eq!(report.written, ["mes-trucs/Ma liste.yml"]);
    assert!(report.ignored.is_empty() && report.created.is_empty());
    assert!(
        read(&world.root, ".oc-sync/openapi/source.yml").contains("key: listPets\n    file: mes-trucs/Ma liste.yml\n")
    );
    let again = plan_of(&world, &pets_with_header());
    assert!(!again.diverged(), "{:?}", statuses(&again));

    let folder = self::world(PETS);
    fs::rename(folder.root.join("pets"), folder.root.join("animaux")).unwrap();
    let plan = plan_of(&folder, PETS);
    assert_eq!((plan.summary.missing, plan.summary.unchanged), (0, 3), "{:?}", statuses(&plan));
    assert_eq!(op(&plan, "showPet").file.as_deref(), Some("animaux/Show pet.yml"));
}

#[test]
fn ef_syn_01_a_missing_file_stays_missing_when_the_match_is_not_certain() {
    let world = world(PETS);
    fs::rename(world.root.join(PETS_LIST), world.root.join("Ma liste.yml")).unwrap();
    fs::copy(world.root.join("Ma liste.yml"), world.root.join("Autre copie.yml")).unwrap();
    let plan = plan_of(&world, PETS);
    assert_eq!(op(&plan, "listPets").status, OpStatus::Missing, "deux candidates : pas de rapprochement");
    assert!(plan.diverged(), "une opération manquante est un écart");

    let world = self::world(PETS);
    fs::rename(world.root.join(PETS_LIST), world.root.join("Ma liste.yml")).unwrap();
    replace_in(&world, "Ma liste.yml", "url: \"{{baseUrl}}/pets\"", "url: \"{{baseUrl}}/autre\"");
    let plan = plan_of(&world, PETS);
    assert_eq!(op(&plan, "listPets").status, OpStatus::Missing, "méthode ou chemin différents");

    let world = self::world(PETS);
    fs::remove_file(world.root.join(PETS_LIST)).unwrap();
    let plan = plan_of(&world, PETS);
    assert_eq!((plan.summary.missing, plan.diverged()), (1, true));
}

#[test]
fn ef_syn_06_a_spec_without_operations_is_refused_while_operations_are_tracked() {
    let world = world(PETS);
    let before = files_under(&world.root);
    let empty = "openapi: 3.0.0\ninfo: {title: T, version: 1.0.0}\npaths: {}\n";
    let error = sync::plan(&world.root, empty, recorded(&world.root), &[]).unwrap_err();
    assert!(error.is_input() && error.to_string().contains("aucune opération"), "{error}");
    assert!(error.to_string().contains("3 opération"), "{error}");
    assert_eq!(files_under(&world.root), before);

    let bare = self::world(empty);
    assert!(sync::plan(&bare.root, empty, recorded(&bare.root), &[]).is_ok(), "rien n'est suivi : rien n'est perdu");
}

#[test]
fn ef_syn_03_a_restored_operation_is_merged_against_the_version_kept_when_it_was_removed() {
    let world = world(PETS);
    let imported = read(&world.root, PETS_SHOW);
    plan_of(&world, &pets_without_show()).apply(&Decisions::default()).unwrap();
    let kept = world.root.join(".oc-sync/openapi/removed");
    let files: Vec<_> =
        fs::read_dir(&kept).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(files, ["showPet.yml"]);
    assert_eq!(fs::read_to_string(kept.join("showPet.yml")).unwrap(), imported, "dernière version de la requête");

    let bearer = PETS.replace(
        "oauth2\n      flows:\n        clientCredentials:\n          tokenUrl: https://auth.test/token\n          scopes: {read: r}",
        "http\n      scheme: bearer",
    );
    let plan = plan_of(&world, &bearer);
    let show = op(&plan, "showPet");
    assert_eq!(show.status, OpStatus::Restored, "aucun faux conflit : {:?}", show.changes);
    assert_eq!((show.changes[0].id.as_str(), show.changes[0].kind), ("showPet::auth", Kind::Applied));
    assert_eq!(show.changes[0].base.as_deref().map(|b| b.starts_with("oauth2")), Some(true));
    plan.apply(&Decisions::default()).unwrap();
    assert!(read(&world.root, PETS_SHOW).contains("type: bearer"));
    assert!(!kept.join("showPet.yml").exists(), "supprimée après la restauration");
    assert!(!plan_of(&world, &bearer).diverged());

    let lost = self::world(PETS);
    plan_of(&lost, &pets_without_show()).apply(&Decisions::default()).unwrap();
    fs::remove_file(lost.root.join(".oc-sync/openapi/removed/showPet.yml")).unwrap();
    let plan = plan_of(&lost, &bearer);
    assert_eq!(op(&plan, "showPet").status, OpStatus::Conflict, "sans base, conflit au moindre doute");
}

#[test]
fn ef_syn_03_the_kept_version_of_a_removed_operation_follows_its_key_and_goes_when_it_is_forgotten() {
    let world = world(USERS_V1);
    let dir = world.root.join(".oc-sync/openapi/removed");
    let gone = USERS_V1
        .replace("  /health:\n    get:\n      summary: Health\n      responses: {'200': {description: ok}}\n", "");
    plan_of(&world, &gone).apply(&Decisions::default()).unwrap();
    let names: Vec<_> =
        fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, ["GET %2Fhealth.yml"], "la clé, caractères interdits écrits %XX");
    let health = store::removed_files(&world.root).into_iter().next().unwrap();
    fs::remove_file(world.root.join(&health)).unwrap();
    plan_of(&world, &gone).apply(&Decisions::default()).unwrap();
    assert_eq!(fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0), 0, "l'opération oubliée n'a plus de copie");
}

fn many(count: usize, folder: &str) -> String {
    let mut spec = String::from("openapi: 3.0.0\ninfo: {title: Large, version: 1.0.0}\npaths:\n");
    for i in 0..count {
        spec.push_str(&format!(
            "  /{folder}/r{i}/items/{{id}}:\n    get:\n      summary: Item {i}\n      tags: [t{}]\n      responses: {{'200': {{description: ok}}}}\n",
            i % 20
        ));
    }
    spec
}

#[test]
fn ef_syn_01_pairing_suggestions_are_one_per_operation_and_bounded() {
    let world = world(&many(500, "v1"));
    let plan = plan_of(&world, &many(500, "v2"));
    assert_eq!((plan.summary.removed, plan.summary.created), (500, 500));
    assert_eq!(plan.suggestions.len(), 50, "plafonné");
    let (mut removed, mut added): (Vec<_>, Vec<_>) =
        plan.suggestions.iter().map(|s| (s.removed.as_str(), s.added.as_str())).unzip();
    removed.sort_unstable();
    added.sort_unstable();
    assert!(removed.windows(2).all(|w| w[0] != w[1]) && added.windows(2).all(|w| w[0] != w[1]), "appariement 1-1");
    for s in &plan.suggestions {
        assert_eq!(s.reason, "même méthode et même nom de requête");
        assert_eq!(s.removed.replace("/v1/", "/v2/"), s.added, "la meilleure candidate : le même nom");
    }
    assert!(serde_json::to_string(&plan).unwrap().len() < 1 << 20, "quelques Ko, pas des dizaines de Mo");

    let small = self::world(&many(3, "v1"));
    let plan = plan_of(&small, &many(2, "v2"));
    assert_eq!(plan.suggestions.len(), 2, "une candidate par opération retirée, une opération retirée par candidate");
}

#[test]
fn ef_syn_06_entries_cannot_target_reserved_files_or_share_one() {
    let world = world(PETS);
    let source = world.root.join(".oc-sync/openapi/source.yml");
    let text = fs::read_to_string(&source).unwrap();
    for file in [
        "opencollection.yml",
        "pets/folder.yml",
        "environments/Environment 1.yml",
        "pets/node_modules/x.yml",
        "./.hidden.yml",
    ] {
        let hostile = text.replace(PETS_LIST, file);
        fs::write(&source, &hostile).unwrap();
        let error = sync::plan(&world.root, &pets_with_header(), "x".into(), &[]).unwrap_err();
        assert!(error.is_input() && error.to_string().contains("chemin refusé"), "{file} : {error}");
    }
    let shared = text.replace(
        "  - key: createPet\n    file: pets/Create pet.yml",
        "  - key: createPet\n    file: pets/List pets.yml",
    );
    fs::write(&source, shared).unwrap();
    let before = files_under(&world.root);
    let error = sync::plan(&world.root, &pets_with_header(), "x".into(), &[]).unwrap_err();
    assert!(
        error.is_input() && error.to_string().contains("« listPets » et « createPet » désignent le même fichier"),
        "{error}"
    );
    assert_eq!(files_under(&world.root), before);
    let again = text.replace("pets/List pets.yml", "Pets/./list pets.yml").replace(
        "  - key: createPet\n    file: pets/Create pet.yml",
        "  - key: createPet\n    file: pets/List pets.yml",
    );
    fs::write(&source, again).unwrap();
    assert!(sync::plan(&world.root, PETS, "x".into(), &[]).is_err(), "même fichier sous une autre graphie");
}

#[test]
fn ef_syn_06_apply_only_writes_requests_of_type_http() {
    let world = world(PETS);
    replace_in(&world, PETS_LIST, "type: http", "type: graphql");
    let plan = plan_of(&world, &pets_with_header());
    let before = files_under(&world.root);
    let error = plan.apply(&Decisions::default()).unwrap_err();
    assert!(matches!(&error, SyncError::NotARequest(file) if file == PETS_LIST), "{error}");
    assert_eq!(files_under(&world.root), before, "rien n'est écrit, la base n'a pas bougé");
}

#[test]
fn ef_syn_06_an_unreadable_team_file_is_named_and_a_lost_base_copy_means_no_base() {
    let world = world(PETS);
    fs::write(world.root.join("pets/Create pet.yml"), "info: [\n").unwrap();
    let error = sync::plan(&world.root, &pets_with_header(), recorded(&world.root), &[]).unwrap_err();
    assert!(error.is_input(), "{error}");
    assert!(error.to_string().starts_with("fichier de l'équipe illisible : pets/Create pet.yml : "), "{error}");

    let lost = self::world(PETS);
    fs::remove_file(lost.root.join(".oc-sync/openapi/spec.yaml")).unwrap();
    let plan = plan_of(&lost, &pets_with_header());
    assert!(!plan.has_base && plan.from.is_none());
    assert_eq!(plan.summary.conflicts, 0);
    assert_eq!(op(&plan, "listPets").status, OpStatus::Updated, "{:?}", statuses(&plan));
    plan.apply(&Decisions::default()).unwrap();
    assert!(lost.root.join(".oc-sync/openapi/spec.yaml").is_file(), "la base est reconstituée");
    assert!(plan_of(&lost, &pets_with_header()).has_base);
}

#[test]
fn ef_syn_06_a_resumed_sync_writes_the_folder_file_that_is_missing() {
    let world = world(PETS);
    let spec = PETS.replace("components:", "  /owners:\n    get:\n      summary: List owners\n      operationId: listOwners\n      tags: [owners]\n      responses: {'200': {description: ok}}\ncomponents:");
    let plan = plan_of(&world, &spec);
    plan.apply(&Decisions::default()).unwrap();
    let folder = read(&world.root, "owners/folder.yml");
    let store_dir = world.root.join(".oc-sync/openapi");
    let (source, copy) =
        (read(&world.root, ".oc-sync/openapi/source.yml"), fs::read(store_dir.join("spec.yaml")).unwrap());
    fs::remove_file(world.root.join("owners/folder.yml")).unwrap();
    fs::remove_file(world.root.join("owners/List owners.yml")).unwrap();
    fs::write(
        store_dir.join("source.yml"),
        source.replace("  - key: listOwners\n    file: owners/List owners.yml\n", ""),
    )
    .unwrap();
    fs::write(store_dir.join("spec.yaml"), copy).unwrap();
    let replay = plan_of(&world, &spec);
    assert_eq!(op(&replay, "listOwners").status, OpStatus::New);
    replay.apply(&Decisions::default()).unwrap();
    assert_eq!(read(&world.root, "owners/folder.yml"), folder, "le dossier existant reçoit son folder.yml");
    assert!(world.root.join("owners/List owners.yml").is_file());

    fs::write(world.root.join("owners/folder.yml"), "info:\n  name: Équipe\n  type: folder\n").unwrap();
    let again = plan_of(&world, &spec.replace("operationId: listOwners", "operationId: listAllOwners"));
    again.apply(&Decisions::default()).unwrap();
    assert!(
        read(&world.root, "owners/folder.yml").contains("name: Équipe"),
        "un folder.yml existant n'est jamais écrasé"
    );
}

#[test]
fn ef_syn_05_op_view_locates_ranges_in_files_indented_by_hand() {
    let world = world(PETS);
    let four = "info:\n  name: List pets\n  type: http\n  seq: 1\nhttp:\n    method: GET\n    url: \"{{baseUrl}}/pets\"\n    headers:\n        - name: X-Req\n          value: team\n          description: corr\n    params:\n        - name: limit\n          value: ''\n          type: query\n          description: team desc\n          disabled: true\n    auth: inherit\n";
    fs::write(world.root.join(PETS_LIST), four).unwrap();
    let spec = PETS.replace(
        "{name: limit, in: query, description: \"How many\"",
        "{name: limit, in: query, description: \"spec desc\"",
    );
    let plan = plan_of(&world, &spec);
    let view = plan.op_view("listPets", &Decisions::uniform(&plan, Choice::Team)).unwrap();
    let hunk = view.hunks.iter().find(|h| h.change_id == "listPets::param/query/limit").unwrap();
    assert_eq!(lines(&view.ours, hunk.ours.unwrap())[0], "        - name: limit");
    assert_eq!(lines(&view.ours, hunk.ours.unwrap()).len(), 5);
    let header = view.hunks.iter().find(|h| h.change_id == "listPets::header/x-req").unwrap();
    assert_eq!(lines(&view.ours, header.ours.unwrap())[0], "        - name: X-Req");

    let flat = "info:\n  name: List pets\n  type: http\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/pets\"\n  params:\n  - name: limit\n    value: ''\n    type: query\n    description: team desc\n    disabled: true\n  auth: inherit\n";
    fs::write(world.root.join(PETS_LIST), flat).unwrap();
    let plan = plan_of(&world, &spec);
    let view = plan.op_view("listPets", &Decisions::uniform(&plan, Choice::Team)).unwrap();
    let hunk = view.hunks.iter().find(|h| h.change_id == "listPets::param/query/limit").unwrap();
    assert_eq!(
        lines(&view.ours, hunk.ours.unwrap()),
        ["  - name: limit", "    value: ''", "    type: query", "    description: team desc", "    disabled: true"]
    );
}

#[test]
fn ef_syn_01_a_shared_operation_id_keeps_each_file_on_its_own_operation() {
    let both = "openapi: 3.0.0
info: {title: D, version: 1.0.0}
paths:
  /a:
    get: {summary: A, operationId: dup, tags: [t], responses: {'200': {description: ok}}}
  /b:
    get: {summary: B, operationId: dup, tags: [t], responses: {'200': {description: ok}}}
";
    let world = world(both);
    assert_eq!(source_keys(&world.root), ["dup (GET /a)", "dup (GET /b)"]);
    let swapped = both.replace("/a:", "/tmp:").replace("/b:", "/a:").replace("/tmp:", "/b:");
    let before = files_under(&world.root);
    let plan = plan_of(&world, &swapped);
    assert!(plan.operations.iter().all(|o| o.status == OpStatus::Unchanged), "{:?}", statuses(&plan));
    plan.apply(&Decisions::default()).unwrap();
    assert_eq!(read(&world.root, "t/A.yml"), String::from_utf8(before["t/A.yml"].clone()).unwrap());
    assert_eq!(read_request(&world.root, "t/B.yml").unwrap().url, "{{baseUrl}}/b");

    let only_b = "openapi: 3.0.0\ninfo: {title: D, version: 1.0.0}\npaths:\n  /b:\n    get: {summary: B, operationId: dup, tags: [t], responses: {'200': {description: ok}}}\n";
    let plan = plan_of(&world, only_b);
    assert_eq!(op(&plan, "dup (GET /a)").status, OpStatus::Removed, "/a n'est plus dans la spec");
    assert_eq!(op(&plan, "dup (GET /a)").file.as_deref(), Some("t/A.yml"));
    assert!(plan.operations.iter().all(|o| o.file.as_deref() != Some("t/A.yml") || o.status == OpStatus::Removed));
}

#[cfg(unix)]
#[test]
fn ef_syn_06_a_symbolic_link_of_the_collection_stays_a_link_and_its_target_is_updated() {
    let world = world(PETS);
    std::os::unix::fs::symlink(world.root.join(PETS_LIST), world.root.join("pets/alias.yml")).unwrap();
    let source = read(&world.root, ".oc-sync/openapi/source.yml");
    fs::write(world.root.join(".oc-sync/openapi/source.yml"), source.replace(PETS_LIST, "pets/alias.yml")).unwrap();
    plan_of(&world, &pets_with_header()).apply(&Decisions::default()).unwrap();
    assert!(fs::symlink_metadata(world.root.join("pets/alias.yml")).unwrap().file_type().is_symlink());
    assert!(read(&world.root, PETS_LIST).contains("X-New"));
}

#[cfg(unix)]
#[test]
fn enf_sec_01_the_store_never_writes_through_a_symbolic_link() {
    let world = world(PETS);
    let outside = world._dir.path().join("outside.txt");
    fs::write(&outside, "intact").unwrap();
    let copy = world.root.join(".oc-sync/openapi/spec.yaml");
    fs::remove_file(&copy).unwrap();
    std::os::unix::fs::symlink(&outside, &copy).unwrap();
    let plan = sync::plan(&world.root, &pets_with_header(), recorded(&world.root), &[]);
    assert!(plan.is_err(), "la copie brute qui sort du stockage n'est pas lue comme une spec");
    fs::write(&outside, PETS).unwrap();
    plan_of(&world, &pets_with_header()).apply(&Decisions::default()).unwrap();
    assert_eq!(fs::read_to_string(&outside).unwrap(), PETS, "la cible du lien n'est pas modifiée");
    assert!(!fs::symlink_metadata(&copy).unwrap().file_type().is_symlink());
    assert!(read(&world.root, ".oc-sync/openapi/spec.yaml").contains("X-New"));
}

#[cfg(unix)]
#[test]
fn enf_sec_01_the_kept_versions_are_never_written_or_pruned_through_a_link_or_among_foreign_files() {
    let world = world(PETS);
    let outside = world._dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("precious.yml"), "intact").unwrap();
    std::os::unix::fs::symlink(&outside, world.root.join(".oc-sync/openapi/removed")).unwrap();
    plan_of(&world, &pets_with_header()).apply(&Decisions::default()).unwrap();
    assert_eq!(
        fs::read_to_string(outside.join("precious.yml")).unwrap(),
        "intact",
        "rien n'est supprimé hors du stockage"
    );
    let error = plan_of(&world, &pets_without_show()).apply(&Decisions::default()).unwrap_err();
    assert!(error.to_string().contains("lien symbolique"), "{error}");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1, "rien n'est écrit hors du stockage");

    let world = self::world(PETS);
    let dir = world.root.join(".oc-sync/openapi/removed");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("notes.txt"), "à moi").unwrap();
    fs::write(dir.join("stale.yml"), "plus utile").unwrap();
    plan_of(&world, &pets_with_header()).apply(&Decisions::default()).unwrap();
    assert!(dir.join("notes.txt").is_file() && !dir.join("stale.yml").exists());
}
