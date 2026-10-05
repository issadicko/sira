use std::fs;
use std::path::Path;

use xc_core::{open_collection, read_environment, save_environment, set_default_environment, CoreError, EnvVar};

const LOCAL: &str = "name: Local
extends: base
color: \"#ff0000\"
variables:
  - name: host
    value: http://localhost:8080
  - secret: true
    name: token
  - name: off
    value: x
    disabled: true
    description: désactivée
  - name: port
    value:
      type: number
      data: \"8080\"
externalSecrets:
  type: vault
  variables: []
";

const COLLECTION: &str = "opencollection: 1.0.0\n\ninfo:\n  name: Démo\n";

fn collection(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("opencollection.yml"), COLLECTION).unwrap();
    for (name, text) in files {
        let path = dir.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    dir
}

fn local() -> tempfile::TempDir {
    collection(&[("environments/Local.yml", LOCAL)])
}

fn text(dir: &Path, name: &str) -> String {
    fs::read_to_string(dir.join("environments").join(format!("{name}.yml"))).unwrap()
}

fn var(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.into(),
        value: Some(value.into()),
        secret: false,
        enabled: true,
        description: None,
        data_type: None,
    }
}

#[test]
fn ef_var_01_read_gives_secrets_without_value_types_and_descriptions() {
    let dir = local();
    let vars = read_environment(dir.path(), "Local").unwrap();
    assert_eq!(vars.len(), 4);
    assert_eq!(vars[0], var("host", "http://localhost:8080"));
    assert!(vars[1].secret && vars[1].value.is_none() && vars[1].name == "token");
    assert!(!vars[2].enabled && vars[2].description.as_deref() == Some("désactivée"));
    assert_eq!((vars[3].value.as_deref(), vars[3].data_type.as_deref()), (Some("8080"), Some("number")));
}

#[test]
fn ef_var_01_saving_what_was_read_writes_nothing_and_keeps_a_hand_written_file_as_is() {
    let hand =
        "# réglages locaux\nname: Local\nvariables:\n    - name:   host # l'hôte\n      value: http://localhost:8080\n";
    let dir = collection(&[("environments/Local.yml", hand)]);
    let vars = read_environment(dir.path(), "Local").unwrap();
    assert!(!save_environment(dir.path(), "Local", &vars).unwrap());
    assert_eq!(text(dir.path(), "Local"), hand);
}

#[test]
fn ef_var_01_changing_a_value_changes_only_its_line_and_keeps_every_other_key() {
    let dir = local();
    let mut vars = read_environment(dir.path(), "Local").unwrap();
    vars[0].value = Some("http://localhost:9090".into());
    assert!(save_environment(dir.path(), "Local", &vars).unwrap());
    assert_eq!(text(dir.path(), "Local"), LOCAL.replace("localhost:8080\n  - secret", "localhost:9090\n  - secret"));
    assert!(!save_environment(dir.path(), "Local", &vars).unwrap(), "un second enregistrement ne change rien");
}

#[test]
fn ef_var_02_a_typed_value_keeps_its_type_when_its_text_is_edited() {
    let dir = local();
    let mut vars = read_environment(dir.path(), "Local").unwrap();
    vars[3].value = Some("9090".into());
    save_environment(dir.path(), "Local", &vars).unwrap();
    assert_eq!(text(dir.path(), "Local"), LOCAL.replace("data: \"8080\"", "data: \"9090\""));
    assert_eq!(read_environment(dir.path(), "Local").unwrap()[3].data_type.as_deref(), Some("number"));
}

#[test]
fn ef_var_01_disabling_adds_disabled_and_enabling_removes_it() {
    let dir = local();
    let mut vars = read_environment(dir.path(), "Local").unwrap();
    vars[0].enabled = false;
    vars[2].enabled = true;
    save_environment(dir.path(), "Local", &vars).unwrap();
    let written = text(dir.path(), "Local");
    assert!(written.contains("  - name: host\n    value: http://localhost:8080\n    disabled: true\n"), "{written}");
    assert!(written.contains("  - name: off\n    value: x\n    description: désactivée\n"), "{written}");
}

#[test]
fn ef_var_01_added_variables_are_appended_removed_ones_disappear_and_none_left_drops_the_list() {
    let dir = local();
    let mut vars = read_environment(dir.path(), "Local").unwrap();
    vars.retain(|v| v.name != "off");
    vars.push(var("nouvelle", "a: b"));
    save_environment(dir.path(), "Local", &vars).unwrap();
    let written = text(dir.path(), "Local");
    assert!(
        !written.contains("name: off") && written.contains("  - name: nouvelle\n    value: \"a: b\"\n"),
        "{written}"
    );
    save_environment(dir.path(), "Local", &[]).unwrap();
    assert_eq!(
        text(dir.path(), "Local"),
        "name: Local\nextends: base\ncolor: \"#ff0000\"\nexternalSecrets:\n  type: vault\n  variables: []\n"
    );
}

#[test]
fn ef_var_01_a_renamed_variable_keeps_its_description_and_its_type() {
    let dir = local();
    let mut vars = read_environment(dir.path(), "Local").unwrap();
    vars[3].name = "portHttp".into();
    vars[2].name = "inactive".into();
    save_environment(dir.path(), "Local", &vars).unwrap();
    let written = text(dir.path(), "Local");
    assert!(
        written.contains("  - name: portHttp\n    value:\n      type: number\n      data: \"8080\"\n"),
        "{written}"
    );
    assert!(
        written.contains("  - name: inactive\n    value: x\n    disabled: true\n    description: désactivée\n"),
        "{written}"
    );
}

#[test]
fn enf_sec_01_a_secret_never_receives_a_value_in_the_file() {
    let dir = local();
    let mut vars = read_environment(dir.path(), "Local").unwrap();
    vars[1].value = Some("jeton-à-ne-pas-écrire".into());
    vars[0].value = Some("http://h".into());
    save_environment(dir.path(), "Local", &vars).unwrap();
    let written = text(dir.path(), "Local");
    assert!(!written.contains("jeton-à-ne-pas-écrire"), "{written}");
    assert!(written.contains("  - secret: true\n    name: token\n"), "{written}");
    vars.push(EnvVar { secret: true, value: Some("autre secret".into()), ..var("cle", "") });
    save_environment(dir.path(), "Local", &vars).unwrap();
    let written = text(dir.path(), "Local");
    assert!(!written.contains("autre secret") && written.contains("  - secret: true\n    name: cle\n"), "{written}");
}

#[test]
fn ef_var_01_saving_creates_a_missing_environment_and_its_folder() {
    let dir = collection(&[]);
    assert!(save_environment(dir.path(), "Dev", &[var("host", "http://d")]).unwrap());
    assert_eq!(text(dir.path(), "Dev"), "name: Dev\nvariables:\n  - name: host\n    value: http://d\n");
    assert_eq!(open_collection(dir.path()).unwrap().environments, ["Dev"]);
}

#[test]
fn ef_var_01_a_byte_order_mark_and_windows_line_endings_are_kept() {
    let crlf = format!("\u{feff}{}", LOCAL.replace('\n', "\r\n"));
    let dir = collection(&[("environments/Local.yml", &crlf)]);
    let mut vars = read_environment(dir.path(), "Local").unwrap();
    vars[0].value = Some("http://h".into());
    save_environment(dir.path(), "Local", &vars).unwrap();
    let written = fs::read(dir.path().join("environments/Local.yml")).unwrap();
    assert!(written.starts_with("\u{feff}name: Local\r\n".as_bytes()));
    assert!(!String::from_utf8(written).unwrap().replace("\r\n", "").contains('\n'));
}

#[test]
fn ef_var_01_an_environment_name_is_a_plain_name_never_a_path() {
    let dir = local();
    for name in ["", ".caché", "../Local", "sous/dossier", "a\\b", "environments/Local"] {
        let read = read_environment(dir.path(), name).unwrap_err();
        let saved = save_environment(dir.path(), name, &[]).unwrap_err();
        assert!(matches!(read, CoreError::InvalidEnvironment(_)), "{name:?} : {read}");
        assert!(matches!(saved, CoreError::InvalidEnvironment(_)), "{name:?} : {saved}");
    }
    assert_eq!(fs::read_dir(dir.path().join("environments")).unwrap().count(), 1);
}

#[test]
fn ef_var_01_a_file_that_is_not_a_table_is_refused_not_overwritten() {
    let dir = collection(&[("environments/Mal.yml", "- a\n- b\n")]);
    assert!(save_environment(dir.path(), "Mal", &[var("a", "b")]).is_err());
    assert_eq!(text(dir.path(), "Mal"), "- a\n- b\n");
}

#[test]
fn ef_var_01_the_default_environment_is_set_replaced_and_cleared_with_its_empty_tables() {
    let dir = collection(&[]);
    let config = || fs::read_to_string(dir.path().join("opencollection.yml")).unwrap();
    set_default_environment(dir.path(), Some("dev")).unwrap();
    assert_eq!(open_collection(dir.path()).unwrap().default_environment.as_deref(), Some("dev"));
    assert!(config().ends_with("extensions:\n  bruno:\n    presets:\n      defaultEnvironment: dev\n"), "{}", config());
    set_default_environment(dir.path(), Some("prod")).unwrap();
    assert_eq!(open_collection(dir.path()).unwrap().default_environment.as_deref(), Some("prod"));
    set_default_environment(dir.path(), None).unwrap();
    assert_eq!(config(), COLLECTION, "les tables vidées sont retirées");
    set_default_environment(dir.path(), None).unwrap();
    assert_eq!(config(), COLLECTION);
}

#[test]
fn ef_var_01_choosing_the_default_environment_keeps_the_other_presets_and_extensions() {
    let text = "opencollection: 1.0.0\n\ninfo:\n  name: Démo\nextensions:\n  bruno:\n    ignore:\n      - node_modules\n    presets:\n      requestType: http\n      requestUrl: https://x.test\n";
    let dir = collection(&[]);
    fs::write(dir.path().join("opencollection.yml"), text).unwrap();
    set_default_environment(dir.path(), Some("dev")).unwrap();
    let written = fs::read_to_string(dir.path().join("opencollection.yml")).unwrap();
    assert_eq!(written, format!("{text}      defaultEnvironment: dev\n"));
    set_default_environment(dir.path(), None).unwrap();
    assert_eq!(fs::read_to_string(dir.path().join("opencollection.yml")).unwrap(), text);
}

#[test]
fn ef_var_01_a_collection_config_whose_presets_is_not_a_table_is_refused_untouched() {
    let text = "opencollection: 1.0.0\n\nextensions:\n  bruno:\n    presets: oups\n";
    let dir = collection(&[]);
    fs::write(dir.path().join("opencollection.yml"), text).unwrap();
    assert!(set_default_environment(dir.path(), Some("dev")).is_err());
    assert_eq!(fs::read_to_string(dir.path().join("opencollection.yml")).unwrap(), text);
}

#[cfg(unix)]
#[test]
fn enf_sec_01_the_default_environment_is_never_written_through_a_symbolic_link() {
    let dir = collection(&[]);
    let real = dir.path().join("reel.yml");
    fs::rename(dir.path().join("opencollection.yml"), &real).unwrap();
    std::os::unix::fs::symlink(&real, dir.path().join("opencollection.yml")).unwrap();
    assert!(matches!(set_default_environment(dir.path(), Some("dev")), Err(CoreError::Symlink(_))));
    assert_eq!(fs::read_to_string(real).unwrap(), COLLECTION);
}
