//! Local history: the on-disk store of each file's past texts.

use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use fe_buffer::{
    LocalHistory,
    local_history::{Entry, Limits, Pruned, Reason},
};

const BASE: u64 = 1_800_000_000_000;

fn at(millis: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(BASE + millis)
}

fn path() -> &'static Path {
    Path::new("/project/src/main.rs")
}

/// What's in `dir`, by name, without the lock file.
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|item| item.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != ".lock")
        .collect();
    names.sort();
    names
}

/// The one file directory in the store.
fn only_slot(dir: &Path) -> PathBuf {
    let names = names(dir);
    assert_eq!(names.len(), 1, "{names:?}");
    dir.join(&names[0])
}

fn texts(store: &LocalHistory, path: &Path) -> Vec<String> {
    store
        .entries(path)
        .unwrap()
        .iter()
        .map(|entry| String::from_utf8(store.read(path, entry.id).unwrap()).unwrap())
        .collect()
}

fn record(store: &LocalHistory, path: &Path, text: &str, millis: u64) {
    store
        .record(path, text.as_bytes(), Reason::Saved, at(millis))
        .unwrap()
        .expect("recorded");
}

#[test]
fn records_and_lists_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::new(dir.path().join("history"));
    // Bytes as they are: not UTF-8, and line endings kept.
    let first: &[u8] = b"caf\xe9\r\n";
    let second: &[u8] = b"second\n";
    let third: &[u8] = b"third\n";

    let first_id = store
        .record(path(), first, Reason::Saved, at(0))
        .unwrap()
        .unwrap();
    let second_id = store
        .record(path(), second, Reason::External, at(10))
        .unwrap()
        .unwrap();
    // Two in one millisecond still get their own entries, in order.
    let third_id = store
        .record(path(), third, Reason::Saved, at(10))
        .unwrap()
        .unwrap();

    assert_eq!(
        store.entries(path()).unwrap(),
        [
            Entry {
                id: third_id,
                time: at(10),
                reason: Reason::Saved,
                len: third.len() as u64,
            },
            Entry {
                id: second_id,
                time: at(10),
                reason: Reason::External,
                len: second.len() as u64,
            },
            Entry {
                id: first_id,
                time: at(0),
                reason: Reason::Saved,
                len: first.len() as u64,
            },
        ]
    );
    assert_eq!(store.read(path(), first_id).unwrap(), first);
    assert_eq!(store.read(path(), second_id).unwrap(), second);
    assert_eq!(store.read(path(), third_id).unwrap(), third);

    let other = Path::new("/project/src/never.rs");
    assert_eq!(store.entries(other).unwrap(), []);
    assert_eq!(
        store.read(other, first_id).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );
}

#[test]
fn a_store_that_was_never_written_lists_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("history");
    let store = LocalHistory::new(&missing);
    assert_eq!(store.entries(path()).unwrap(), []);
    assert_eq!(store.prune(at(0)).unwrap(), Pruned::default());
    assert!(!missing.exists(), "reading must not make the store");
}

#[test]
fn entries_lay_out_under_the_fnv_key_of_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::new(dir.path());
    // An absolute path on each platform, and an independent FNV-1a 64 of
    // its bytes.
    let (file, key) = if cfg!(windows) {
        (r"C:\a", "aaf0df9dede14117")
    } else {
        ("/a", "07d66707b49cd92d")
    };
    store
        .record(Path::new(file), b"one", Reason::Saved, at(0))
        .unwrap();
    store
        .record(Path::new(file), b"two", Reason::External, at(1))
        .unwrap();

    assert_eq!(names(dir.path()), [key]);
    let slot = dir.path().join(key);
    assert_eq!(
        names(&slot),
        [
            format!("{BASE}-0.saved"),
            format!("{}-1.external", BASE + 1),
            "path".to_owned(),
        ]
    );
    assert_eq!(fs::read(slot.join("path")).unwrap(), file.as_bytes());
    assert_eq!(
        fs::read(slot.join(format!("{BASE}-0.saved"))).unwrap(),
        b"one"
    );
}

#[test]
fn a_relative_path_means_the_file_under_the_current_directory() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::new(dir.path());
    let relative = Path::new("relative.rs");
    record(&store, relative, "text", 0);
    let absolute = std::env::current_dir().unwrap().join(relative);
    assert_eq!(texts(&store, &absolute), ["text"]);
    assert_eq!(
        fs::read(only_slot(dir.path()).join("path")).unwrap(),
        absolute.as_os_str().as_encoded_bytes()
    );
}

#[cfg(unix)]
#[test]
fn a_path_that_is_not_utf8_has_its_own_history() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};

    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::new(dir.path());
    let latin1 = Path::new(OsStr::from_bytes(b"/project/caf\xe9.txt"));
    let utf8 = Path::new("/project/café.txt");
    record(&store, latin1, "bytes", 0);
    record(&store, utf8, "text", 0);
    assert_eq!(texts(&store, latin1), ["bytes"]);
    assert_eq!(texts(&store, utf8), ["text"]);
}

#[test]
fn identical_text_is_not_recorded_twice() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::new(dir.path());

    let first = store.record(path(), b"one", Reason::Saved, at(0)).unwrap();
    assert!(first.is_some());
    // Whatever the reason or the time.
    assert_eq!(
        store.record(path(), b"one", Reason::Saved, at(1)).unwrap(),
        None
    );
    assert_eq!(
        store
            .record(path(), b"one", Reason::External, at(2))
            .unwrap(),
        None
    );
    assert_eq!(store.entries(path()).unwrap().len(), 1);

    // Same length, different text.
    record(&store, path(), "two", 3);
    // Only the newest counts: going back to an older text is a new entry.
    record(&store, path(), "one", 4);
    assert_eq!(texts(&store, path()), ["one", "two", "one"]);

    // Another file with the same text has its own history.
    let other = Path::new("/project/other.rs");
    record(&store, other, "one", 5);
    assert_eq!(texts(&store, other), ["one"]);
}

#[test]
fn oversize_text_is_not_recorded() {
    assert_eq!(
        Limits::default(),
        Limits {
            max_entries_per_file: 100,
            max_age: Duration::from_secs(30 * 24 * 60 * 60),
            max_text_len: 4 * 1024 * 1024,
            max_total_len: 256 * 1024 * 1024,
        }
    );

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("history");
    let store = LocalHistory::with_limits(
        &root,
        Limits {
            max_text_len: 8,
            ..Limits::default()
        },
    );

    assert_eq!(
        store
            .record(path(), b"123456789", Reason::Saved, at(0))
            .unwrap(),
        None
    );
    assert!(!root.exists(), "nothing recorded, nothing made");

    record(&store, path(), "12345678", 1);
    assert_eq!(
        store
            .record(path(), b"abcdefghi", Reason::Saved, at(2))
            .unwrap(),
        None
    );
    assert_eq!(texts(&store, path()), ["12345678"]);
}

#[test]
fn count_and_age_limits_drop_oldest() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::with_limits(
        dir.path().join("count"),
        Limits {
            max_entries_per_file: 3,
            ..Limits::default()
        },
    );
    for (index, text) in ["a", "b", "c", "d", "e"].into_iter().enumerate() {
        record(&store, path(), text, index as u64);
    }
    assert_eq!(texts(&store, path()), ["e", "d", "c"]);

    let store = LocalHistory::with_limits(
        dir.path().join("age"),
        Limits {
            max_age: Duration::from_millis(100),
            ..Limits::default()
        },
    );
    record(&store, path(), "a", 0);
    record(&store, path(), "b", 50);
    // Exactly the limit old isn't older than it.
    record(&store, path(), "c", 100);
    assert_eq!(texts(&store, path()), ["c", "b", "a"]);
    record(&store, path(), "d", 101);
    assert_eq!(texts(&store, path()), ["d", "c", "b"]);

    // An expired newest entry doesn't stand in for the text: the file's
    // current text stays in the history.
    record(&store, path(), "d", 1000);
    assert_eq!(texts(&store, path()), ["d"]);

    let store = LocalHistory::with_limits(
        dir.path().join("none"),
        Limits {
            max_entries_per_file: 0,
            ..Limits::default()
        },
    );
    assert_eq!(
        store.record(path(), b"a", Reason::Saved, at(0)).unwrap(),
        None
    );
    assert_eq!(store.entries(path()).unwrap(), []);
}

#[test]
fn the_clock_moving_back_keeps_recording_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::with_limits(
        dir.path(),
        Limits {
            max_entries_per_file: 2,
            ..Limits::default()
        },
    );
    record(&store, path(), "first", 1000);
    record(&store, path(), "second", 1001);
    // Recorded with an earlier time, but still the newest: it's kept, and it's
    // what the next text is compared with.
    record(&store, path(), "third", 0);
    assert_eq!(texts(&store, path()), ["third", "second"]);
    assert_eq!(
        store
            .record(path(), b"third", Reason::Saved, at(1))
            .unwrap(),
        None
    );
    assert_eq!(store.entries(path()).unwrap()[0].time, at(0));
}

#[test]
fn prune_fits_the_total_limit_oldest_first_across_files() {
    let dir = tempfile::tempdir().unwrap();
    let limits = Limits {
        max_age: Duration::from_millis(1000),
        max_total_len: 30,
        ..Limits::default()
    };
    let store = LocalHistory::with_limits(dir.path(), limits);
    let (a, b, c, d) = (
        Path::new("/a"),
        Path::new("/b"),
        Path::new("/c"),
        Path::new("/d"),
    );

    record(&store, d, "d-expired!", 0); // Past the age limit, whatever the total.
    record(&store, c, "c-oldest!!", 1500); // Goes to fit, with its directory.
    record(&store, b, "b-older!!!", 1600); // Goes to fit.
    record(&store, a, "a-newer!!!", 1700);
    record(&store, b, "b-newer!!!", 1800);
    record(&store, a, "a-newest!!", 1900);
    assert_eq!(names(dir.path()).len(), 4);

    let pruned = store.prune(at(2000)).unwrap();

    assert_eq!(
        pruned,
        Pruned {
            entries: 3,
            bytes: 30,
        }
    );
    assert_eq!(texts(&store, a), ["a-newest!!", "a-newer!!!"]);
    assert_eq!(texts(&store, b), ["b-newer!!!"]);
    assert_eq!(texts(&store, c), [""; 0]);
    assert_eq!(texts(&store, d), [""; 0]);
    assert_eq!(names(dir.path()).len(), 2, "c's and d's directories go");

    // It fits now, so another prune changes nothing.
    assert_eq!(store.prune(at(2000)).unwrap(), Pruned::default());

    // Everything ages out: no directories left, and the file can record again.
    let pruned = store.prune(at(100_000)).unwrap();
    assert_eq!(pruned.entries, 3);
    assert_eq!(names(dir.path()), [""; 0]);
    record(&store, a, "again", 100_001);
    assert_eq!(texts(&store, a), ["again"]);
}

#[test]
fn prune_removes_crash_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::new(dir.path());
    record(&store, path(), "kept", 0);
    let slot = only_slot(dir.path());
    let key = slot.file_name().unwrap().to_str().unwrap().to_owned();

    // An entry that crashed before its rename, and a directory that did.
    let temp_entry = slot.join(format!(".{}-1.saved.tmp", BASE + 1));
    fs::write(&temp_entry, "half").unwrap();
    let temp_slot = dir.path().join(format!(".{key}-1.tmp"));
    fs::create_dir(&temp_slot).unwrap();
    fs::write(temp_slot.join("path"), "/other").unwrap();
    // Not the store's.
    let strangers = [
        slot.join("notes.txt"),
        slot.join(".notes.tmp"),
        dir.path().join("notes.txt"),
        dir.path().join(".notes.tmp"),
    ];
    for stranger in &strangers {
        fs::write(stranger, "mine").unwrap();
    }

    // A half-written entry isn't listed.
    assert_eq!(store.entries(path()).unwrap().len(), 1);
    assert_eq!(store.prune(at(1)).unwrap(), Pruned::default());

    assert!(!temp_entry.exists());
    assert!(!temp_slot.exists());
    for stranger in &strangers {
        assert!(stranger.exists(), "{stranger:?}");
    }
    assert_eq!(texts(&store, path()), ["kept"]);

    // A directory holding a stranger stays when its last entry goes, still
    // naming its file.
    assert_eq!(store.prune(at(u64::from(u32::MAX))).unwrap().entries, 1);
    assert!(slot.join("path").exists());
    assert!(slot.join("notes.txt").exists());
    record(&store, path(), "back", u64::from(u32::MAX));
    assert_eq!(texts(&store, path()), ["back"]);
    assert_eq!(names(dir.path()).iter().filter(|n| **n == key).count(), 1);
}

#[test]
fn a_crashed_write_of_the_same_entry_is_written_over() {
    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::new(dir.path());
    record(&store, path(), "kept", 0);
    let slot = only_slot(dir.path());
    fs::write(
        slot.join(format!(".{}-1.saved.tmp", BASE + 1)),
        "a longer text from the crash",
    )
    .unwrap();
    record(&store, path(), "new", 1);
    assert_eq!(texts(&store, path()), ["new", "kept"]);
    assert!(!slot.join(format!(".{}-1.saved.tmp", BASE + 1)).exists());
}

/// Absolute on Unix only, and elsewhere a prefix would part them.
#[cfg(unix)]
#[test]
fn two_paths_with_one_hash_key_stay_apart() {
    // A real collision, found by a parallel rho search and checked with an
    // independent FNV-1a 64; so this pins the hash too.
    let key = "c61dbb1c6d191a42";
    let first = Path::new("/c/f2668fe41cc8d235");
    let second = Path::new("/c/05b6e9112f6d027f");

    let dir = tempfile::tempdir().unwrap();
    let store = LocalHistory::with_limits(
        dir.path(),
        Limits {
            max_age: Duration::from_millis(1000),
            ..Limits::default()
        },
    );
    let first_id = store
        .record(first, b"same", Reason::Saved, at(0))
        .unwrap()
        .unwrap();
    // Not compared with the first path's newest text, despite the key.
    let second_id = store
        .record(second, b"same", Reason::Saved, at(500))
        .unwrap()
        .unwrap();
    record(&store, second, "second", 600);

    let suffixed = format!("{key}-1");
    assert_eq!(names(dir.path()), [key, suffixed.as_str()]);
    let slot = dir.path().join(key);
    assert_eq!(
        fs::read(slot.join("path")).unwrap(),
        first.as_os_str().as_encoded_bytes()
    );
    let slot = dir.path().join(&suffixed);
    assert_eq!(
        fs::read(slot.join("path")).unwrap(),
        second.as_os_str().as_encoded_bytes()
    );
    assert_eq!(texts(&store, first), ["same"]);
    assert_eq!(texts(&store, second), ["second", "same"]);
    assert_eq!(store.read(first, first_id).unwrap(), b"same");
    assert_eq!(store.read(second, second_id).unwrap(), b"same");
    assert_eq!(
        store.read(first, second_id).unwrap_err().kind(),
        io::ErrorKind::NotFound
    );

    // Only the first path's entries are old enough to go, so `<key>` goes and
    // `<key>-1` stays: the second path must still find its history there, not
    // take the free name.
    assert_eq!(store.prune(at(1200)).unwrap().entries, 1);
    assert_eq!(names(dir.path()), [suffixed.as_str()]);
    assert_eq!(texts(&store, first), [""; 0]);
    assert_eq!(texts(&store, second), ["second", "same"]);
    record(&store, second, "third", 1300);
    assert_eq!(names(dir.path()), [suffixed.as_str()]);

    // And the first path takes the free name again.
    record(&store, first, "again", 1400);
    assert_eq!(names(dir.path()), [key, suffixed.as_str()]);
    assert_eq!(texts(&store, first), ["again"]);
    assert_eq!(texts(&store, second), ["third", "second", "same"]);
}

#[test]
fn two_stores_on_one_dir_record_concurrently() {
    const THREADS: usize = 4;
    const RECORDS: usize = 20;

    let dir = tempfile::tempdir().unwrap();
    let limits = Limits {
        max_entries_per_file: THREADS * RECORDS,
        ..Limits::default()
    };
    let stores = &[
        LocalHistory::with_limits(dir.path(), limits),
        LocalHistory::with_limits(dir.path(), limits),
    ];
    let shared = Path::new("/project/shared.rs");
    let done = &AtomicBool::new(false);

    std::thread::scope(|scope| {
        let writers: Vec<_> = (0..THREADS)
            .map(|thread| {
                let store = &stores[thread % stores.len()];
                scope.spawn(move || {
                    let own = PathBuf::from(format!("/project/own{thread}.rs"));
                    for index in 0..RECORDS {
                        // One millisecond for all: only the store keeps the
                        // names apart.
                        let text = format!("text {thread} {index}");
                        record(store, shared, &text, 0);
                        record(store, &own, &text, 0);
                    }
                })
            })
            .collect();
        // Meanwhile the other store prunes, and reads see only whole texts.
        let pruner = scope.spawn(move || {
            let store = &stores[1];
            while !done.load(Ordering::Relaxed) {
                assert_eq!(store.prune(at(0)).unwrap(), Pruned::default());
                for entry in store.entries(shared).unwrap() {
                    let text = store.read(shared, entry.id).unwrap();
                    assert!(text.starts_with(b"text "), "{text:?}");
                    assert_eq!(text.len() as u64, entry.len);
                }
            }
        });
        for writer in writers {
            writer.join().unwrap();
        }
        done.store(true, Ordering::Relaxed);
        pruner.join().unwrap();
    });

    let store = &stores[0];
    let expected = |threads: std::ops::Range<usize>| -> BTreeSet<String> {
        threads
            .flat_map(|thread| (0..RECORDS).map(move |index| format!("text {thread} {index}")))
            .collect()
    };
    let entries = store.entries(shared).unwrap();
    assert_eq!(entries.len(), THREADS * RECORDS);
    let ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
    for (index, id) in ids.iter().enumerate() {
        assert!(!ids[..index].contains(id), "{id:?} twice");
    }
    let got: BTreeSet<String> = texts(store, shared).into_iter().collect();
    assert_eq!(got, expected(0..THREADS));
    for thread in 0..THREADS {
        let own = PathBuf::from(format!("/project/own{thread}.rs"));
        let got: BTreeSet<String> = texts(store, &own).into_iter().collect();
        assert_eq!(got, expected(thread..thread + 1));
    }
    // One directory per file, and no temporary files left over.
    let slots = names(dir.path());
    assert_eq!(slots.len(), THREADS + 1, "{slots:?}");
    for slot in slots {
        for name in names(&dir.path().join(slot)) {
            assert!(!name.starts_with('.'), "{name}");
        }
    }
}

#[test]
fn two_stores_racing_on_one_text_record_it_once() {
    let dir = tempfile::tempdir().unwrap();
    let stores = [LocalHistory::new(dir.path()), LocalHistory::new(dir.path())];
    std::thread::scope(|scope| {
        for store in &stores {
            scope.spawn(move || {
                store.record(path(), b"same", Reason::Saved, at(0)).unwrap();
            });
        }
    });
    assert_eq!(texts(&stores[0], path()), ["same"]);
}

#[cfg(unix)]
#[test]
fn store_dirs_are_private() {
    use std::os::unix::fs::PermissionsExt;

    fn mode(path: &Path) -> u32 {
        fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    let dir = tempfile::tempdir().unwrap();
    // A new directory, under one that doesn't exist yet.
    let new = dir.path().join("state/history");
    let store = LocalHistory::new(&new);
    record(&store, path(), "secret", 0);
    assert_eq!(mode(&new), 0o700);
    let slot = only_slot(&new);
    assert_eq!(mode(&slot), 0o700);
    assert_eq!(mode(&slot.join("path")), 0o600);
    assert_eq!(mode(&slot.join(format!("{BASE}-0.saved"))), 0o600);
    assert_eq!(mode(&new.join(".lock")), 0o600);

    // A directory that was already there, wide open.
    let existing = dir.path().join("existing");
    fs::create_dir(&existing).unwrap();
    fs::set_permissions(&existing, fs::Permissions::from_mode(0o755)).unwrap();
    record(&LocalHistory::new(&existing), path(), "secret", 0);
    assert_eq!(mode(&existing), 0o700);
}
