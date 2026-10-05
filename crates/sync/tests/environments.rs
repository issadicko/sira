//! EF-VAR-01 et EF-COL-04 : `xc_sync::manage` crée, renomme, duplique et supprime les environnements d'une collection
//! (`environments/<nom>.yml`) sans jamais remplacer un fichier, sans toucher aux autres clés, et en gardant
//! l'environnement par défaut de `opencollection.yml` cohérent. Les tests n'utilisent jamais la vraie corbeille.

use std::fs;
use std::path::Path;

use xc_core::{open_collection, CoreError};
use xc_sync::manage::{clone_environment, create_environment, delete_environment, rename_environment, ManageError};

const PROD: &str = "name: prod
color: \"#ff6b00\"
variables:
  - name: baseUrl
    value: https://api.example.test
  - secret: true
    name: token
";

fn collection(default: Option<&str>, envs: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let presets = default.map_or(String::new(), |name| {
        format!("extensions:\n  bruno:\n    presets:\n      defaultEnvironment: {name}\n")
    });
    fs::write(
        dir.path().join("opencollection.yml"),
        format!("opencollection: 1.0.0\n\ninfo:\n  name: Démo\n{presets}"),
    )
    .unwrap();
    for (name, text) in envs {
        let path = dir.path().join("environments").join(format!("{name}.yml"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

fn text(root: &Path, name: &str) -> String {
    fs::read_to_string(root.join("environments").join(format!("{name}.yml"))).unwrap()
}

fn names(root: &Path) -> Vec<String> {
    open_collection(root).unwrap().environments
}

fn default(root: &Path) -> Option<String> {
    open_collection(root).unwrap().default_environment
}

fn config(root: &Path) -> String {
    fs::read_to_string(root.join("opencollection.yml")).unwrap()
}

/// « Corbeille » de test : déplace le fichier dans `bin`.
fn bin(bin: &Path) -> impl Fn(&Path) -> Result<(), String> + '_ {
    move |path| fs::rename(path, bin.join(path.file_name().unwrap())).map_err(|e| e.to_string())
}

#[test]
fn ef_col_04_creating_an_environment_writes_the_file_bruno_writes_and_creates_the_folder() {
    let dir = collection(None, &[]);
    assert_eq!(create_environment(dir.path(), "Dev").unwrap(), "Dev");
    assert_eq!(text(dir.path(), "Dev"), "name: Dev\n");
    assert_eq!(names(dir.path()), ["Dev"]);
}

#[test]
fn ef_col_04_a_taken_environment_name_gets_a_suffix_and_nothing_is_replaced() {
    let dir = collection(None, &[("Dev", PROD)]);
    assert_eq!(create_environment(dir.path(), "Dev").unwrap(), "Dev 1");
    assert_eq!(create_environment(dir.path(), "dev").unwrap(), "dev 2", "la casse ne distingue pas deux noms");
    assert_eq!(text(dir.path(), "Dev"), PROD);
    assert_eq!(text(dir.path(), "Dev 1"), "name: Dev 1\n");
    assert_eq!(names(dir.path()), ["Dev", "Dev 1", "dev 2"]);
}

#[test]
fn ef_col_04_an_environment_name_is_cleaned_like_a_request_name_and_bad_names_are_refused() {
    let dir = collection(None, &[]);
    assert_eq!(create_environment(dir.path(), "Prod/EU").unwrap(), "Prod-EU");
    for bad in ["", "   ", ".caché", "con", "folder", &"x".repeat(300)] {
        let error = create_environment(dir.path(), bad).unwrap_err();
        assert!(matches!(error, ManageError::InvalidName(_)) && error.is_input(), "{bad:?} : {error}");
    }
    assert_eq!(names(dir.path()), ["Prod-EU"]);
}

#[cfg(unix)]
#[test]
fn enf_sec_01_the_environments_folder_is_never_followed_when_it_is_a_symbolic_link() {
    let dir = collection(None, &[]);
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("environments")).unwrap();
    let error = create_environment(dir.path(), "Dev").unwrap_err();
    assert!(matches!(error, ManageError::Core(CoreError::Symlink(_))), "{error}");
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn ef_col_04_renaming_moves_the_file_updates_its_name_key_and_keeps_everything_else() {
    let dir = collection(None, &[("prod", PROD), ("dev", "name: dev\n")]);
    assert_eq!(rename_environment(dir.path(), "prod", "Production").unwrap(), "Production");
    assert_eq!(text(dir.path(), "Production"), PROD.replace("name: prod\n", "name: Production\n"));
    assert_eq!(names(dir.path()), ["dev", "Production"]);
    assert!(!dir.path().join("environments/prod.yml").exists());
}

#[test]
fn ef_col_04_renaming_to_a_taken_name_takes_the_next_free_one_and_never_replaces() {
    let dir = collection(None, &[("prod", PROD), ("dev", "name: dev\n")]);
    assert_eq!(rename_environment(dir.path(), "prod", "dev").unwrap(), "dev 1");
    assert_eq!(text(dir.path(), "dev"), "name: dev\n");
    assert_eq!(text(dir.path(), "dev 1"), PROD.replace("name: prod\n", "name: dev 1\n"));
}

#[test]
fn ef_col_04_renaming_to_the_same_name_changes_nothing_and_a_change_of_case_alone_is_allowed() {
    let dir = collection(None, &[("prod", PROD)]);
    assert_eq!(rename_environment(dir.path(), "prod", "prod").unwrap(), "prod");
    assert_eq!(text(dir.path(), "prod"), PROD);
    assert_eq!(rename_environment(dir.path(), "prod", "PROD").unwrap(), "PROD");
    assert_eq!(names(dir.path()), ["PROD"]);
    assert_eq!(text(dir.path(), "PROD"), PROD.replace("name: prod\n", "name: PROD\n"));
}

#[test]
fn ef_var_01_the_default_environment_follows_a_rename_and_only_that_one() {
    let dir = collection(Some("prod"), &[("prod", PROD), ("dev", "name: dev\n")]);
    rename_environment(dir.path(), "dev", "local").unwrap();
    assert_eq!(default(dir.path()).as_deref(), Some("prod"));
    rename_environment(dir.path(), "prod", "production").unwrap();
    assert_eq!(default(dir.path()).as_deref(), Some("production"));
}

#[test]
fn ef_col_04_a_missing_environment_or_a_name_that_is_a_path_is_refused_before_any_change() {
    let dir = collection(None, &[("prod", PROD)]);
    for action in [
        rename_environment(dir.path(), "absent", "x"),
        rename_environment(dir.path(), "../opencollection", "x"),
        rename_environment(dir.path(), "a/b", "x"),
        clone_environment(dir.path(), "absent", "x"),
        clone_environment(dir.path(), "", "x"),
    ] {
        let error = action.unwrap_err();
        assert!(error.is_input(), "{error}");
    }
    assert_eq!(names(dir.path()), ["prod"]);
    assert_eq!(text(dir.path(), "prod"), PROD);
}

#[cfg(unix)]
#[test]
fn enf_sec_01_an_environment_that_is_a_symbolic_link_is_neither_renamed_nor_cloned() {
    let dir = collection(None, &[("prod", PROD)]);
    std::os::unix::fs::symlink("prod.yml", dir.path().join("environments/lien.yml")).unwrap();
    for action in [rename_environment(dir.path(), "lien", "x"), clone_environment(dir.path(), "lien", "x")] {
        assert!(matches!(action, Err(ManageError::Core(CoreError::Symlink(_)))));
    }
    assert!(fs::symlink_metadata(dir.path().join("environments/lien.yml")).unwrap().is_symlink());
}

#[test]
fn ef_col_04_cloning_copies_the_file_with_its_own_name_and_leaves_the_original_alone() {
    let dir = collection(Some("prod"), &[("prod", PROD)]);
    assert_eq!(clone_environment(dir.path(), "prod", "prod copie").unwrap(), "prod copie");
    assert_eq!(text(dir.path(), "prod copie"), PROD.replace("name: prod\n", "name: prod copie\n"));
    assert_eq!(text(dir.path(), "prod"), PROD);
    assert_eq!(clone_environment(dir.path(), "prod", "prod copie").unwrap(), "prod copie 1");
    assert_eq!(default(dir.path()).as_deref(), Some("prod"), "la copie n'est jamais l'environnement par défaut");
}

#[test]
fn ef_col_04_deleting_sends_the_file_to_the_bin_and_clears_the_default_only_when_it_was_the_default() {
    let trash = tempfile::tempdir().unwrap();
    let dir = collection(Some("prod"), &[("prod", PROD), ("dev", "name: dev\n")]);
    let before = config(dir.path());
    delete_environment(dir.path(), "dev", bin(trash.path())).unwrap();
    assert!(trash.path().join("dev.yml").exists() && !dir.path().join("environments/dev.yml").exists());
    assert_eq!(config(dir.path()), before);
    delete_environment(dir.path(), "prod", bin(trash.path())).unwrap();
    assert!(trash.path().join("prod.yml").exists());
    assert_eq!(default(dir.path()), None);
    assert!(!config(dir.path()).contains("extensions"), "{}", config(dir.path()));
    assert!(names(dir.path()).is_empty());
}

#[test]
fn ef_col_04_a_bin_that_fails_leaves_the_environment_and_the_default_untouched() {
    let dir = collection(Some("prod"), &[("prod", PROD)]);
    let error = delete_environment(dir.path(), "prod", |_| Err("corbeille indisponible".into())).unwrap_err();
    assert!(matches!(&error, ManageError::Trash(why) if why == "corbeille indisponible"), "{error}");
    assert_eq!(text(dir.path(), "prod"), PROD);
    assert_eq!(default(dir.path()).as_deref(), Some("prod"));
}

#[test]
fn ef_col_04_deleting_a_missing_environment_is_refused_without_calling_the_bin() {
    let dir = collection(None, &[]);
    let error =
        delete_environment(dir.path(), "absent", |_| panic!("la corbeille ne doit pas être appelée")).unwrap_err();
    assert!(matches!(error, ManageError::NotFound(_)) && error.is_input(), "{error}");
}

#[cfg(unix)]
#[test]
fn ef_col_04_a_default_that_cannot_be_updated_after_the_rename_says_the_rename_was_done() {
    let dir = collection(Some("prod"), &[("prod", PROD)]);
    let real = dir.path().join("reel.yml");
    fs::rename(dir.path().join("opencollection.yml"), &real).unwrap();
    std::os::unix::fs::symlink(&real, dir.path().join("opencollection.yml")).unwrap();
    let error = rename_environment(dir.path(), "prod", "production").unwrap_err();
    assert!(!error.is_input());
    assert!(error.to_string().starts_with("l'environnement est bien renommé, mais "), "{error}");
    assert!(dir.path().join("environments/production.yml").exists());
}
