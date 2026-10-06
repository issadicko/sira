use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

fn xc(args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xc")).args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn collection() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("opencollection.yml"),
        "opencollection: 1.0.0\n\ninfo:\n  name: Boutique\n\nrequest:\n  variables:\n    - name: baseUrl\n      value: https://api.boutique.test\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("ping.yml"),
        "info:\n  name: Ping\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"{{baseUrl}}/ping\"\n  auth: inherit\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("grpc.yml"),
        "info:\n  name: Grpc\n  type: grpc\n  seq: 2\n\ngrpc:\n  url: localhost:50051\n",
    )
    .unwrap();
    dir
}

fn path(dir: &Path, name: &str) -> String {
    dir.join(name).to_str().unwrap().to_owned()
}

#[test]
fn ef_imp_03_cli_export_prints_the_document_and_lists_what_was_left_out_on_stderr() {
    let dir = collection();
    let root = dir.path().to_str().unwrap();

    let (code, stdout, stderr) = xc(&["export", root, "--format", "openapi"]);
    assert_eq!(code, 0, "{stderr}");
    let spec: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(spec["openapi"], "3.0.3");
    assert_eq!(spec["info"]["title"], "Boutique");
    assert_eq!(spec["servers"][0]["url"], "https://api.boutique.test");
    assert!(spec["paths"]["/ping"]["get"].is_object());
    assert!(stderr.contains("avertissement") && stderr.contains("Grpc"), "{stderr}");

    let (code, stdout, stderr) = xc(&["export", root, "--format", "postman"]);
    assert_eq!(code, 0, "{stderr}");
    let postman: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(postman["info"]["name"], "Boutique");
    assert_eq!(postman["item"][0]["name"], "Ping");
    assert!(stderr.contains("Grpc"), "{stderr}");
}

#[test]
fn ef_imp_03_cli_export_writes_a_new_file_and_never_replaces_one_without_force() {
    let dir = collection();
    let root = dir.path().to_str().unwrap();
    let out = tempfile::tempdir().unwrap();
    let file = path(out.path(), "spec.json");

    let (code, stdout, stderr) = xc(&["export", root, "--format", "openapi", "--output", &file]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout.trim(), file);
    let written = fs::read_to_string(&file).unwrap();
    assert_eq!(serde_json::from_str::<Value>(&written).unwrap()["info"]["title"], "Boutique");

    fs::write(&file, "précieux").unwrap();
    let (code, _, stderr) = xc(&["export", root, "--format", "openapi", "--output", &file]);
    assert_eq!(code, 2);
    assert!(stderr.contains("existe déjà") && stderr.contains("--force"), "{stderr}");
    assert_eq!(fs::read_to_string(&file).unwrap(), "précieux");

    let (code, _, stderr) = xc(&["export", root, "--format", "openapi", "--output", &file, "--force"]);
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(fs::read_to_string(&file).unwrap(), written);
}

#[cfg(unix)]
#[test]
fn ef_imp_03_cli_export_does_not_write_through_a_symbolic_link() {
    let dir = collection();
    let root = dir.path().to_str().unwrap();
    let out = tempfile::tempdir().unwrap();
    let target = out.path().join("cible.json");
    fs::write(&target, "intact").unwrap();
    let link = out.path().join("lien.json");
    std::os::unix::fs::symlink(&target, &link).unwrap();

    let (code, _, stderr) = xc(&["export", root, "--format", "postman", "--output", link.to_str().unwrap(), "--force"]);

    assert_eq!(code, 2);
    assert!(stderr.contains("lien symbolique"), "{stderr}");
    assert_eq!(fs::read_to_string(&target).unwrap(), "intact");
}

#[test]
fn ef_imp_03_cli_export_rejects_an_unknown_format_and_a_folder_that_is_not_a_collection() {
    let dir = collection();
    let (code, _, stderr) = xc(&["export", dir.path().to_str().unwrap(), "--format", "yaml"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("postman") && stderr.contains("openapi"), "{stderr}");

    let empty = tempfile::tempdir().unwrap();
    let (code, _, stderr) = xc(&["export", empty.path().to_str().unwrap(), "--format", "postman"]);
    assert_eq!(code, 2, "{stderr}");
    assert!(stderr.contains("erreur"), "{stderr}");
}
