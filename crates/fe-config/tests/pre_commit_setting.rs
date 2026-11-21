//! `git.pre_commit_checks`: empty by default, so nothing runs unasked; a
//! project file can list its own tasks.

use fe_config::{Settings, SettingsStore};

fn write(dir: &tempfile::TempDir, name: &str, contents: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn the_default_is_empty() {
    assert!(Settings::default().git.pre_commit_checks.is_empty());
    let dir = tempfile::tempdir().unwrap();
    let store = SettingsStore::load(Some(write(&dir, "u.toml", "[git]\n")), None);
    assert_eq!(store.diagnostics(), []);
    assert!(store.settings().git.pre_commit_checks.is_empty());
}

#[test]
fn user_and_project_files_set_it() {
    let dir = tempfile::tempdir().unwrap();
    let user = write(&dir, "u.toml", "[git]\npre_commit_checks = [\"fmt\"]\n");
    let store = SettingsStore::load(Some(user.clone()), None);
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().git.pre_commit_checks, ["fmt"]);
    let project = write(
        &dir,
        "p.toml",
        "[git]\npre_commit_checks = [\"clippy\", \"test\"]\n",
    );
    let store = SettingsStore::load(Some(user), Some(project));
    assert_eq!(store.diagnostics(), []);
    assert_eq!(store.settings().git.pre_commit_checks, ["clippy", "test"]);
}
