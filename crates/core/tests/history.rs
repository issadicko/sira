use xc_core::history::{Entry, History, MAX_ENTRIES};

fn entry(n: usize) -> Entry {
    Entry {
        path: format!("r{n}.yml"),
        name: format!("Requête {n}"),
        method: "GET".into(),
        url: "{{baseUrl}}/users?key={{apiKey}}".into(),
        env: Some("dev".into()),
        status: Some(200),
        error: None,
        duration_ms: 12.5,
        size: 321,
        at: "2026-10-06T12:00:00.000Z".into(),
    }
}

#[test]
fn ef_ux_02_the_history_lists_the_newest_request_first_and_survives_a_restart() {
    let dir = tempfile::tempdir().unwrap();
    let history = History::of(dir.path(), "/work/shop");
    assert!(history.list().is_empty());
    history.push(entry(1)).unwrap();
    history.push(entry(2)).unwrap();
    let reopened = History::of(dir.path(), "/work/shop").list();
    assert_eq!(reopened.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(), ["r2.yml", "r1.yml"]);
    assert_eq!(reopened[0], entry(2));
}

#[test]
fn ef_ux_02_each_collection_has_its_own_history() {
    let dir = tempfile::tempdir().unwrap();
    History::of(dir.path(), "/work/shop").push(entry(1)).unwrap();
    assert!(History::of(dir.path(), "/work/blog").list().is_empty());
}

#[test]
fn ef_ux_02_the_history_keeps_the_most_recent_entries_only() {
    let dir = tempfile::tempdir().unwrap();
    let history = History::of(dir.path(), "/work/shop");
    for n in 0..MAX_ENTRIES + 5 {
        history.push(entry(n)).unwrap();
    }
    let kept = history.list();
    assert_eq!(kept.len(), MAX_ENTRIES);
    assert_eq!(kept[0].path, format!("r{}.yml", MAX_ENTRIES + 4));
}

#[test]
fn ef_ux_02_the_history_keeps_the_typed_url_never_a_secret_value() {
    let dir = tempfile::tempdir().unwrap();
    let history = History::of(dir.path(), "/work/shop");
    history.push(entry(1)).unwrap();
    let on_disk: String = std::fs::read_dir(dir.path().join("history"))
        .unwrap()
        .map(|f| std::fs::read_to_string(f.unwrap().path()).unwrap())
        .collect();
    assert!(on_disk.contains("{{apiKey}}"), "{on_disk}");
}

#[test]
fn ef_ux_02_clearing_forgets_everything_and_an_unreadable_file_starts_over() {
    let dir = tempfile::tempdir().unwrap();
    let history = History::of(dir.path(), "/work/shop");
    history.push(entry(1)).unwrap();
    history.clear().unwrap();
    assert!(history.list().is_empty());
    history.clear().unwrap();
    history.push(entry(2)).unwrap();
    let file = std::fs::read_dir(dir.path().join("history")).unwrap().next().unwrap().unwrap().path();
    std::fs::write(&file, "pas du json").unwrap();
    assert!(history.list().is_empty());
    assert_eq!(history.push(entry(3)).unwrap().len(), 1, "un fichier illisible est remplacé");
}

#[test]
fn ef_ux_02_the_entries_of_a_renamed_request_follow_it() {
    let dir = tempfile::tempdir().unwrap();
    let history = History::of(dir.path(), "/work/shop");
    history.push(entry(1)).unwrap();
    history.push(entry(2)).unwrap();

    assert_eq!(history.follow("r1.yml", "users/liste.yml", Some("Liste")).unwrap(), 1);
    let entries = history.list();
    assert_eq!((entries[0].path.as_str(), entries[0].name.as_str()), ("r2.yml", "Requête 2"));
    assert_eq!((entries[1].path.as_str(), entries[1].name.as_str()), ("users/liste.yml", "Liste"));
}

#[test]
fn ef_ux_02_the_entries_inside_a_renamed_or_moved_folder_follow_it() {
    let dir = tempfile::tempdir().unwrap();
    let history = History::of(dir.path(), "/work/shop");
    for path in ["users/a.yml", "users/admin/b.yml", "usersx/c.yml", "users"] {
        history.push(Entry { path: path.into(), ..entry(0) }).unwrap();
    }

    assert_eq!(history.follow("users", "clients", None).unwrap(), 3);
    let paths: Vec<String> = history.list().into_iter().map(|e| e.path).collect();
    assert_eq!(paths, ["clients", "usersx/c.yml", "clients/admin/b.yml", "clients/a.yml"]);
    assert_eq!(history.list()[0].name, "Requête 0", "le nom d'une requête ne change que sur demande");
}

#[test]
fn ef_ux_02_a_rename_that_keeps_the_file_still_updates_the_displayed_name() {
    let dir = tempfile::tempdir().unwrap();
    let history = History::of(dir.path(), "/work/shop");
    history.push(entry(1)).unwrap();
    assert_eq!(history.follow("r1.yml", "r1.yml", Some("Autre nom")).unwrap(), 1);
    assert_eq!(history.list()[0].name, "Autre nom");
}

#[test]
fn ef_ux_02_nothing_is_written_when_no_entry_is_concerned() {
    let dir = tempfile::tempdir().unwrap();
    let history = History::of(dir.path(), "/work/shop");
    assert_eq!(history.follow("a.yml", "b.yml", None).unwrap(), 0, "pas d'historique : aucun fichier créé");
    assert!(!dir.path().join("history").exists());
    history.push(entry(1)).unwrap();
    assert_eq!(history.follow("autre.yml", "b.yml", None).unwrap(), 0);
    assert_eq!(history.list()[0].path, "r1.yml");
}
