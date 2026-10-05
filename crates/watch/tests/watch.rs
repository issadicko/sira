use std::fs;
use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::thread::sleep;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use xc_watch::{Batch, Watch};

const PATIENCE: Duration = Duration::from_secs(15);

fn watched() -> (TempDir, Watch, Receiver<Batch>) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("sous")).unwrap();
    fs::write(dir.path().join("a.yml"), "a").unwrap();
    fs::write(dir.path().join("sous/b.yml"), "b").unwrap();
    let (sender, batches) = mpsc::channel();
    let watch = Watch::start(dir.path(), move |batch| drop(sender.send(batch))).unwrap();
    sleep(Duration::from_millis(800));
    batches.try_iter().for_each(drop);
    (dir, watch, batches)
}

/// Lots reçus jusqu'à ce que `done` soit satisfait, ou `PATIENCE` écoulée.
fn collect_until(batches: &Receiver<Batch>, done: impl Fn(&[Batch]) -> bool) -> Vec<Batch> {
    let start = Instant::now();
    let mut all = Vec::new();
    while !done(&all) && start.elapsed() < PATIENCE {
        if let Ok(batch) = batches.recv_timeout(Duration::from_millis(250)) {
            all.push(batch);
        }
    }
    all
}

fn reported(all: &[Batch], path: &str) -> bool {
    all.iter().any(|batch| batch.truncated || batch.paths.iter().any(|p| p == path))
}

fn settle() {
    sleep(Duration::from_millis(800));
}

#[test]
fn ef_col_03_a_changed_file_is_reported_with_its_path_relative_to_the_collection() {
    let (dir, _watch, batches) = watched();
    fs::write(dir.path().join("sous/b.yml"), "modifié").unwrap();
    let all = collect_until(&batches, |all| reported(all, "sous/b.yml"));
    assert!(reported(&all, "sous/b.yml"), "{all:?}");
}

#[test]
fn ef_col_03_created_removed_and_renamed_files_are_reported() {
    let (dir, _watch, batches) = watched();
    fs::write(dir.path().join("nouvelle.yml"), "n").unwrap();
    let created = collect_until(&batches, |all| reported(all, "nouvelle.yml"));
    assert!(reported(&created, "nouvelle.yml"), "{created:?}");

    settle();
    fs::remove_file(dir.path().join("a.yml")).unwrap();
    let removed = collect_until(&batches, |all| reported(all, "a.yml"));
    assert!(reported(&removed, "a.yml"), "{removed:?}");

    settle();
    fs::rename(dir.path().join("sous/b.yml"), dir.path().join("sous/c.yml")).unwrap();
    let renamed = collect_until(&batches, |all| reported(all, "sous/b.yml") && reported(all, "sous/c.yml"));
    assert!(reported(&renamed, "sous/b.yml") && reported(&renamed, "sous/c.yml"), "{renamed:?}");
}

#[test]
fn ef_col_03_a_new_folder_and_the_files_written_into_it_are_reported() {
    let (dir, _watch, batches) = watched();
    fs::create_dir(dir.path().join("Neuf")).unwrap();
    fs::write(dir.path().join("Neuf/x.yml"), "x").unwrap();
    let all = collect_until(&batches, |all| reported(all, "Neuf/x.yml") || reported(all, "Neuf"));
    assert!(reported(&all, "Neuf/x.yml") || reported(&all, "Neuf"), "{all:?}");
}

#[test]
fn ef_col_03_a_burst_of_writes_arrives_in_a_handful_of_batches_not_one_per_file() {
    let (dir, _watch, batches) = watched();
    for i in 0..60 {
        fs::write(dir.path().join(format!("rafale-{i}.yml")), "r").unwrap();
    }
    let all = collect_until(&batches, |all| (0..60).all(|i| reported(all, &format!("rafale-{i}.yml"))));
    assert!((0..60).all(|i| reported(&all, &format!("rafale-{i}.yml"))), "{all:?}");
    assert!(all.len() <= 6, "{} lots pour 60 écritures", all.len());
}

#[test]
fn ef_col_03_git_internals_the_sync_base_and_editor_temporary_files_are_not_reported() {
    let (dir, _watch, batches) = watched();
    fs::create_dir_all(dir.path().join(".git")).unwrap();
    fs::create_dir_all(dir.path().join(".oc-sync/openapi")).unwrap();
    sleep(Duration::from_millis(500));
    fs::write(dir.path().join(".git/index"), "i").unwrap();
    fs::write(dir.path().join(".oc-sync/openapi/source.yml"), "s").unwrap();
    fs::write(dir.path().join("a.yml.swp"), "s").unwrap();
    fs::write(dir.path().join(".xc-1-1.tmp"), "t").unwrap();
    fs::write(dir.path().join("vrai.yml"), "v").unwrap();
    let all = collect_until(&batches, |all| reported(all, "vrai.yml"));
    assert!(reported(&all, "vrai.yml"), "{all:?}");
    settle();
    let more: Vec<Batch> = batches.try_iter().collect();
    for batch in all.iter().chain(&more) {
        for path in &batch.paths {
            assert!(
                !path.starts_with(".git")
                    && !path.starts_with(".oc-sync")
                    && !path.ends_with(".swp")
                    && !path.ends_with(".tmp"),
                "{path} ne devait pas être annoncé : {batch:?}"
            );
        }
    }
}

#[test]
fn ef_col_03_dropping_the_watch_stops_the_reports() {
    let (dir, watch, batches) = watched();
    drop(watch);
    settle();
    batches.try_iter().for_each(drop);
    fs::write(dir.path().join("apres.yml"), "x").unwrap();
    sleep(Duration::from_millis(1200));
    assert!(batches.try_recv().is_err());
}

#[test]
fn ef_col_03_watching_a_missing_folder_fails_in_french() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("absent");
    let failure = Watch::start(&missing, |_| {}).err().expect("un dossier absent ne se surveille pas");
    assert!(failure.to_string().starts_with("dossier introuvable"), "{failure}");
    assert!(!Path::new(&missing).exists());
}
