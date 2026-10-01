//! Renommage qui ne remplace jamais la cible : `fs::rename` écraserait en silence un élément créé entre le choix du nom
//! et le renommage.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::Path;

use crate::import::fold;

/// Renomme `from` en `to`, de façon atomique et sans remplacer : `AlreadyExists` quand `to` est pris, y compris par un
/// nom équivalent (casse, normalisation Unicode) sur un volume qui les confond. Un renommage qui ne change que la
/// casse ou la normalisation du nom est permis : la cible est alors l'élément lui-même.
pub(super) fn rename_new(from: &Path, to: &Path) -> io::Result<()> {
    match exclusive(from, to) {
        Err(e) if e.kind() == ErrorKind::AlreadyExists && is_alias(from, to) => fs::rename(from, to),
        other => other,
    }
}

fn is_alias(from: &Path, to: &Path) -> bool {
    let name = |path: &Path| path.file_name().map(|name| fold(&name.to_string_lossy()));
    name(from) == name(to) && same_file::is_same_file(from, to).unwrap_or(false)
}

/// Repli pour un système ou un volume sans renommage exclusif : la course entre la vérification et le renommage reste
/// possible.
#[cfg(not(windows))]
fn check_then_rename(from: &Path, to: &Path) -> io::Result<()> {
    match fs::symlink_metadata(to) {
        Ok(_) => Err(ErrorKind::AlreadyExists.into()),
        Err(e) if e.kind() == ErrorKind::NotFound => fs::rename(from, to),
        Err(e) => Err(e),
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn c_path(path: &Path) -> io::Result<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| ErrorKind::InvalidInput.into())
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn unsupported(e: &io::Error) -> bool {
    e.raw_os_error().is_some_and(|code| [libc::EINVAL, libc::ENOTSUP, libc::EOPNOTSUPP, libc::ENOSYS].contains(&code))
}

/// `renamex_np` avec `RENAME_EXCL` ; les pointeurs sont des chaînes C valides pendant l'appel.
#[cfg(target_os = "macos")]
fn exclusive(from: &Path, to: &Path) -> io::Result<()> {
    let (from_c, to_c) = (c_path(from)?, c_path(to)?);
    match unsafe { libc::renamex_np(from_c.as_ptr(), to_c.as_ptr(), libc::RENAME_EXCL) } {
        0 => Ok(()),
        _ => match io::Error::last_os_error() {
            e if unsupported(&e) => check_then_rename(from, to),
            e => Err(e),
        },
    }
}

/// `renameat2` avec `RENAME_NOREPLACE` ; les pointeurs sont des chaînes C valides pendant l'appel.
#[cfg(target_os = "linux")]
fn exclusive(from: &Path, to: &Path) -> io::Result<()> {
    let (from_c, to_c) = (c_path(from)?, c_path(to)?);
    let renamed = unsafe {
        libc::renameat2(libc::AT_FDCWD, from_c.as_ptr(), libc::AT_FDCWD, to_c.as_ptr(), libc::RENAME_NOREPLACE)
    };
    match renamed {
        0 => Ok(()),
        _ => match io::Error::last_os_error() {
            e if unsupported(&e) => check_then_rename(from, to),
            e => Err(e),
        },
    }
}

/// `MoveFileExW` sans `MOVEFILE_REPLACE_EXISTING` : échoue si la cible existe.
#[cfg(windows)]
fn exclusive(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let wide = |path: &Path| path.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (from_w, to_w) = (wide(from), wide(to));
    match unsafe { MoveFileExW(from_w.as_ptr(), to_w.as_ptr(), 0) } {
        0 => Err(io::Error::last_os_error()),
        _ => Ok(()),
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn exclusive(from: &Path, to: &Path) -> io::Result<()> {
    check_then_rename(from, to)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manage::names::{claim_unique, taken_names, Wanted};
    use crate::manage::{io_error, ManageError};

    #[test]
    fn ef_col_04_rename_new_moves_a_file_or_a_folder_to_a_free_name() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.yml"), "A").unwrap();
        fs::create_dir(dir.path().join("d")).unwrap();
        fs::write(dir.path().join("d/x.yml"), "X").unwrap();
        rename_new(&dir.path().join("a.yml"), &dir.path().join("b.yml")).unwrap();
        rename_new(&dir.path().join("d"), &dir.path().join("e")).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join("b.yml")).unwrap(), "A");
        assert_eq!(fs::read_to_string(dir.path().join("e/x.yml")).unwrap(), "X");
        assert!(!dir.path().join("a.yml").exists() && !dir.path().join("d").exists());
    }

    #[test]
    fn ef_col_04_rename_new_never_replaces_an_existing_file_or_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| dir.path().join(name);
        fs::write(path("a.yml"), "A").unwrap();
        fs::write(path("b.yml"), "B").unwrap();
        fs::create_dir(path("d")).unwrap();
        fs::create_dir(path("e")).unwrap();
        fs::write(path("e/x.yml"), "X").unwrap();
        fs::create_dir(path("vide")).unwrap();
        for (from, to) in [("a.yml", "b.yml"), ("d", "e"), ("d", "vide"), ("a.yml", "e")] {
            let error = rename_new(&path(from), &path(to)).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::AlreadyExists, "{from} -> {to} : {error}");
        }
        assert_eq!(fs::read_to_string(path("a.yml")).unwrap(), "A");
        assert_eq!(fs::read_to_string(path("b.yml")).unwrap(), "B");
        assert_eq!(fs::read_to_string(path("e/x.yml")).unwrap(), "X");
        assert!(path("d").is_dir() && path("vide").is_dir());
    }

    #[test]
    fn ef_col_04_rename_new_allows_a_change_of_case_or_normalization_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| dir.path().join(name);
        fs::write(path("a.yml"), "A").unwrap();
        rename_new(&path("a.yml"), &path("A.yml")).unwrap();
        let names = |dir: &Path| -> Vec<String> {
            fs::read_dir(dir).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect()
        };
        assert_eq!(names(dir.path()), ["A.yml"]);
        let (nfd, nfc) = ("e\u{301}te\u{301}.yml", "\u{e9}t\u{e9}.yml");
        fs::write(path(nfd), "É").unwrap();
        let moved = rename_new(&path(nfd), &path(nfc));
        let same_entry = fs::read_to_string(path(nfc)).is_ok_and(|text| text == "É");
        assert!(moved.is_ok() && same_entry, "{moved:?}");
    }

    #[test]
    fn ef_col_04_a_name_taken_after_it_was_chosen_makes_the_rename_take_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = |name: &str| dir.path().join(name);
        fs::write(path("a.yml"), "A").unwrap();
        let stale = taken_names(dir.path(), Some("a.yml")).unwrap();
        fs::write(path("B.yml"), "créé entre-temps").unwrap();
        let from = path("a.yml");
        let chosen = claim_unique(dir.path(), stale, Wanted::new("B", ".yml"), |to| {
            rename_new(&from, to).map_err(|e| io_error(&from, e))
        })
        .unwrap();
        assert_eq!(chosen, "B 1.yml");
        assert_eq!(fs::read_to_string(path("B.yml")).unwrap(), "créé entre-temps");
        assert_eq!(fs::read_to_string(path("B 1.yml")).unwrap(), "A");
        let missing =
            claim_unique(dir.path(), taken_names(dir.path(), None).unwrap(), Wanted::new("C", ".yml"), |to| {
                rename_new(&path("absent.yml"), to).map_err(|e| io_error(&from, e))
            });
        assert!(matches!(missing, Err(ManageError::Core(_))), "une autre erreur n'est pas rejouée : {missing:?}");
    }
}
