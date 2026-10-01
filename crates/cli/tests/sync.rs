use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../sync/tests/fixtures")
}

fn xc(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xc")).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

struct Collection {
    _dir: tempfile::TempDir,
    root: PathBuf,
    v1: String,
    v2: String,
}

fn collection() -> Collection {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    let v1 = dir.path().join("api.yaml");
    fs::copy(fixtures().join("openapi/specs/oai-petstore-3.0.yaml"), &v1).unwrap();
    let v2 = dir.path().join("api-v2.yaml");
    fs::copy(fixtures().join("sync/petstore-v2.yaml"), &v2).unwrap();
    let (code, stdout, stderr) = xc(&["import", v1.to_str().unwrap(), out.to_str().unwrap()]);
    assert_eq!(code, 0, "{stderr}");
    Collection {
        root: PathBuf::from(stdout.trim()),
        v1: v1.to_str().unwrap().to_owned(),
        v2: v2.to_str().unwrap().to_owned(),
        _dir: dir,
    }
}

impl Collection {
    fn path(&self) -> &str {
        self.root.to_str().unwrap()
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.root.join(relative)).unwrap()
    }

    fn team_edits_the_body(&self) {
        let path = self.root.join("pets/Create a pet.yml");
        let text = fs::read_to_string(&path).unwrap();
        fs::write(&path, text.replace("\"tag\": \"\"", "\"tag\": \"friendly\"")).unwrap();
    }

    fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().flatten().collect();
            entries.sort_by_key(|e| e.path());
            for entry in entries {
                let path = entry.path();
                if path.is_dir() {
                    walk(base, &path, out);
                } else {
                    out.push((path.strip_prefix(base).unwrap().display().to_string(), fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.root, &self.root, &mut out);
        out
    }
}

#[test]
fn ef_syn_07_check_exits_0_when_the_spec_has_not_diverged() {
    let c = collection();
    let (code, stdout, stderr) = xc(&["sync", c.path(), "--check"]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("Spec : Swagger Petstore 1.0.0 → 1.0.0"), "{stdout}");
    assert!(stdout.contains("3 inchangée(s), 0 mise(s) à jour"), "{stdout}");
}

#[test]
fn ef_syn_07_check_exits_1_when_the_spec_has_diverged_and_writes_nothing() {
    let c = collection();
    c.team_edits_the_body();
    let before = c.snapshot();
    let (code, stdout, stderr) = xc(&["sync", c.path(), "--check", "--source", &c.v2]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    assert!(stdout.contains("Spec : Swagger Petstore 1.0.0 → 1.1.0"), "{stdout}");
    assert!(stdout.contains("1 mise(s) à jour") && stdout.contains("1 en conflit (1 champ(s))"), "{stdout}");
    assert!(stdout.contains("3 nouvelle(s), 1 retirée(s)"), "{stdout}");
    assert!(stdout.contains("conflit : Corps — Modifié à la fois par l'équipe et par la spec"), "{stdout}");
    assert!(stdout.contains("retirée     GET /pets/{petId} (pets/Info for a specific pet.yml)"), "{stdout}");
    assert_eq!(c.snapshot(), before, "--check n'écrit rien");
}

#[test]
fn ef_syn_07_check_reads_the_recorded_source() {
    let c = collection();
    fs::copy(&c.v2, c.root.join("../../api.yaml")).unwrap();
    let (code, stdout, _) = xc(&["sync", c.path(), "--check"]);
    assert_eq!(code, 1, "la source enregistrée est relue : {stdout}");
    assert!(stdout.contains("1.0.0 → 1.1.0"), "{stdout}");
}

#[test]
fn ef_syn_07_an_unconnected_collection_needs_a_source() {
    let c = collection();
    fs::remove_dir_all(c.root.join(".oc-sync")).unwrap();
    let (code, _, stderr) = xc(&["sync", c.path(), "--check"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.starts_with("erreur : la collection n'est pas connectée"), "{stderr}");

    let (code, stdout, stderr) = xc(&["sync", c.path(), "--check", "--source", &c.v1]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("collection non connectée, aucune base"), "{stdout}");
    assert!(stdout.contains("3 inchangée(s)"), "{stdout}");
    let (code, stdout, _) = xc(&["sync", c.path(), "--check", "--source", &c.v2]);
    assert_eq!(code, 1, "{stdout}");
}

#[test]
fn ef_syn_07_unreadable_inputs_exit_2() {
    let c = collection();
    let missing = c.root.join("absent.yaml");
    let (code, _, stderr) = xc(&["sync", c.path(), "--check", "--source", missing.to_str().unwrap()]);
    assert_eq!(code, 2, "{stderr}");
    let garbage = c.root.join("../garbage.yaml");
    fs::write(&garbage, "{{{ pas du yaml").unwrap();
    let (code, _, stderr) = xc(&["sync", c.path(), "--check", "--source", garbage.to_str().unwrap()]);
    assert_eq!(code, 2, "{stderr}");
    let nowhere = c.root.join("../nowhere");
    fs::create_dir(&nowhere).unwrap();
    let (code, _, stderr) = xc(&["sync", nowhere.to_str().unwrap(), "--check", "--source", &c.v2]);
    assert_eq!(code, 2, "pas une collection : {stderr}");
    assert!(stderr.contains("n'est pas une collection"), "{stderr}");
}

#[test]
fn ef_syn_07_flags_are_checked_by_the_usage() {
    let c = collection();
    for args in [
        vec!["sync", c.path()],
        vec!["sync", c.path(), "--check", "--apply"],
        vec!["sync", c.path(), "--apply", "--keep-team", "--take-spec"],
        vec!["sync", c.path(), "--check", "--keep-team"],
    ] {
        let (code, _, stderr) = xc(&args);
        assert_eq!(code, 2, "{args:?} : {stderr}");
    }
}

#[test]
fn ef_syn_07_apply_refuses_unarbitrated_conflicts_with_exit_1_and_lists_them() {
    let c = collection();
    c.team_edits_the_body();
    let before = c.snapshot();
    let (code, _, stderr) = xc(&["sync", c.path(), "--apply", "--source", &c.v2]);
    assert_eq!(code, 1, "{stderr}");
    assert!(stderr.contains("refus : 1 conflit(s) à arbitrer"), "{stderr}");
    assert!(stderr.contains("createPets::body  Corps"), "{stderr}");
    assert_eq!(c.snapshot(), before, "rien n'est écrit");
}

#[test]
fn ef_syn_07_apply_keep_team_resolves_every_conflict_for_the_team() {
    let c = collection();
    c.team_edits_the_body();
    let (code, stdout, stderr) = xc(&["sync", c.path(), "--apply", "--keep-team", "--source", &c.v2]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(
        stdout.contains("1 fichier(s) modifié(s), 3 créé(s), 1 marqué(s) retiré(s) de la spec, 0 ignorée(s)"),
        "{stdout}"
    );
    assert!(
        stdout.contains("modifié  pets/List all pets.yml") && stdout.contains("créé     owners/List the owners.yml"),
        "{stdout}"
    );
    assert!(c.read("pets/Create a pet.yml").contains("\"tag\": \"friendly\""));
    assert!(c.read(".oc-sync/openapi/source.yml").contains("removed: true"));
    let (code, stdout, _) = xc(&["sync", c.path(), "--check"]);
    assert_eq!(code, 0, "la collection est à jour : {stdout}");
}

#[test]
fn ef_syn_07_apply_take_spec_resolves_every_conflict_for_the_spec() {
    let c = collection();
    c.team_edits_the_body();
    let (code, stdout, stderr) = xc(&["sync", c.path(), "--apply", "--take-spec", "--source", &c.v2]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let file = c.read("pets/Create a pet.yml");
    let request = file.split("settings:").next().unwrap();
    assert!(request.contains("\"birthday\": \"\"") && !request.contains("friendly"), "{request}");
    let (code, _, _) = xc(&["sync", c.path(), "--check"]);
    assert_eq!(code, 0);
}

#[test]
fn ef_syn_07_apply_without_conflict_needs_no_option() {
    let c = collection();
    let (code, stdout, stderr) = xc(&["sync", c.path(), "--apply", "--source", &c.v2]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains("2 fichier(s) modifié(s), 3 créé(s)"), "{stdout}");
    let (code, stdout, _) = xc(&["check", c.path()]);
    assert_eq!(code, 0, "les fichiers écrits se relisent sans diff : {stdout}");
}

#[test]
fn ef_syn_07_apply_connects_a_collection_that_has_no_base() {
    let c = collection();
    fs::remove_dir_all(c.root.join(".oc-sync")).unwrap();
    let (code, stdout, stderr) = xc(&["sync", c.path(), "--apply", "--keep-team", "--source", &c.v1]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    let source = c.read(".oc-sync/openapi/source.yml");
    assert!(source.contains("  - key: showPetById\n    file: pets/Info for a specific pet.yml\n"), "{source}");
    let (code, _, _) = xc(&["sync", c.path(), "--check"]);
    assert_eq!(code, 0);
}

#[test]
fn ef_syn_07_the_source_can_be_an_url() {
    let c = collection();
    let v2 = fs::read_to_string(&c.v2).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/openapi.yaml", listener.local_addr().unwrap());
    thread::spawn(move || {
        for stream in listener.incoming().take(2) {
            let mut stream = stream.unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf).unwrap();
            write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/yaml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{v2}",
                v2.len()
            )
            .unwrap();
        }
    });
    let (code, stdout, stderr) = xc(&["sync", c.path(), "--check", "--source", &url]);
    assert_eq!(code, 1, "{stdout}{stderr}");
    let (code, stdout, stderr) = xc(&["sync", c.path(), "--apply", "--source", &url]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(c.read(".oc-sync/openapi/source.yml").starts_with(&format!("source: {url}\n")));
}
