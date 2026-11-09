//! The recently opened list: order, caps, persistence and concurrent writers.

use std::path::PathBuf;

use fe_config::RecentList;
use fe_config::recent::{self, Entry, MAX_FILES, MAX_FOLDERS};

fn p(s: &str) -> PathBuf {
    PathBuf::from(s)
}

#[test]
fn recording_moves_to_front_without_duplicates_and_caps() {
    let mut list = RecentList::default();
    list.record_file(&p("/a"));
    list.record_file(&p("/b"));
    list.record_file(&p("/a"));
    assert_eq!(list.files, [p("/a"), p("/b")]);
    list.record_folder(&p("/x"));
    list.record_folder(&p("/y"));
    list.record_folder(&p("/x"));
    assert_eq!(list.folders, [p("/x"), p("/y")]);

    for i in 0..80 {
        list.record_file(&p(&format!("/f{i}")));
        list.record_folder(&p(&format!("/d{i}")));
    }
    assert_eq!(list.files.len(), MAX_FILES);
    assert_eq!(list.folders.len(), MAX_FOLDERS);
    assert_eq!(list.files[0], p("/f79"));
    assert_eq!(list.folders[0], p("/d79"));
}

#[test]
fn load_missing_invalid_and_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state").join("recent.toml");
    let (list, err) = RecentList::load(&path);
    assert!(list.files.is_empty() && list.folders.is_empty() && err.is_none());

    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "files = [[[ nope").unwrap();
    let (list, err) = RecentList::load(&path);
    assert_eq!(list, RecentList::default());
    assert!(err.is_some());

    let mut saved = RecentList::default();
    saved.record_file(&p("/a b/c.rs"));
    saved.record_folder(&p("/proj"));
    saved.save(&path).unwrap();
    let (loaded, err) = RecentList::load(&path);
    assert!(err.is_none());
    assert_eq!(loaded, saved);
}

#[test]
fn two_threads_recording_lose_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.toml");
    // 100 files and 100 folders recorded, so the caps are what limit the result.
    let handles: Vec<_> = (0..2)
        .map(|t| {
            let path = path.clone();
            std::thread::spawn(move || {
                for i in 0..50 {
                    let file = PathBuf::from(format!("/t{t}/f{i}"));
                    recent::record(&path, Entry::File(&file)).unwrap();
                    let folder = PathBuf::from(format!("/t{t}/d{i}"));
                    recent::record(&path, Entry::Folder(&folder)).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let (list, err) = RecentList::load(&path);
    assert!(err.is_none());
    assert_eq!(list.files.len(), MAX_FILES);
    assert_eq!(list.folders.len(), MAX_FOLDERS);
    // Whatever the interleaving, no entry is duplicated or torn, and each
    // thread's surviving files keep the order it recorded them in.
    let mut seen = list.files.clone();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), MAX_FILES);
    for t in 0..2 {
        let prefix = format!("/t{t}/f");
        let order: Vec<u32> = list
            .files
            .iter()
            .filter_map(|f| f.to_str()?.strip_prefix(&prefix)?.parse().ok())
            .collect();
        assert!(order.windows(2).all(|w| w[0] > w[1]), "{order:?}");
    }
}

#[test]
fn threads_recording_few_entries_keep_all_of_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.toml");
    let handles: Vec<_> = (0..2)
        .map(|t| {
            let path = path.clone();
            std::thread::spawn(move || {
                for i in 0..25 {
                    let file = PathBuf::from(format!("/t{t}/f{i}"));
                    recent::record(&path, Entry::File(&file)).unwrap();
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let (list, _) = RecentList::load(&path);
    assert_eq!(list.files.len(), 50);
}

#[test]
fn existing_skips_gone_paths_and_remove_and_clear_persist() {
    let dir = tempfile::tempdir().unwrap();
    let here = dir.path().join("here.txt");
    std::fs::write(&here, "x").unwrap();
    let gone = dir.path().join("gone.txt");
    let path = dir.path().join("recent.toml");

    recent::record(&path, Entry::File(&here)).unwrap();
    recent::record(&path, Entry::File(&gone)).unwrap();
    recent::record(&path, Entry::Folder(dir.path())).unwrap();

    let (list, _) = RecentList::load(&path);
    assert_eq!(list.files, [gone.clone(), here.clone()]);
    assert_eq!(list.existing().files, std::slice::from_ref(&here));
    assert_eq!(list.existing().folders, [dir.path().to_owned()]);

    recent::remove(&path, &gone).unwrap();
    let (list, _) = RecentList::load(&path);
    assert_eq!(list.files, [here]);

    recent::clear(&path).unwrap();
    let (list, _) = RecentList::load(&path);
    assert_eq!(list, RecentList::default());
}

#[cfg(unix)]
#[test]
fn an_unreadable_file_is_kept_and_the_update_errors() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.toml");
    recent::record(&path, Entry::File(&p("/a"))).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&path).is_ok() {
        return; // running as root: the file stays readable
    }
    assert!(recent::record(&path, Entry::File(&p("/b"))).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let (list, err) = RecentList::load(&path);
    assert!(err.is_none());
    assert_eq!(list.files, [p("/a")]);
}

#[test]
fn an_invalid_file_is_replaced_by_record() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.toml");
    std::fs::write(&path, "files = [[[").unwrap();
    let list = recent::record(&path, Entry::File(&p("/a"))).unwrap();
    assert_eq!(list.files, [p("/a")]);
    assert!(RecentList::load(&path).1.is_none());
}

#[test]
fn concurrent_saves_never_expose_partial_contents() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.toml");
    let mut list = RecentList::default();
    for i in 0..MAX_FILES {
        list.record_file(&p(&format!("/some/long/path/{i}")));
    }
    list.save(&path).unwrap();
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let (path, list) = (path.clone(), list.clone());
            std::thread::spawn(move || {
                for _ in 0..50 {
                    list.save(&path).unwrap();
                    let (read, err) = RecentList::load(&path);
                    assert!(err.is_none());
                    assert_eq!(read, list);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    let leftovers = std::fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(leftovers, 1, "temp files left behind");
}

#[test]
fn loads_during_concurrent_saves_never_fail() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.toml");
    let mut list = RecentList::default();
    for i in 0..MAX_FILES {
        list.record_file(&p(&format!("/some/long/path/{i}")));
    }
    list.save(&path).unwrap();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (path, list) = (path.clone(), list.clone());
            std::thread::spawn(move || {
                for _ in 0..500 {
                    list.save(&path).unwrap();
                    let (read, err) = RecentList::load(&path);
                    assert!(err.is_none(), "load failed: {err:?}");
                    assert_eq!(read, list);
                }
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn save_does_not_follow_a_planted_symlink() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recent.toml");
    let victim = dir.path().join("victim.txt");
    std::fs::write(&victim, "keep").unwrap();
    // The counter is shared by every test in this process, so plant a wide
    // range of the names `save` may pick next.
    for n in 0..2000 {
        let mut name = path.as_os_str().to_owned();
        name.push(format!(".{}.{n}.tmp", std::process::id()));
        std::os::unix::fs::symlink(&victim, name).unwrap();
    }
    let mut list = RecentList::default();
    list.record_file(&p("/a"));
    list.save(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep");
    assert_eq!(RecentList::load(&path).0, list);
}
