//! EF-COL-01, EF-COL-04 et EF-SYN-01 : `xc_sync::manage` crée une collection, une requête et un dossier comme Bruno,
//! renomme, duplique, supprime, réordonne et déplace sans toucher aux autres lignes, et tient `source.yml` à jour.
//!
//! Les fichiers de `fixtures/manage/` sont ceux que produit le vrai sérialiseur de Bruno (voir `tools/oracle`). Les
//! tests n'utilisent jamais la vraie corbeille : le « supprimeur » déplace vers un dossier temporaire, et ne lancent
//! jamais Node.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use xc_core::collection::{ignore_name, list_folder};
use xc_core::request::BLANK_BEFORE;
use xc_core::{normalize, read_request};
use xc_sync::import::import_spec;
use xc_sync::manage::{
    clone_item, create_collection, create_folder, create_request, delete_item, init_collection, inspect_folder,
    move_item, rename_item, DropPosition, FolderKind, ManageError,
};
use xc_sync::openapi::GroupBy;
use xc_sync::store::{self, Entry};
use xc_sync::sync::{self, Decisions, OpStatus};

fn fixture(name: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/manage").join(name)).unwrap()
}

fn files_under(dir: &Path) -> BTreeMap<String, String> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, String>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let relative = path.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
                out.insert(relative, String::from_utf8_lossy(&fs::read(&path).unwrap()).into_owned());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}

const REQUEST: &str = "info:
  name: NAME
  type: http
  seq: SEQ
  x-owner: equipe-a

http:
  method: GET
  url: \"{{baseUrl}}/liste\"
  auth: inherit
x-extension:
  nested:
    - a
    - b
";

const FOLDER: &str = "info:
  name: NAME
  type: folder
  seq: SEQ
  x-owner: equipe-a

request:
  auth: inherit
";

fn request(name: &str, seq: i64) -> String {
    REQUEST.replace("NAME", name).replace("SEQ", &seq.to_string())
}

fn folder(name: &str, seq: i64) -> String {
    FOLDER.replace("NAME", name).replace("SEQ", &seq.to_string())
}

struct World {
    dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = create_collection(dir.path(), "Ma Collection").unwrap();
        fs::create_dir(dir.path().join("corbeille")).unwrap();
        Self { dir, root }
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(&self, relative: &str) -> String {
        fs::read_to_string(self.root.join(relative)).unwrap_or_else(|e| panic!("{relative} : {e}"))
    }

    fn exists(&self, relative: &str) -> bool {
        self.root.join(relative).exists()
    }

    fn snapshot(&self) -> BTreeMap<String, String> {
        files_under(&self.root)
    }

    fn requests(&self, names: &[(&str, i64)]) {
        for (name, seq) in names {
            self.write(&format!("{name}.yml"), &request(name, *seq));
        }
    }

    fn order(&self, folder: &str) -> Vec<String> {
        let items = list_folder(&self.root, folder).unwrap().unwrap();
        items.iter().map(|item| item.path().to_owned()).collect()
    }

    fn seq(&self, relative: &str) -> Option<i64> {
        let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        let items = list_folder(&self.root, parent).unwrap().unwrap();
        items.iter().find(|item| item.path() == relative).unwrap_or_else(|| panic!("{relative} absent")).seq()
    }

    fn trash(&self) -> PathBuf {
        self.dir.path().join("corbeille")
    }

    fn trashed(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.trash())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    fn delete(&self, relative: &str) -> Result<(), ManageError> {
        let bin = self.trash();
        delete_item(&self.root, relative, |path| {
            fs::rename(path, bin.join(path.file_name().unwrap())).map_err(|e| e.to_string())
        })
    }
}

fn err<T: std::fmt::Debug>(result: Result<T, ManageError>) -> ManageError {
    result.expect_err("une erreur était attendue")
}

fn invalid_name(error: ManageError) {
    assert!(matches!(error, ManageError::InvalidName(_)), "{error}");
    assert!(error.is_input());
}

#[test]
fn ef_col_04_fixtures_are_canonical() {
    assert_eq!(normalize(&request("A", 1), BLANK_BEFORE).unwrap(), request("A", 1));
    assert_eq!(normalize(&folder("A", 1), BLANK_BEFORE).unwrap(), folder("A", 1));
}

#[test]
fn ef_col_04_inspect_folder_classifies_the_chosen_folder() {
    let dir = tempfile::tempdir().unwrap();
    let path = |name: &str| dir.path().join(name);
    let make = |name: &str, files: &[&str]| {
        fs::create_dir(path(name)).unwrap();
        files.iter().for_each(|file| fs::write(path(name).join(file), "x").unwrap());
        inspect_folder(&path(name)).unwrap()
    };
    assert_eq!(make("vide", &[]), FolderKind::Empty);
    assert_eq!(make("systeme", &[".DS_Store", "Thumbs.db"]), FolderKind::Empty);
    assert_eq!(make("autre", &["notes.txt"]), FolderKind::Other);
    assert_eq!(make("cache", &[".git"]), FolderKind::Other);
    assert_eq!(make("bru", &["bruno.json", "a.bru"]), FolderKind::Bru);
    assert_eq!(make("collection", &["opencollection.yml"]), FolderKind::Collection);
    assert_eq!(make("les-deux", &["opencollection.yml", "bruno.json"]), FolderKind::Collection);
    for missing in [path("absent"), path("autre/notes.txt")] {
        let error = err(inspect_folder(&missing));
        assert!(matches!(error, ManageError::FolderNotFound(_)) && error.is_input(), "{error}");
    }
}

#[test]
fn ef_col_04_inspect_folder_serializes_the_kind_in_lowercase() {
    let kinds = [FolderKind::Collection, FolderKind::Empty, FolderKind::Bru, FolderKind::Other];
    let json: Vec<String> = kinds.iter().map(|kind| serde_json::to_string(kind).unwrap()).collect();
    assert_eq!(json, ["\"collection\"", "\"empty\"", "\"bru\"", "\"other\""]);
}

#[test]
fn ef_col_04_create_collection_writes_exactly_the_files_bruno_writes() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_collection(dir.path(), "Ma Collection").unwrap();
    assert_eq!(root, dir.path().join("Ma Collection"));
    let files = files_under(&root);
    assert_eq!(files.keys().collect::<Vec<_>>(), [".gitignore", "opencollection.yml"]);
    assert_eq!(files["opencollection.yml"], fixture("collection.yml"));
    assert_eq!(files[".gitignore"], fixture("gitignore"));
    assert!(!fixture("gitignore").ends_with('\n'));
    assert!(!root.join("environments").exists() && !root.join(".env").exists());
    assert_eq!(inspect_folder(&root).unwrap(), FolderKind::Collection);
}

#[test]
fn ef_col_04_create_collection_sanitizes_the_folder_and_keeps_the_typed_name_quoted_like_bruno() {
    let dir = tempfile::tempdir().unwrap();
    let root = create_collection(dir.path(), "API: v2 #1").unwrap();
    assert_eq!(root.file_name().unwrap(), "API- v2 #1");
    assert_eq!(fs::read_to_string(root.join("opencollection.yml")).unwrap(), fixture("collection-special.yml"));
    let opened = xc_core::open_collection(&root).unwrap();
    assert_eq!((opened.name.as_str(), opened.request_count), ("API: v2 #1", 0));
}

#[test]
fn ef_col_04_create_collection_reuses_an_empty_folder_and_suffixes_a_busy_one() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("Ma Collection")).unwrap();
    let reused = create_collection(dir.path(), "Ma Collection").unwrap();
    assert_eq!(reused, dir.path().join("Ma Collection"));
    let (first, second) = (
        create_collection(dir.path(), "Ma Collection").unwrap(),
        create_collection(dir.path(), "Ma Collection").unwrap(),
    );
    let names = [first.file_name().unwrap(), second.file_name().unwrap()];
    assert_eq!(names, ["Ma Collection 1", "Ma Collection 2"]);
    assert_eq!(fs::read_to_string(first.join("opencollection.yml")).unwrap(), fixture("collection.yml"));

    let other = tempfile::tempdir().unwrap();
    fs::write(other.path().join("Ma Collection"), "un fichier").unwrap();
    let beside = create_collection(other.path(), "Ma Collection").unwrap();
    assert_eq!(beside.file_name().unwrap(), "Ma Collection 1");
    assert_eq!(fs::read_to_string(other.path().join("Ma Collection")).unwrap(), "un fichier");
}

#[test]
fn ef_col_04_create_collection_refuses_names_that_validate_name_refuses() {
    let dir = tempfile::tempdir().unwrap();
    let too_long = "x".repeat(256);
    for name in ["", "   ", "---", "con", "NUL", "com1", too_long.as_str()] {
        invalid_name(err(create_collection(dir.path(), name)));
        invalid_name(err(init_collection(dir.path(), name)));
    }
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    let missing = err(create_collection(&dir.path().join("absent"), "Ma Collection"));
    assert!(matches!(missing, ManageError::FolderNotFound(_)) && missing.is_input(), "{missing}");
}

#[test]
fn ef_col_04_init_collection_writes_in_place_and_keeps_everything_that_exists() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("notes.txt"), "à garder").unwrap();
    fs::write(dir.path().join(".gitignore"), "target\n").unwrap();
    assert_eq!(inspect_folder(dir.path()).unwrap(), FolderKind::Other);
    let root = init_collection(dir.path(), "Ma Collection").unwrap();
    assert_eq!(root, dir.path());
    let files = files_under(&root);
    assert_eq!(files["opencollection.yml"], fixture("collection.yml"));
    assert_eq!(files[".gitignore"], "target\n");
    assert_eq!(files["notes.txt"], "à garder");
    assert_eq!(files.len(), 3);

    let error = err(init_collection(dir.path(), "Autre"));
    assert!(matches!(error, ManageError::AlreadyCollection(_)) && error.is_input(), "{error}");
    assert!(error.to_string().contains("opencollection.yml"));
    assert_eq!(files_under(&root), files, "rien n'est modifié");
}

#[test]
fn ef_col_04_init_collection_in_an_empty_folder_matches_create_collection() {
    let dir = tempfile::tempdir().unwrap();
    init_collection(dir.path(), "Ma Collection").unwrap();
    let files = files_under(dir.path());
    assert_eq!(files["opencollection.yml"], fixture("collection.yml"));
    assert_eq!(files[".gitignore"], fixture("gitignore"));
    assert!(matches!(err(init_collection(&dir.path().join("absent"), "x")), ManageError::FolderNotFound(_)));
}

#[test]
fn ef_col_04_create_request_matches_the_blank_request_bruno_writes() {
    let w = World::new();
    w.requests(&[("Une", 1)]);
    assert_eq!(create_folder(&w.root, "", "Deux").unwrap(), "Deux");
    let created = create_request(&w.root, "", "Lister les commandes").unwrap();
    assert_eq!(created, "Lister les commandes.yml");
    assert_eq!(w.read(&created), fixture("request-seq3.yml"));
    let nested = create_request(&w.root, "Deux", "Lister les commandes").unwrap();
    assert_eq!(nested, "Deux/Lister les commandes.yml");
    assert_eq!(w.read(&nested), fixture("request-seq3.yml").replace("seq: 3", "seq: 1"));
    let doc = read_request(&w.root, &nested).unwrap();
    assert_eq!((doc.name.as_str(), doc.method.as_str(), doc.url.as_str()), ("Lister les commandes", "GET", ""));
}

#[test]
fn ef_col_04_create_request_writes_a_special_name_the_way_bruno_does() {
    let w = World::new();
    let created = create_request(&w.root, "", "Obtenir: l'état #1").unwrap();
    assert_eq!(created, "Obtenir- l'état #1.yml");
    assert_eq!(w.read(&created), fixture("request-special.yml"));
}

#[test]
fn ef_col_04_create_request_suffixes_taken_file_names_and_keeps_the_typed_name() {
    let w = World::new();
    w.requests(&[("Une", 1), ("Deux", 2)]);
    let first = create_request(&w.root, "", "Lister les commandes").unwrap();
    let second = create_request(&w.root, "", "Lister les commandes").unwrap();
    let third = create_request(&w.root, "", "Lister les commandes").unwrap();
    assert_eq!(
        (first.as_str(), second.as_str(), third.as_str()),
        ("Lister les commandes.yml", "Lister les commandes 1.yml", "Lister les commandes 2.yml")
    );
    assert_eq!(w.read(&second), fixture("request-seq4.yml"));
    assert_eq!(read_request(&w.root, &third).unwrap().name, "Lister les commandes");
    assert_eq!(w.seq(&third), Some(5));
    let by_case = create_request(&w.root, "", "LISTER LES COMMANDES").unwrap();
    assert_eq!(by_case, "LISTER LES COMMANDES 3.yml", "les noms qui ne diffèrent que par la casse se confondent");
    assert_eq!(read_request(&w.root, &by_case).unwrap().name, "LISTER LES COMMANDES");
}

#[test]
fn ef_col_04_create_request_never_overwrites_an_existing_file() {
    let w = World::new();
    w.write("Mocks.yml", "info:\n  name: pris\n");
    let created = create_request(&w.root, "", "Mocks").unwrap();
    assert_eq!(created, "Mocks 1.yml");
    assert_eq!(w.read("Mocks.yml"), "info:\n  name: pris\n");
}

#[test]
fn ef_col_04_create_request_refuses_reserved_and_invalid_names() {
    let w = World::new();
    let before = w.snapshot();
    let too_long = "x".repeat(256);
    for name in [
        "",
        "   ",
        "collection",
        "folder",
        "Folder",
        "opencollection",
        "OpenCollection",
        "con",
        ".caché",
        "-",
        too_long.as_str(),
    ] {
        invalid_name(err(create_request(&w.root, "", name)));
    }
    assert_eq!(w.snapshot(), before);
}

#[test]
fn ef_col_04_create_request_refuses_a_missing_or_forbidden_folder() {
    let w = World::new();
    w.write("Commandes/folder.yml", &folder("Commandes", 1));
    w.requests(&[("Une", 1)]);
    let before = w.snapshot();
    assert!(matches!(err(create_request(&w.root, "absent", "x")), ManageError::FolderNotFound(_)));
    assert!(matches!(err(create_request(&w.root, "Une.yml", "x")), ManageError::FolderNotFound(_)));
    assert!(matches!(err(create_request(&w.root, "Commandes/folder.yml", "x")), ManageError::Forbidden(_)));
    for forbidden in [".oc-sync", "environments", "node_modules"] {
        let error = err(create_request(&w.root, forbidden, "x"));
        assert!(matches!(error, ManageError::Forbidden(_)) && error.is_input(), "{forbidden} : {error}");
    }
    for outside in ["../x", "/tmp", "Commandes/../.."] {
        let error = err(create_request(&w.root, outside, "x"));
        assert!(error.is_input() && error.to_string().contains("hors de la collection"), "{outside} : {error}");
    }
    assert_eq!(w.snapshot(), before);
}

#[test]
fn ef_col_04_create_folder_writes_the_folder_yml_bruno_writes() {
    let w = World::new();
    w.requests(&[("Une", 1)]);
    assert_eq!(create_folder(&w.root, "", "Commandes").unwrap(), "Commandes");
    assert_eq!(w.read("Commandes/folder.yml"), fixture("folder.yml"));
    assert_eq!(fs::read_dir(w.root.join("Commandes")).unwrap().count(), 1);
    assert_eq!(create_folder(&w.root, "Commandes", "Détails").unwrap(), "Commandes/Détails");
    assert_eq!(
        w.read("Commandes/Détails/folder.yml"),
        fixture("folder.yml").replace("seq: 2", "seq: 1").replace("Commandes", "Détails")
    );
    assert_eq!(w.order(""), ["Une.yml", "Commandes"]);
}

#[test]
fn ef_col_04_create_folder_suffixes_taken_names_and_keeps_the_typed_name() {
    let w = World::new();
    w.requests(&[("Une", 1)]);
    let first = create_folder(&w.root, "", "Commandes").unwrap();
    let second = create_folder(&w.root, "", "Commandes").unwrap();
    let by_case = create_folder(&w.root, "", "COMMANDES").unwrap();
    assert_eq!((first.as_str(), second.as_str(), by_case.as_str()), ("Commandes", "Commandes 1", "COMMANDES 2"));
    assert_eq!(w.read("Commandes 1/folder.yml"), fixture("folder.yml").replace("seq: 2", "seq: 3"));
    assert_eq!(
        w.read("COMMANDES 2/folder.yml"),
        fixture("folder.yml").replace("seq: 2", "seq: 4").replace("Commandes", "COMMANDES")
    );
}

#[test]
fn ef_col_04_create_folder_refuses_the_names_the_tree_would_hide() {
    let w = World::new();
    let before = w.snapshot();
    let too_long = "x".repeat(256);
    for name in [
        "",
        "environments",
        "Environments",
        "mocks",
        "opencollection.yml",
        "folder.yml",
        ".oc-sync",
        ".OC-SYNC",
        ".git",
        "node_modules",
        "Node_Modules",
        ".caché",
        "con",
        too_long.as_str(),
    ] {
        invalid_name(err(create_folder(&w.root, "", name)));
    }
    assert_eq!(w.snapshot(), before);
    create_folder(&w.root, "", "Sous").unwrap();
    for allowed in ["mocks", "environments"] {
        assert_eq!(create_folder(&w.root, "Sous", allowed).unwrap(), format!("Sous/{allowed}"));
    }
    for name in [".oc-sync", ".git", "node_modules"] {
        invalid_name(err(create_folder(&w.root, "Sous", name)));
    }
}

#[test]
fn ef_col_04_actions_refuse_paths_outside_the_collection_and_reserved_targets() {
    let w = World::new();
    w.requests(&[("A", 1)]);
    w.write("environments/dev.yml", "name: dev\n");
    w.write(".oc-sync/openapi/source.yml", "source: api.yaml\n");
    w.write("notes.txt", "x");
    fs::create_dir(w.dir.path().join("dehors")).unwrap();
    fs::write(w.dir.path().join("hors.yml"), "info:\n  name: hors\n").unwrap();
    fs::write(w.dir.path().join("dehors/hors.yml"), "info:\n  name: hors\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(w.dir.path().join("dehors"), w.root.join("lien")).unwrap();
    let before = w.snapshot();
    let outside_before = files_under(w.dir.path());
    let mut targets = vec![
        "",
        "../hors.yml",
        "a/../../hors.yml",
        "/etc/hosts",
        "opencollection.yml",
        "environments",
        "environments/dev.yml",
        "Environments",
        ".oc-sync",
        ".oc-sync/openapi/source.yml",
        ".gitignore",
        "node_modules",
        "absent.yml",
        "notes.txt",
    ];
    if cfg!(unix) {
        targets.extend(["lien/hors.yml", "lien"]);
    }
    for target in targets {
        let errors = [
            err(rename_item(&w.root, target, "Autre")),
            err(clone_item(&w.root, target, "Copie")),
            err(w.delete(target)),
            err(move_item(&w.root, target, "", DropPosition::Inside)),
            err(move_item(&w.root, "A.yml", target, DropPosition::Before)),
            err(move_item(&w.root, "A.yml", target, DropPosition::Inside)),
        ];
        for error in errors {
            assert!(error.is_input(), "{target} : {error}");
        }
    }
    assert!(w.trashed().is_empty(), "rien n'est envoyé à la corbeille");
    assert_eq!(w.snapshot(), before);
    assert_eq!(files_under(w.dir.path()), outside_before);
}

fn only_line_changed(before: &str, after: &str, old: &str, new: &str) {
    assert_eq!(after, before.replacen(old, new, 1), "seule la ligne `{old}` devait changer");
    assert_ne!(before, after);
}

#[test]
fn ef_col_04_rename_request_changes_only_info_name_then_the_file_name() {
    let w = World::new();
    w.write("Commandes/Get users.yml", &request("Get users", 1));
    let renamed = rename_item(&w.root, "Commandes/Get users.yml", "Liste des utilisateurs").unwrap();
    assert_eq!(renamed, "Commandes/Liste des utilisateurs.yml");
    assert!(!w.exists("Commandes/Get users.yml"));
    only_line_changed(&request("Get users", 1), &w.read(&renamed), "name: Get users", "name: Liste des utilisateurs");
    let leftovers: Vec<_> = w.snapshot().into_keys().filter(|path| path.contains(".xc-")).collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

#[test]
fn ef_col_04_rename_keeps_the_file_when_the_sanitized_name_is_unchanged() {
    let w = World::new();
    w.write("a-b.yml", &request("a-b", 1));
    assert_eq!(rename_item(&w.root, "a-b.yml", "a:b").unwrap(), "a-b.yml");
    only_line_changed(&request("a-b", 1), &w.read("a-b.yml"), "name: a-b", "name: a:b");
    w.write("Dossier/folder.yml", &folder("Dossier", 1));
    assert_eq!(rename_item(&w.root, "Dossier", "Dossier ").unwrap(), "Dossier");
    assert_eq!(read_request(&w.root, "a-b.yml").unwrap().name, "a:b");
}

#[test]
fn ef_col_04_rename_suffixes_a_name_that_is_taken_and_keeps_the_typed_name() {
    let w = World::new();
    w.requests(&[("A", 1), ("B", 2)]);
    let renamed = rename_item(&w.root, "B.yml", "A").unwrap();
    assert_eq!(renamed, "A 1.yml");
    assert_eq!(w.read("A.yml"), request("A", 1), "le fichier existant n'est jamais écrasé");
    only_line_changed(&request("B", 2), &w.read("A 1.yml"), "name: B", "name: A");
    assert_eq!(rename_item(&w.root, "A 1.yml", "A").unwrap(), "A 1.yml", "son propre nom ne le gêne pas");
}

#[test]
fn ef_col_04_rename_supports_a_change_of_case_alone() {
    let w = World::new();
    w.write("a.yml", &request("a", 1));
    w.write("dossier/folder.yml", &folder("dossier", 2));
    assert_eq!(rename_item(&w.root, "a.yml", "A").unwrap(), "A.yml");
    assert_eq!(rename_item(&w.root, "dossier", "Dossier").unwrap(), "Dossier");
    let names: Vec<String> =
        fs::read_dir(&w.root).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert!(names.contains(&"A.yml".to_owned()) && !names.contains(&"a.yml".to_owned()), "{names:?}");
    assert!(names.contains(&"Dossier".to_owned()) && !names.contains(&"dossier".to_owned()), "{names:?}");
    only_line_changed(&request("a", 1), &w.read("A.yml"), "name: a", "name: A");
}

#[test]
fn ef_col_04_rename_normalizes_comments_and_layout_of_a_hand_written_file() {
    let w = World::new();
    w.write("X.yml", "# écrit à la main\ninfo:\n    name: X   # titre\n    type: http\n    seq: 1\nhttp:\n    method: GET\n    url: \"\"\n");
    rename_item(&w.root, "X.yml", "Y").unwrap();
    assert_eq!(w.read("Y.yml"), "info:\n  name: Y\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"\"\n");
}

#[test]
fn ef_col_04_rename_folder_changes_only_its_folder_yml_name_and_moves_everything_in_it() {
    let w = World::new();
    w.write("Commandes/folder.yml", &folder("Commandes", 2));
    w.write("Commandes/A.yml", &request("A", 1));
    w.write("Commandes/Sous/notes.md", "# notes");
    w.write("Commandes/.env", "SECRET=1");
    let inside = w.snapshot();
    assert_eq!(rename_item(&w.root, "Commandes", "Achats").unwrap(), "Achats");
    assert!(!w.exists("Commandes"));
    let moved: BTreeMap<_, _> = w.snapshot().into_iter().filter(|(path, _)| path.starts_with("Achats/")).collect();
    for (path, text) in &inside {
        let Some(rest) = path.strip_prefix("Commandes/") else { continue };
        let now = &moved[&format!("Achats/{rest}")];
        match rest {
            "folder.yml" => only_line_changed(text, now, "name: Commandes", "name: Achats"),
            _ => assert_eq!(now, text, "{rest}"),
        }
    }
    assert_eq!(moved.len(), 4);
}

#[test]
fn ef_col_04_rename_folder_without_folder_yml_creates_the_minimal_one_bruno_writes() {
    let w = World::new();
    w.write("old/A.yml", &request("A", 1));
    assert_eq!(rename_item(&w.root, "old", "Commandes").unwrap(), "Commandes");
    assert_eq!(w.read("Commandes/folder.yml"), fixture("folder-minimal.yml"));
    assert_eq!(w.read("Commandes/A.yml"), request("A", 1));
    let items = list_folder(&w.root, "").unwrap().unwrap();
    assert_eq!(items[0].name(), "Commandes");
}

#[test]
fn ef_col_04_rename_refuses_invalid_and_reserved_names_without_touching_anything() {
    let w = World::new();
    w.requests(&[("A", 1)]);
    w.write("Commandes/folder.yml", &folder("Commandes", 2));
    let before = w.snapshot();
    let too_long = "x".repeat(256);
    for name in ["", "  ", "folder", "collection", "Opencollection", ".caché", too_long.as_str()] {
        invalid_name(err(rename_item(&w.root, "A.yml", name)));
    }
    for name in ["", "environments", "Mocks", ".git", "node_modules", too_long.as_str()] {
        invalid_name(err(rename_item(&w.root, "Commandes", name)));
    }
    assert_eq!(w.snapshot(), before);
}

#[test]
fn ef_col_01_clone_request_copies_the_file_and_changes_only_name_and_seq() {
    let w = World::new();
    w.requests(&[("A", 1), ("B", 2), ("C", 3)]);
    let before = w.snapshot();
    let copy = clone_item(&w.root, "B.yml", "B copie").unwrap();
    assert_eq!(copy, "B copie.yml");
    assert_eq!(w.read(&copy), request("B copie", 4));
    assert_eq!(w.order(""), ["A.yml", "B.yml", "C.yml", "B copie.yml"]);
    for (path, text) in before {
        assert_eq!(w.read(&path), text, "{path} est intact");
    }
}

#[test]
fn ef_col_01_clone_request_goes_to_the_end_of_the_list_even_after_a_deletion() {
    let w = World::new();
    w.requests(&[("A", 1), ("B", 2), ("C", 3)]);
    w.delete("A.yml").unwrap();
    let copy = clone_item(&w.root, "C.yml", "C copie").unwrap();
    assert_eq!(w.seq(&copy), Some(4));
    assert_eq!(w.order(""), ["B.yml", "C.yml", "C copie.yml"]);
}

#[test]
fn ef_col_01_clone_request_suffixes_a_taken_file_name_in_the_same_folder() {
    let w = World::new();
    w.write("Commandes/A.yml", &request("A", 1));
    w.write("Commandes/B.yml", &request("B", 2));
    let copy = clone_item(&w.root, "Commandes/A.yml", "B").unwrap();
    assert_eq!(copy, "Commandes/B 1.yml");
    assert_eq!(w.read(&copy), request("B", 3));
    assert_eq!(w.read("Commandes/B.yml"), request("B", 2));
}

#[test]
fn ef_col_01_clone_folder_copies_every_file_and_changes_only_name_and_seq_of_its_folder_yml() {
    let w = World::new();
    w.requests(&[("Une", 1)]);
    w.write("Commandes/folder.yml", &folder("Commandes", 2));
    w.write("Commandes/A.yml", &request("A", 1));
    w.write("Commandes/Sous/folder.yml", &folder("Sous", 1));
    w.write("Commandes/Sous/B.yml", &request("B", 1));
    w.write("Commandes/Sous/payload.json", "{\"a\":1}");
    w.write("Commandes/notes.md", "# notes");
    w.write(".gitkeep", "");
    w.write("Commandes/.env", "SECRET=1");
    let before = w.snapshot();
    let copy = clone_item(&w.root, "Commandes", "Commandes copie").unwrap();
    assert_eq!(copy, "Commandes copie");
    let after = w.snapshot();
    for (path, text) in &before {
        assert_eq!(&after[path], text, "{path} est intact");
    }
    let copied: Vec<_> = after.keys().filter(|path| path.starts_with("Commandes copie/")).collect();
    assert_eq!(copied.len(), 7);
    for (path, text) in before.iter().filter(|(path, _)| path.starts_with("Commandes/")) {
        let in_copy = &after[&path.replacen("Commandes/", "Commandes copie/", 1)];
        match path.as_str() {
            "Commandes/folder.yml" => assert_eq!(in_copy, &folder("Commandes copie", 3)),
            _ => assert_eq!(in_copy, text, "{path}"),
        }
    }
    assert_eq!(w.order(""), ["Une.yml", "Commandes", "Commandes copie"]);
}

#[test]
fn ef_col_01_clone_folder_without_folder_yml_gives_the_copy_a_minimal_one() {
    let w = World::new();
    w.write("old/A.yml", &request("A", 1));
    assert_eq!(clone_item(&w.root, "old", "Commandes").unwrap(), "Commandes");
    assert_eq!(w.read("Commandes/folder.yml"), fixture("folder-minimal-seq2.yml"));
    assert_eq!(w.read("Commandes/A.yml"), request("A", 1));
    assert!(!w.exists("old/folder.yml"), "l'original n'est pas touché");
}

#[cfg(unix)]
#[test]
fn ef_col_01_clone_folder_refuses_a_symbolic_link_and_leaves_no_partial_copy() {
    let w = World::new();
    w.write("Commandes/folder.yml", &folder("Commandes", 1));
    w.write("Commandes/A.yml", &request("A", 1));
    std::os::unix::fs::symlink(w.root.join("Commandes/A.yml"), w.root.join("Commandes/lien.yml")).unwrap();
    let before = w.snapshot();
    let error = err(clone_item(&w.root, "Commandes", "Copie"));
    assert!(matches!(error, ManageError::Symlink(_)) && error.is_input(), "{error}");
    assert!(!w.exists("Copie"));
    assert_eq!(w.snapshot(), before);
}

#[test]
fn ef_col_04_delete_sends_the_item_to_the_trash_and_does_not_renumber_the_siblings() {
    let w = World::new();
    w.requests(&[("A", 1), ("B", 2), ("C", 3)]);
    w.delete("B.yml").unwrap();
    assert_eq!(w.trashed(), ["B.yml"]);
    assert_eq!(fs::read_to_string(w.trash().join("B.yml")).unwrap(), request("B", 2));
    assert_eq!(w.order(""), ["A.yml", "C.yml"]);
    assert_eq!((w.read("A.yml"), w.read("C.yml")), (request("A", 1), request("C", 3)));
    assert_eq!((w.seq("A.yml"), w.seq("C.yml")), (Some(1), Some(3)));
}

#[test]
fn ef_col_04_delete_folder_sends_the_whole_folder_to_the_trash() {
    let w = World::new();
    w.write("Commandes/folder.yml", &folder("Commandes", 1));
    w.write("Commandes/Sous/notes.md", "# notes");
    w.delete("Commandes").unwrap();
    assert_eq!(w.trashed(), ["Commandes"]);
    assert_eq!(files_under(&w.trash()).keys().collect::<Vec<_>>(), ["Commandes/Sous/notes.md", "Commandes/folder.yml"]);
    assert!(w.order("").is_empty());
}

#[test]
fn ef_col_04_delete_gives_the_trash_the_path_of_the_item_and_keeps_it_when_the_trash_fails() {
    let w = World::new();
    w.requests(&[("A", 1)]);
    let given = std::cell::RefCell::new(Vec::new());
    let failing = |path: &Path| {
        given.borrow_mut().push(path.to_owned());
        Err("corbeille indisponible".to_owned())
    };
    let error = err(delete_item(&w.root, "A.yml", failing));
    assert!(matches!(error, ManageError::Trash(_)), "{error}");
    assert_eq!(error.to_string(), "envoi à la corbeille impossible : corbeille indisponible");
    assert!(!error.is_input());
    assert_eq!(*given.borrow(), [w.root.join("A.yml")]);
    assert_eq!(w.read("A.yml"), request("A", 1));
}

fn names_and_seqs(w: &World, folder: &str) -> Vec<(String, Option<i64>)> {
    w.order(folder).into_iter().map(|path| (path.clone(), w.seq(&path))).collect()
}

#[test]
fn ef_col_01_reorder_in_a_folder_renumbers_and_writes_only_the_seq_lines_that_change() {
    let w = World::new();
    w.requests(&[("A", 1), ("B", 2), ("C", 3), ("D", 4)]);
    let untouched = format!("# garde-moi\n{}", request("A", 1));
    w.write("A.yml", &untouched);
    assert_eq!(move_item(&w.root, "D.yml", "B.yml", DropPosition::Before).unwrap(), "D.yml");
    assert_eq!(w.order(""), ["A.yml", "D.yml", "B.yml", "C.yml"]);
    assert_eq!(w.read("A.yml"), untouched, "le seq de A ne change pas : A n'est pas réécrit");
    only_line_changed(&request("B", 2), &w.read("B.yml"), "seq: 2", "seq: 3");
    only_line_changed(&request("C", 3), &w.read("C.yml"), "seq: 3", "seq: 4");
    only_line_changed(&request("D", 4), &w.read("D.yml"), "seq: 4", "seq: 2");
}

#[test]
fn ef_col_01_reorder_after_the_last_sibling_renumbers_every_item() {
    let w = World::new();
    w.requests(&[("A", 1), ("B", 2), ("C", 3)]);
    move_item(&w.root, "A.yml", "C.yml", DropPosition::After).unwrap();
    let expected = [("B.yml", Some(1)), ("C.yml", Some(2)), ("A.yml", Some(3))];
    assert_eq!(names_and_seqs(&w, ""), expected.map(|(path, seq)| (path.to_owned(), seq)));
}

#[test]
fn ef_col_01_reorder_puts_folders_and_requests_in_the_same_sequence() {
    let w = World::new();
    w.write("A.yml", &request("A", 1));
    w.write("Commandes/folder.yml", &folder("Commandes", 2));
    w.write("B.yml", &request("B", 3));
    move_item(&w.root, "B.yml", "Commandes", DropPosition::Before).unwrap();
    assert_eq!(w.order(""), ["A.yml", "B.yml", "Commandes"]);
    only_line_changed(&folder("Commandes", 2), &w.read("Commandes/folder.yml"), "seq: 2", "seq: 3");
    only_line_changed(&request("B", 3), &w.read("B.yml"), "seq: 3", "seq: 2");
    move_item(&w.root, "Commandes", "A.yml", DropPosition::Before).unwrap();
    assert_eq!(names_and_seqs(&w, "")[0], ("Commandes".to_owned(), Some(1)));
    only_line_changed(&folder("Commandes", 2), &w.read("Commandes/folder.yml"), "seq: 2", "seq: 1");
}

#[test]
fn ef_col_01_reorder_heals_missing_zero_and_duplicate_seq() {
    let w = World::new();
    w.write("A.yml", "info:\n  name: A\n  type: http\n\nhttp:\n  method: GET\n  url: \"\"\n");
    w.write("B.yml", &request("B", 5));
    w.write("C.yml", &request("C", 5));
    w.write("D.yml", &request("D", 0));
    w.write("E.yml", &request("E", 1));
    w.write("F/folder.yml", "info:\n  name: F\n  type: folder\n");
    let mut expected = w.order("");
    let moved = expected.remove(expected.iter().position(|path| path == "E.yml").unwrap());
    let at = expected.iter().position(|path| path == "B.yml").unwrap();
    expected.insert(at, moved);
    move_item(&w.root, "E.yml", "B.yml", DropPosition::Before).unwrap();
    assert_eq!(w.order(""), expected);
    let seqs: Vec<_> = names_and_seqs(&w, "").into_iter().map(|(_, seq)| seq).collect();
    assert_eq!(seqs, (1..=6).map(Some).collect::<Vec<_>>());
    assert_eq!(w.read("A.yml"), "info:\n  name: A\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: \"\"\n");
    let rank = expected.iter().position(|path| path == "F").unwrap() + 1;
    assert_eq!(w.read("F/folder.yml"), format!("info:\n  name: F\n  type: folder\n  seq: {rank}\n"));
}

#[test]
fn ef_col_01_reorder_onto_itself_changes_nothing() {
    let w = World::new();
    w.requests(&[("A", 1), ("B", 2)]);
    let before = w.snapshot();
    assert_eq!(move_item(&w.root, "A.yml", "A.yml", DropPosition::After).unwrap(), "A.yml");
    assert_eq!(w.snapshot(), before);
}

#[test]
fn ef_col_01_dropping_inside_the_own_folder_moves_to_the_end_and_writes_one_seq() {
    let w = World::new();
    w.requests(&[("A", 1), ("B", 2), ("C", 3)]);
    assert_eq!(move_item(&w.root, "A.yml", "", DropPosition::Inside).unwrap(), "A.yml");
    assert_eq!(w.order(""), ["B.yml", "C.yml", "A.yml"]);
    only_line_changed(&request("A", 1), &w.read("A.yml"), "seq: 1", "seq: 4");
    assert_eq!((w.read("B.yml"), w.read("C.yml")), (request("B", 2), request("C", 3)));
}

#[test]
fn ef_col_01_reorder_works_in_a_sub_folder() {
    let w = World::new();
    w.write("Commandes/folder.yml", &folder("Commandes", 1));
    for (name, seq) in [("A", 1), ("B", 2)] {
        w.write(&format!("Commandes/{name}.yml"), &request(name, seq));
    }
    move_item(&w.root, "Commandes/B.yml", "Commandes/A.yml", DropPosition::Before).unwrap();
    assert_eq!(w.order("Commandes"), ["Commandes/B.yml", "Commandes/A.yml"]);
}

fn world_with_folder() -> World {
    let w = World::new();
    w.write("A.yml", &request("A", 1));
    w.write("F/folder.yml", &folder("F", 2));
    w.write("F/X.yml", &request("X", 1));
    w.write("F/Y.yml", &request("Y", 2));
    w
}

#[test]
fn ef_col_01_move_inside_another_folder_renames_the_file_and_appends_it_at_the_end() {
    let w = world_with_folder();
    let before = w.snapshot();
    assert_eq!(move_item(&w.root, "A.yml", "F", DropPosition::Inside).unwrap(), "F/A.yml");
    assert!(!w.exists("A.yml"));
    only_line_changed(&request("A", 1), &w.read("F/A.yml"), "seq: 1", "seq: 3");
    assert_eq!(w.order("F"), ["F/X.yml", "F/Y.yml", "F/A.yml"]);
    for path in ["F/folder.yml", "F/X.yml", "F/Y.yml"] {
        assert_eq!(w.read(path), before[path], "{path} est intact");
    }
    assert_eq!(w.order(""), ["F"], "les frères de la source ne sont pas renumérotés");
    assert_eq!(w.seq("F"), Some(2));
}

#[test]
fn ef_col_01_move_before_a_sibling_of_another_folder_renumbers_that_folder() {
    let w = world_with_folder();
    let untouched = format!("# garde-moi\n{}", request("X", 1));
    w.write("F/X.yml", &untouched);
    assert_eq!(move_item(&w.root, "A.yml", "F/Y.yml", DropPosition::Before).unwrap(), "F/A.yml");
    assert_eq!(w.order("F"), ["F/X.yml", "F/A.yml", "F/Y.yml"]);
    assert_eq!(w.read("F/X.yml"), untouched);
    only_line_changed(&request("A", 1), &w.read("F/A.yml"), "seq: 1", "seq: 2");
    only_line_changed(&request("Y", 2), &w.read("F/Y.yml"), "seq: 2", "seq: 3");
}

#[test]
fn ef_col_01_move_after_a_sibling_of_another_folder_renumbers_that_folder() {
    let w = world_with_folder();
    move_item(&w.root, "A.yml", "F/X.yml", DropPosition::After).unwrap();
    assert_eq!(w.order("F"), ["F/X.yml", "F/A.yml", "F/Y.yml"]);
    assert_eq!(
        names_and_seqs(&w, "F").into_iter().map(|(_, seq)| seq).collect::<Vec<_>>(),
        [Some(1), Some(2), Some(3)]
    );
}

#[test]
fn ef_col_01_move_suffixes_a_name_that_is_taken_in_the_target_folder() {
    let w = world_with_folder();
    w.write("F/a.yml", &request("a", 3));
    assert_eq!(move_item(&w.root, "A.yml", "F", DropPosition::Inside).unwrap(), "F/A 1.yml");
    assert_eq!(w.read("F/a.yml"), request("a", 3));
    assert_eq!(read_request(&w.root, "F/A 1.yml").unwrap().name, "A");
}

#[test]
fn ef_col_01_move_folder_keeps_every_file_and_changes_only_its_seq() {
    let w = world_with_folder();
    w.write("G/folder.yml", &folder("G", 3));
    w.write("F/Sous/notes.md", "# notes");
    w.write("F/.env", "SECRET=1");
    let before = w.snapshot();
    assert_eq!(move_item(&w.root, "F", "G", DropPosition::Inside).unwrap(), "G/F");
    assert!(!w.exists("F"));
    let after = w.snapshot();
    for (path, text) in before.iter().filter(|(path, _)| path.starts_with("F/")) {
        let moved = &after[&format!("G/{path}")];
        match path.as_str() {
            "F/folder.yml" => only_line_changed(text, moved, "seq: 2", "seq: 1"),
            _ => assert_eq!(moved, text, "{path}"),
        }
    }
    assert_eq!(w.order("G"), ["G/F"]);
}

#[test]
fn ef_col_01_move_to_the_root_goes_after_the_root_items() {
    let w = world_with_folder();
    assert_eq!(move_item(&w.root, "F/X.yml", "", DropPosition::Inside).unwrap(), "X.yml");
    assert_eq!(w.order(""), ["A.yml", "F", "X.yml"]);
    assert_eq!(w.seq("X.yml"), Some(3));
    assert_eq!(w.order("F"), ["F/Y.yml"]);
}

#[test]
fn ef_col_01_a_folder_cannot_be_dropped_into_itself_or_a_descendant() {
    let w = world_with_folder();
    w.write("F/Sous/folder.yml", &folder("Sous", 3));
    let before = w.snapshot();
    let drops = [
        ("F", "F", DropPosition::Inside),
        ("F", "F/Sous", DropPosition::Inside),
        ("F", "F/X.yml", DropPosition::Before),
        ("F", "F/Sous", DropPosition::After),
        ("F/Sous", "F/Sous", DropPosition::Inside),
    ];
    for (path, target, position) in drops {
        let error = err(move_item(&w.root, path, target, position));
        assert!(matches!(error, ManageError::IntoItself(_)) && error.is_input(), "{path} -> {target} : {error}");
    }
    assert_eq!(w.snapshot(), before);
}

#[test]
fn ef_col_01_a_folder_can_be_dropped_beside_its_own_children_parent_and_siblings() {
    let w = world_with_folder();
    w.write("G/folder.yml", &folder("G", 3));
    move_item(&w.root, "G", "A.yml", DropPosition::Before).unwrap();
    assert_eq!(w.order(""), ["G", "A.yml", "F"]);
}

#[test]
fn ef_col_01_move_refuses_a_missing_or_non_folder_destination_without_changes() {
    let w = world_with_folder();
    let before = w.snapshot();
    assert!(matches!(err(move_item(&w.root, "A.yml", "absent", DropPosition::Inside)), ManageError::FolderNotFound(_)));
    assert!(matches!(
        err(move_item(&w.root, "A.yml", "F/X.yml", DropPosition::Inside)),
        ManageError::FolderNotFound(_)
    ));
    assert!(matches!(err(move_item(&w.root, "A.yml", "F/absent.yml", DropPosition::Before)), ManageError::NotFound(_)));
    assert!(matches!(err(move_item(&w.root, "absent.yml", "F", DropPosition::Inside)), ManageError::NotFound(_)));
    assert_eq!(w.snapshot(), before);
}

const SPEC: &str = "openapi: 3.0.0
info: {title: Boutique, version: '1'}
paths:
  /pets:
    get: {operationId: listPets, tags: [pets], summary: List pets, responses: {'200': {description: ok}}}
    post: {operationId: createPet, tags: [pets], summary: Create pet, responses: {'201': {description: ok}}}
  /orders:
    get: {operationId: listOrders, tags: [orders], summary: List orders, responses: {'200': {description: ok}}}
";

const OPERATIONS: &str = "operations:
  - key: listPets
    file: pets/List pets.yml
  - key: createPet
    file: pets/Create pet.yml
  - key: listOrders
    file: orders/List orders.yml
";

fn connected() -> World {
    let dir = tempfile::tempdir().unwrap();
    let root = import_spec(SPEC, "api.yaml", dir.path(), GroupBy::Tags).unwrap();
    fs::create_dir(dir.path().join("corbeille")).unwrap();
    let w = World { dir, root };
    assert!(source_yml(&w).ends_with(OPERATIONS), "{}", source_yml(&w));
    w
}

fn source_yml(w: &World) -> String {
    w.read(".oc-sync/openapi/source.yml")
}

fn tracked(w: &World) -> BTreeMap<String, Option<String>> {
    let store = store::read(&w.root).unwrap().unwrap();
    store.operations.into_iter().map(|entry: Entry| (entry.key, entry.file)).collect()
}

fn statuses(w: &World) -> BTreeMap<String, OpStatus> {
    let recorded = sync::status(&w.root).unwrap().source.unwrap();
    let plan = sync::plan(&w.root, SPEC, recorded, &[]).unwrap();
    plan.operations.into_iter().map(|op| (op.key, op.status)).collect()
}

fn file_of(w: &World, key: &str) -> Option<String> {
    tracked(w)[key].clone()
}

#[test]
fn ef_syn_01_renaming_a_tracked_request_moves_its_entry_and_the_sync_still_finds_it() {
    let w = connected();
    let before = source_yml(&w);
    assert_eq!(file_of(&w, "listPets").as_deref(), Some("pets/List pets.yml"));
    assert_eq!(rename_item(&w.root, "pets/List pets.yml", "Show pets").unwrap(), "pets/Show pets.yml");
    assert_eq!(source_yml(&w), before.replace("pets/List pets.yml", "pets/Show pets.yml"));
    assert_eq!(file_of(&w, "createPet").as_deref(), Some("pets/Create pet.yml"));
    let plan = statuses(&w);
    assert!(plan.values().all(|status| matches!(status, OpStatus::Unchanged | OpStatus::Kept)), "{plan:?}");
}

#[test]
fn ef_syn_01_renaming_a_folder_moves_every_entry_under_it() {
    let w = connected();
    assert_eq!(rename_item(&w.root, "pets", "animals").unwrap(), "animals");
    assert_eq!(file_of(&w, "listPets").as_deref(), Some("animals/List pets.yml"));
    assert_eq!(file_of(&w, "createPet").as_deref(), Some("animals/Create pet.yml"));
    assert_eq!(file_of(&w, "listOrders").as_deref(), Some("orders/List orders.yml"));
    let plan = statuses(&w);
    assert!(plan.values().all(|status| matches!(status, OpStatus::Unchanged | OpStatus::Kept)), "{plan:?}");
}

#[test]
fn ef_syn_01_moving_a_tracked_request_or_a_folder_moves_its_entries() {
    let w = connected();
    assert_eq!(
        move_item(&w.root, "orders/List orders.yml", "pets", DropPosition::Inside).unwrap(),
        "pets/List orders.yml"
    );
    assert_eq!(file_of(&w, "listOrders").as_deref(), Some("pets/List orders.yml"));
    assert_eq!(move_item(&w.root, "pets", "orders", DropPosition::Inside).unwrap(), "orders/pets");
    assert_eq!(file_of(&w, "listPets").as_deref(), Some("orders/pets/List pets.yml"));
    assert_eq!(file_of(&w, "createPet").as_deref(), Some("orders/pets/Create pet.yml"));
    assert_eq!(file_of(&w, "listOrders").as_deref(), Some("orders/pets/List orders.yml"));
    let plan = statuses(&w);
    assert!(plan.values().all(|status| matches!(status, OpStatus::Unchanged | OpStatus::Kept)), "{plan:?}");
}

#[test]
fn ef_syn_01_deleting_a_tracked_request_marks_it_ignored_without_file() {
    let w = connected();
    let before = source_yml(&w);
    w.delete("pets/List pets.yml").unwrap();
    assert_eq!(
        source_yml(&w),
        before.replace("  - key: listPets\n    file: pets/List pets.yml\n", "  - key: listPets\n    ignored: true\n")
    );
    let store = store::read(&w.root).unwrap().unwrap();
    assert_eq!(store.operations[0], Entry::ignored("listPets"));
    let plan = statuses(&w);
    assert!(!plan.contains_key("listPets"), "l'équipe ne la veut plus : la synchro ne la recrée pas");
    assert!(!w.exists("pets/List pets.yml"));
}

#[test]
fn ef_syn_01_deleting_a_folder_marks_every_tracked_request_in_it_ignored() {
    let w = connected();
    w.delete("pets").unwrap();
    let store = store::read(&w.root).unwrap().unwrap();
    assert_eq!(
        store.operations,
        [
            Entry::ignored("listPets"),
            Entry::ignored("createPet"),
            Entry::tracked("listOrders", "orders/List orders.yml")
        ]
    );
    assert!(!statuses(&w).contains_key("createPet"));
}

#[test]
fn ef_syn_01_a_copy_is_not_tracked_and_untracked_actions_leave_source_yml_untouched() {
    let w = connected();
    let before = source_yml(&w);
    let copy = clone_item(&w.root, "pets/List pets.yml", "List pets copie").unwrap();
    clone_item(&w.root, "pets", "pets copie").unwrap();
    let created = create_request(&w.root, "orders", "Nouvelle").unwrap();
    let folder = create_folder(&w.root, "", "Autres").unwrap();
    rename_item(&w.root, &copy, "Autre copie").unwrap();
    move_item(&w.root, &created, &folder, DropPosition::Inside).unwrap();
    move_item(&w.root, "pets/Create pet.yml", "pets/List pets.yml", DropPosition::Before).unwrap();
    w.delete("pets copie/Create pet.yml").unwrap();
    assert_eq!(source_yml(&w), before);
    assert_eq!(tracked(&w).len(), 3);
}

#[test]
fn ef_syn_01_a_refused_trash_leaves_the_file_and_source_yml_untouched() {
    let w = connected();
    let before = w.snapshot();
    let error = err(delete_item(&w.root, "pets", |_| Err("corbeille indisponible".into())));
    assert!(matches!(error, ManageError::Trash(_)), "{error}");
    assert_eq!(w.snapshot(), before);
}

#[test]
fn ef_syn_01_an_unreadable_source_yml_refuses_the_action_before_changing_anything() {
    let w = connected();
    w.write(".oc-sync/openapi/source.yml", "source: api.yaml\n");
    let before = w.snapshot();
    for error in [
        err(rename_item(&w.root, "pets/List pets.yml", "Autre")),
        err(rename_item(&w.root, "pets", "animals")),
        err(move_item(&w.root, "orders", "pets", DropPosition::Inside)),
        err(w.delete("pets")),
    ] {
        assert!(error.is_input() && error.to_string().contains("source.yml"), "{error}");
    }
    assert_eq!(w.snapshot(), before);
    assert!(w.trashed().is_empty());
}

#[test]
fn ef_syn_01_connecting_a_spec_adds_oc_sync_to_the_ignore_list_and_changes_nothing_else() {
    let w = World::new();
    let mut opencollection = fixture("collection.yml");
    assert!(!opencollection.contains(".oc-sync"));
    let plan = sync::plan(&w.root, SPEC, "api.yaml".into(), &[]).unwrap();
    plan.apply(&Decisions::default()).unwrap();
    opencollection.push_str("      - .oc-sync\n");
    assert_eq!(w.read("opencollection.yml"), opencollection);
    assert_eq!(w.read(".gitignore"), fixture("gitignore"));
    assert!(w.exists(".oc-sync/openapi/source.yml"));

    let again = sync::plan(&w.root, SPEC, "api.yaml".into(), &[]).unwrap();
    again.apply(&Decisions::default()).unwrap();
    assert_eq!(w.read("opencollection.yml"), opencollection, "déjà dans la liste : fichier intact");
}

#[test]
fn ef_syn_01_hiding_oc_sync_changes_only_the_ignore_list_of_a_full_collection_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("opencollection.yml");
    let stringify = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stringify/collections");
    let full = fs::read_to_string(stringify.join("hand-full.yml")).unwrap();
    fs::write(&file, &full).unwrap();
    ignore_name(dir.path(), ".oc-sync").unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        full.replace("      - dist build\n", "      - dist build\n      - .oc-sync\n")
    );

    let minimal = fs::read_to_string(stringify.join("hand-minimal.yml")).unwrap();
    fs::write(&file, &minimal).unwrap();
    ignore_name(dir.path(), ".oc-sync").unwrap();
    assert_eq!(
        fs::read_to_string(&file).unwrap(),
        minimal.replace("extensions: {}\n", "extensions:\n  bruno:\n    ignore:\n      - .oc-sync\n")
    );
}
