//! The recent list's folders are locations: today's files read as they did,
//! and a remote folder is kept as its `ssh://` URI.

use std::path::PathBuf;

use fe_config::RecentList;
use fe_config::recent::{self, Entry, ProjectLocation};

/// The recent list as the build before locations had it.
#[derive(serde::Serialize)]
struct Before {
    folders: Vec<PathBuf>,
    files: Vec<PathBuf>,
}

/// A file as the build before locations wrote it.
fn todays_file() -> String {
    let before = Before {
        folders: vec!["/home/me/src/app".into(), "/home/me/src/lib".into()],
        files: vec!["/home/me/src/app/main.rs".into()],
    };
    toml::to_string_pretty(&before).unwrap()
}

fn local(path: &str) -> ProjectLocation {
    ProjectLocation::Local(PathBuf::from(path))
}

fn neon_app() -> ProjectLocation {
    ProjectLocation::parse("ssh://neon/home/me/src/app").unwrap()
}

#[test]
fn a_file_written_before_locations_loads_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.toml");
    std::fs::write(&path, todays_file()).unwrap();
    let (list, error) = RecentList::load(&path);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(
        list.folders,
        [local("/home/me/src/app"), local("/home/me/src/lib")]
    );
    assert_eq!(list.files, [PathBuf::from("/home/me/src/app/main.rs")]);
    list.save(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), todays_file());
}

#[test]
fn a_remote_folder_is_stored_as_its_uri() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.toml");
    recent::record(&path, Entry::Folder(&PathBuf::from("/home/me/src/app"))).unwrap();
    recent::record(&path, Entry::Location(&neon_app())).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains(r#""ssh://neon/home/me/src/app""#), "{text}");
    let (list, error) = RecentList::load(&path);
    assert!(error.is_none(), "{error:?}");
    assert_eq!(list.folders, [neon_app(), local("/home/me/src/app")]);
}

#[test]
fn the_same_folder_on_two_hosts_is_two_entries() {
    let mut list = RecentList::default();
    let box_app = ProjectLocation::parse("ssh://me@box:2222/home/me/src/app").unwrap();
    list.record_location(&neon_app());
    list.record_location(&box_app);
    list.record_location(&neon_app());
    list.record_folder(&PathBuf::from("/home/me/src/app"));
    assert_eq!(
        list.folders,
        [local("/home/me/src/app"), neon_app(), box_app]
    );
}

#[test]
fn a_remote_folder_is_listed_without_looking_for_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut list = RecentList::default();
    list.record_folder(&dir.path().join("gone"));
    list.record_location(&neon_app());
    list.record_folder(dir.path());
    assert_eq!(
        list.existing().folders,
        [ProjectLocation::Local(dir.path().to_owned()), neon_app()]
    );
}
