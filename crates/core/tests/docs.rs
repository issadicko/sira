use std::fs;
use std::path::Path;

use xc_core::{read_docs, read_request, save_docs, save_request};

fn write(root: &Path, rel: &str, text: &str) {
    let path = root.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn sample() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, "opencollection.yml", "opencollection: 1.0.0\n\ninfo:\n  name: Boutique\n\nextensions:\n  bruno:\n    ignore:\n      - node_modules\n");
    write(
        root,
        "clients/folder.yml",
        "info:\n  name: Clients\n  type: folder\n  seq: 1\n\ndocs:\n  content: Les clients\n  type: text/markdown\n",
    );
    write(root, "clients/liste.yml", "info:\n  name: Liste\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: https://x.test/clients\n\ndocs:\n  content: |-\n    # Liste\n\n    Renvoie les clients.\n  type: text/markdown\n");
    dir
}

#[test]
fn ef_doc_01_01_reads_the_docs_of_a_folder_written_as_a_table() {
    let dir = sample();
    assert_eq!(read_docs(dir.path(), "clients").unwrap(), "Les clients");
}

#[test]
fn ef_doc_01_02_a_missing_docs_key_or_folder_file_reads_as_empty() {
    let dir = sample();
    write(dir.path(), "vide/liste.yml", "info:\n  name: L\n  type: http\n\nhttp:\n  method: GET\n  url: x\n");
    assert_eq!(read_docs(dir.path(), "").unwrap(), "");
    assert_eq!(read_docs(dir.path(), "vide").unwrap(), "");
}

#[test]
fn ef_doc_01_03_saving_the_collection_docs_keeps_the_rest_of_the_file() {
    let dir = sample();
    assert!(save_docs(dir.path(), "", "# Boutique\n\nL'API de la boutique.").unwrap());
    let text = fs::read_to_string(dir.path().join("opencollection.yml")).unwrap();
    assert!(text.contains("name: Boutique"));
    assert!(text.contains("ignore:"));
    assert!(text.contains("type: text/markdown"));
    assert_eq!(read_docs(dir.path(), "").unwrap(), "# Boutique\n\nL'API de la boutique.");
    assert!(text.find("docs:").unwrap() < text.find("extensions:").unwrap());
}

#[test]
fn ef_doc_01_04_saving_unchanged_docs_writes_nothing() {
    let dir = sample();
    let before = fs::read_to_string(dir.path().join("clients/folder.yml")).unwrap();
    assert!(!save_docs(dir.path(), "clients", "Les clients").unwrap());
    assert_eq!(fs::read_to_string(dir.path().join("clients/folder.yml")).unwrap(), before);
}

#[test]
fn ef_doc_01_05_emptying_the_docs_removes_the_key_and_keeps_the_folder_info() {
    let dir = sample();
    assert!(save_docs(dir.path(), "clients", "").unwrap());
    let text = fs::read_to_string(dir.path().join("clients/folder.yml")).unwrap();
    assert!(!text.contains("docs"));
    assert!(text.contains("name: Clients") && text.contains("seq: 1"));
}

#[test]
fn ef_doc_01_06_a_folder_without_folder_yml_gets_one_with_its_name() {
    let dir = sample();
    write(dir.path(), "paiements/pay.yml", "info:\n  name: Pay\n  type: http\n\nhttp:\n  method: POST\n  url: x\n");
    assert!(!save_docs(dir.path(), "paiements", "").unwrap());
    assert!(!dir.path().join("paiements/folder.yml").exists());
    assert!(save_docs(dir.path(), "paiements", "Les paiements").unwrap());
    let text = fs::read_to_string(dir.path().join("paiements/folder.yml")).unwrap();
    assert!(text.contains("name: paiements") && text.contains("type: folder"));
    assert_eq!(read_docs(dir.path(), "paiements").unwrap(), "Les paiements");
}

#[test]
fn ef_doc_01_07_multiline_docs_and_crlf_files_round_trip() {
    let dir = sample();
    write(dir.path(), "opencollection.yml", "opencollection: 1.0.0\r\n\r\ninfo:\r\n  name: Boutique\r\n");
    let text = "Ligne 1\n\n- a\n- b: « c »";
    assert!(save_docs(dir.path(), "", text).unwrap());
    assert_eq!(read_docs(dir.path(), "").unwrap(), text);
    assert!(fs::read_to_string(dir.path().join("opencollection.yml")).unwrap().contains("\r\n"));
}

#[test]
fn ef_doc_01_08_a_request_docs_table_is_read_and_its_shape_kept_on_save() {
    let dir = sample();
    let mut doc = read_request(dir.path(), "clients/liste.yml").unwrap();
    assert_eq!(doc.docs.as_deref(), Some("# Liste\n\nRenvoie les clients."));
    doc.docs = Some("Nouveau texte".into());
    assert!(save_request(dir.path(), "clients/liste.yml", &doc).unwrap());
    let text = fs::read_to_string(dir.path().join("clients/liste.yml")).unwrap();
    assert!(text.contains("content: Nouveau texte") && text.contains("type: text/markdown"));
    assert_eq!(read_request(dir.path(), "clients/liste.yml").unwrap().docs.as_deref(), Some("Nouveau texte"));
}

#[test]
fn ef_doc_01_09_a_plain_string_request_docs_stays_a_plain_string() {
    let dir = sample();
    write(dir.path(), "a.yml", "info:\n  name: A\n  type: http\n\nhttp:\n  method: GET\n  url: x\n\ndocs: Notes\n");
    let mut doc = read_request(dir.path(), "a.yml").unwrap();
    doc.docs = Some("Autres notes".into());
    save_request(dir.path(), "a.yml", &doc).unwrap();
    let text = fs::read_to_string(dir.path().join("a.yml")).unwrap();
    assert!(text.contains("docs: Autres notes") && !text.contains("content:"));
}

#[test]
fn ef_doc_01_10_paths_outside_the_collection_are_refused() {
    let dir = sample();
    assert!(read_docs(dir.path(), "../autre").is_err());
    assert!(save_docs(dir.path(), "../autre", "x").is_err());
}
