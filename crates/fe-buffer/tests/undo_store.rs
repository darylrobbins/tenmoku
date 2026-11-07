//! Persistent undo: `Buffer::export_undo` and `import_undo`, and the
//! `UndoStore` that keeps the records on disk.

use std::{
    fs,
    ops::Range,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime},
};

use fe_buffer::{
    Buffer, Origin, UndoImportError, UndoRecord, UndoStore,
    undo_store::{MAX_ENTRIES, text_hash},
};

fn user(origin: &Origin) -> bool {
    *origin == Origin::User
}

fn ai() -> Origin {
    Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    }
}

fn text(buffer: &Buffer) -> String {
    buffer.text().to_string()
}

/// The texts `u` steps back through, until there's nothing to undo, then
/// `<C-r>` forward, until there's nothing to redo.
fn walk(buffer: &mut Buffer) -> (Vec<String>, Vec<String>) {
    let mut back = Vec::new();
    while buffer.undo(user).unwrap().is_some() {
        back.push(text(buffer));
    }
    let mut forward = Vec::new();
    while buffer.redo(user).unwrap().is_some() {
        forward.push(text(buffer));
    }
    (back, forward)
}

/// A buffer with `text` and history of its own, which it came by typing.
fn typed(start: &str, edits: &[(std::ops::Range<usize>, &str)]) -> Buffer {
    let mut buffer = Buffer::new(start);
    for (range, new_text) in edits {
        buffer.edit([(range.clone(), *new_text)], Origin::User);
    }
    buffer
}

#[test]
fn export_then_import_round_trips_undo_and_redo() {
    let now = Arc::new(AtomicU64::new(1_000));
    let mut original = Buffer::new("fn main() {}\n");
    let clock = now.clone();
    original.set_clock(move || clock.fetch_add(10, Ordering::Relaxed));
    original.edit([(11..11, "\n    let a = 1;\n")], Origin::User);
    // Several batches in one transaction, as insert mode types them.
    original.start_transaction(Origin::User);
    original.edit([(29..29, "    ")], Origin::User);
    original.edit([(33..33, "a")], Origin::User);
    original.edit([(34..34, "é;\n")], Origin::User);
    original.end_transaction();
    // Two places at once, as a multi-caret edit.
    original.edit([(3..7, "start"), (24..25, "2")], Origin::User);
    original.edit([(0..2, "")], Origin::User);
    // The newest two undone: redo reapplies them.
    original.undo(user).unwrap();
    original.undo(user).unwrap();
    original.did_save();
    let times: Vec<_> = (original.history_times(user).iter())
        .map(|time| (time.at_unix_ms, time.undone))
        .collect();

    let record = original.export_undo().expect("a user history is stored");
    assert_eq!(record.entries.len(), 4);
    assert_eq!(
        (record.entries.iter().map(|entry| entry.undone)).collect::<Vec<_>>(),
        [false, false, true, true]
    );
    assert_eq!(record.text_hash, text_hash(original.text()));
    assert_eq!(record.text_len, original.len() as u64);

    // Through the store and back, as a reopened file gets it.
    let dir = tempfile::tempdir().unwrap();
    let store = UndoStore::new(dir.path().join("undo"));
    let path = dir.path().join("main.rs");
    store.write(&path, &record).unwrap();
    let read = store.read(&path).unwrap().expect("written");
    assert_eq!(read, record);

    let mut reopened = Buffer::new(&text(&original));
    let version = reopened.version();
    reopened.import_undo(read).unwrap();
    assert_eq!(text(&reopened), text(&original));
    assert!(!reopened.is_dirty(), "the text is the file's");
    assert!(reopened.version() > version, "rebuilt by edits");
    let reopened_times: Vec<_> = (reopened.history_times(user).iter())
        .map(|time| (time.at_unix_ms, time.undone))
        .collect();
    assert_eq!(reopened_times, times, "entries keep their times");

    let walked = walk(&mut original);
    assert_eq!((walked.0.len(), walked.1.len()), (2, 4));
    assert_eq!(
        walked.0.last().unwrap(),
        "fn main() {}\n",
        "back to the start"
    );
    assert_eq!(walk(&mut reopened), walked);
    assert_eq!(text(&reopened), text(&original));
}

#[test]
fn a_changed_text_is_stale_and_leaves_the_buffer_untouched() {
    let original = typed("one\n", &[(3..3, " two")]);
    let record = original.export_undo().unwrap();

    let mut changed = Buffer::new("one three\n");
    let version = changed.version();
    assert_eq!(
        changed.import_undo(record.clone()),
        Err(UndoImportError::Stale)
    );
    assert_eq!(text(&changed), "one three\n");
    assert_eq!(changed.version(), version);
    assert_eq!(changed.undo(user), Ok(None));

    // The same length, other bytes: the hash tells them apart.
    let mut same_length = Buffer::new("one twx\n");
    assert_eq!(
        same_length.import_undo(record.clone()),
        Err(UndoImportError::Stale)
    );
    assert_eq!(same_length.undo(user), Ok(None));

    // The right text, with history of its own: that's kept.
    let mut edited = typed("one two!\n", &[(7..8, "")]);
    let version = edited.version();
    assert_eq!(
        edited.import_undo(record.clone()),
        Err(UndoImportError::Edited)
    );
    assert_eq!(edited.version(), version);
    edited.undo(user).unwrap();
    assert_eq!(text(&edited), "one two!\n");

    // A record whose edits don't fit its own text.
    let mut forged = record;
    forged.entries[0].batches[0].edits[0].new_text = " zzz".into();
    let mut fresh = Buffer::new("one two\n");
    let version = fresh.version();
    assert_eq!(fresh.import_undo(forged), Err(UndoImportError::Invalid));
    assert_eq!(
        (text(&fresh), fresh.version()),
        ("one two\n".into(), version)
    );
}

#[test]
fn ai_origin_entries_are_not_stored() {
    let mut buffer = Buffer::new("a\n");
    buffer.edit([(1..1, "b")], Origin::User);
    buffer.edit([(2..2, "c")], ai());
    buffer.edit([(3..3, "d")], Origin::User);
    buffer.edit([(4..4, "e")], Origin::User);
    assert_eq!(text(&buffer), "abcde\n");

    let record = buffer.export_undo().unwrap();
    assert_eq!(record.entries.len(), 2, "the user's after the AI's");
    let mut reopened = Buffer::new("abcde\n");
    reopened.import_undo(record).unwrap();
    reopened.undo(user).unwrap();
    assert_eq!(text(&reopened), "abcd\n");
    reopened.undo(user).unwrap();
    assert_eq!(text(&reopened), "abc\n", "the AI's text stays");
    assert_eq!(reopened.undo(|_| true), Ok(None), "and isn't undoable");

    // With the AI's newest, there's nothing to store.
    buffer.edit([(5..5, "f")], ai());
    assert_eq!(buffer.export_undo(), None);
    // Nor with only the AI's.
    let mut only_ai = Buffer::new("x");
    only_ai.edit([(1..1, "y")], ai());
    assert_eq!(only_ai.export_undo(), None);
    assert_eq!(Buffer::new("x").export_undo(), None, "nor with none");
}

#[test]
fn caps_entries_and_bytes_dropping_the_oldest() {
    let extra = 5;
    let mut buffer = Buffer::new("");
    for n in 0..MAX_ENTRIES + extra {
        let at = buffer.len();
        buffer.edit(
            [(at..at, char::from(b'a' + (n % 26) as u8).to_string())],
            Origin::User,
        );
    }
    let record = buffer.export_undo().unwrap();
    assert_eq!(record.entries.len(), MAX_ENTRIES);
    let mut reopened = Buffer::new(&text(&buffer));
    reopened.import_undo(record).unwrap();
    let (back, _) = walk(&mut reopened);
    assert_eq!(back.len(), MAX_ENTRIES);
    assert_eq!(back.last().unwrap(), "abcde", "the oldest five dropped");

    // Five edits of a MiB each: four don't fit in 4 MiB with their offsets.
    let mib = 1 << 20;
    let mut buffer = Buffer::new("");
    for n in 0..5u8 {
        let at = buffer.len();
        let chunk = char::from(b'0' + n).to_string().repeat(mib);
        buffer.edit([(at..at, chunk)], Origin::User);
    }
    let record = buffer.export_undo().unwrap();
    assert_eq!(record.entries.len(), 3);
    let first = &record.entries[0].batches[0].edits[0];
    assert_eq!(
        (first.range.start, &first.new_text[..1]),
        (2 * mib, "2"),
        "the newest kept"
    );
    assert_eq!(
        record.entries[0].bytes(),
        mib + fe_buffer::undo_store::EDIT_OVERHEAD
    );
    assert!(
        record
            .entries
            .iter()
            .map(|entry| entry.bytes())
            .sum::<usize>()
            <= 4 * mib
    );
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn store_writes_0600_and_ignores_a_corrupt_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = UndoStore::new(dir.path().join("undo"));
    let path = dir.path().join("a.txt");
    let record = typed("one\n", &[(3..3, " two")]).export_undo().unwrap();
    assert_eq!(store.read(&path).unwrap(), None, "none written yet");
    store.write(&path, &record).unwrap();
    let file = store.file_of(&path).unwrap();
    assert_eq!(file.parent(), Some(store.dir()));
    let name = file.file_name().unwrap().to_str().unwrap().to_owned();
    assert!(name.ends_with(".undo") && name.len() == 16 + 5, "{name}");
    #[cfg(unix)]
    {
        assert_eq!(mode(store.dir()), 0o700);
        assert_eq!(mode(&file), 0o600);
    }
    let names: Vec<_> = (fs::read_dir(store.dir()).unwrap())
        .map(|item| item.unwrap().file_name().into_string().unwrap())
        .filter(|name| name != ".lock")
        .collect();
    assert_eq!(names, std::slice::from_ref(&name), "no temporary left");
    assert_eq!(store.read(&path).unwrap().as_ref(), Some(&record));
    assert_eq!(store.read(&dir.path().join("b.txt")).unwrap(), None);

    // A record replaced keeps the newest.
    let newer = typed("one\n", &[(0..0, "zero ")]).export_undo().unwrap();
    store.write(&path, &newer).unwrap();
    assert_eq!(store.read(&path).unwrap(), Some(newer));

    // One byte flipped: ignored, and removed.
    let mut bytes = fs::read(&file).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x55;
    fs::write(&file, &bytes).unwrap();
    assert_eq!(store.read(&path).unwrap(), None);
    assert!(!file.exists(), "a corrupt record is removed");

    // Truncated: likewise.
    store.write(&path, &record).unwrap();
    let bytes = fs::read(&file).unwrap();
    fs::write(&file, &bytes[..bytes.len() - 3]).unwrap();
    assert_eq!(store.read(&path).unwrap(), None);
    assert!(!file.exists());

    // Another format version: ignored, and left for the build that wrote it.
    store.write(&path, &record).unwrap();
    let mut bytes = fs::read(&file).unwrap();
    bytes[8] = bytes[8].wrapping_add(1);
    fs::write(&file, &bytes).unwrap();
    assert_eq!(store.read(&path).unwrap(), None);
    assert!(file.exists(), "an unknown version is kept");

    // Garbage that isn't a record at all.
    fs::write(&file, b"not an undo file").unwrap();
    assert_eq!(store.read(&path).unwrap(), None);
    assert!(!file.exists());
}

fn set_age(path: &Path, now: SystemTime, age: Duration) {
    let file = fs::File::options().write(true).open(path).unwrap();
    file.set_modified(now - age).unwrap();
}

#[test]
fn prune_drops_old_files_and_keeps_the_newest() {
    let dir = tempfile::tempdir().unwrap();
    let store = UndoStore::new(dir.path().join("undo"));
    let now = SystemTime::now();
    assert_eq!(store.prune(now).unwrap(), 0, "no store yet");
    let record: UndoRecord = typed("x", &[(1..1, "y")]).export_undo().unwrap();
    let paths: Vec<_> = (0..=fe_buffer::undo_store::MAX_FILES + 1)
        .map(|n| dir.path().join(format!("{n}.txt")))
        .collect();
    for (n, path) in paths.iter().enumerate() {
        store.write(path, &record).unwrap();
        set_age(
            &store.file_of(path).unwrap(),
            now,
            Duration::from_secs(n as u64),
        );
    }
    // The first written over 30 days ago.
    let stale = &paths[0];
    set_age(
        &store.file_of(stale).unwrap(),
        now,
        Duration::from_secs(31 * 24 * 60 * 60),
    );
    // A crash's temporary file, and one of a write still going.
    let key = store.file_of(&paths[1]).unwrap();
    let key = key.file_name().unwrap().to_str().unwrap();
    let crashed = store.dir().join(format!(".{key}.1-0.tmp"));
    let writing = store.dir().join(format!(".{key}.1-1.tmp"));
    fs::write(&crashed, b"partial").unwrap();
    fs::write(&writing, b"partial").unwrap();
    set_age(&crashed, now, Duration::from_secs(2 * 60 * 60));
    // Not the store's.
    let other = store.dir().join("notes");
    fs::write(&other, b"mine").unwrap();
    set_age(&other, now, Duration::from_secs(365 * 24 * 60 * 60));

    assert_eq!(store.prune(now).unwrap(), 2);
    assert_eq!(store.read(stale).unwrap(), None, "too old");
    let oldest = paths.last().unwrap();
    assert_eq!(store.read(oldest).unwrap(), None, "the oldest beyond 500");
    for path in &paths[1..paths.len() - 1] {
        assert!(store.file_of(path).unwrap().exists(), "{}", path.display());
    }
    assert!(!crashed.exists());
    assert!(
        writing.exists(),
        "a recent temporary may be a write going on"
    );
    assert!(other.exists());
    assert_eq!(store.prune(now).unwrap(), 0);
}

#[test]
fn remove_takes_only_the_files_own_record() {
    let dir = tempfile::tempdir().unwrap();
    let store = UndoStore::new(dir.path().join("undo"));
    let path = dir.path().join("a.txt");
    store.remove(&path).unwrap();
    let record = typed("one\n", &[(3..3, " two")]).export_undo().unwrap();
    store.write(&path, &record).unwrap();
    // Another file whose key is the same: simulated by moving a.txt's
    // record under b.txt's name.
    let other = dir.path().join("b.txt");
    fs::rename(
        store.file_of(&path).unwrap(),
        store.file_of(&other).unwrap(),
    )
    .unwrap();
    store.remove(&other).unwrap();
    assert!(
        store.file_of(&other).unwrap().exists(),
        "a.txt's record stays"
    );
    fs::rename(
        store.file_of(&other).unwrap(),
        store.file_of(&path).unwrap(),
    )
    .unwrap();
    store.remove(&path).unwrap();
    assert_eq!(store.read(&path).unwrap(), None);
    assert!(!store.file_of(&path).unwrap().exists());
}

/// A corrupt record's cleanup decides under the store's lock, so a valid
/// record written while it reads is never the one it removes.
#[test]
fn a_write_racing_a_corrupt_cleanup_survives() {
    let dir = tempfile::tempdir().unwrap();
    let store = UndoStore::new(dir.path().join("undo"));
    let path = dir.path().join("a.txt");
    let record = typed("one\n", &[(3..3, " two")]).export_undo().unwrap();
    store.write(&path, &record).unwrap();
    let file = store.file_of(&path).unwrap();
    for _ in 0..200 {
        fs::write(&file, b"corrupt").unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| store.read(&path).unwrap());
            scope.spawn(|| store.write(&path, &record).unwrap());
        });
        assert_eq!(store.read(&path).unwrap().as_ref(), Some(&record));
    }
}

/// Records that don't fit their text, or couldn't come from a buffer, are
/// refused without touching it, never panicking (review F10).
#[test]
fn a_malformed_record_is_invalid_and_leaves_the_buffer_untouched() {
    let record = typed("one\n", &[(3..3, " two")]).export_undo().unwrap();
    let forge = |change: &dyn Fn(&mut fe_buffer::undo_store::UndoEdit)| {
        let mut forged = record.clone();
        change(&mut forged.entries[0].batches[0].edits[0]);
        forged
    };
    let cases = [
        forge(&|edit| edit.old_text = "\r".into()),
        forge(&|edit| edit.new_text = " t\rwo".into()),
        forge(&|edit| edit.range = usize::MAX - 1..usize::MAX - 1),
        forge(&|edit| edit.range = Range { start: 5, end: 3 }),
        forge(&|edit| edit.range = 100..100),
        forge(&|edit| edit.new_text = String::new()),
    ];
    for (n, forged) in cases.into_iter().enumerate() {
        let mut buffer = Buffer::new("one two\n");
        let version = buffer.version();
        assert_eq!(
            buffer.import_undo(forged),
            Err(UndoImportError::Invalid),
            "case {n}"
        );
        assert_eq!(
            (text(&buffer), buffer.version()),
            ("one two\n".into(), version)
        );
        assert_eq!(buffer.undo(user), Ok(None));
    }
}

/// Importing into a buffer whose history limit is smaller than the record
/// rebuilds the whole history before the limit applies, so the text ends as
/// it was, undone entries and all, and only the oldest entries go (review
/// round 4 F6).
#[test]
fn a_small_history_limit_leaves_the_imported_text_unchanged() {
    let mut original = Buffer::new("");
    for word in ["one ", "two ", "three ", "four "] {
        let at = original.len();
        original.edit([(at..at, word)], Origin::User);
    }
    original.undo(user).unwrap();
    original.undo(user).unwrap();
    assert_eq!(text(&original), "one two ");
    let record = original.export_undo().unwrap();
    assert_eq!(
        (record.entries.iter().filter(|entry| entry.undone)).count(),
        2
    );

    let mut reopened = Buffer::new("one two ");
    reopened.set_history_limit(1);
    reopened.import_undo(record).unwrap();
    assert_eq!(text(&reopened), "one two ", "the text is unchanged");
    assert!(!reopened.is_dirty());
    // The undo position is where it was, after the applied entries and
    // before the undone ones: the limit then drops the oldest, as it would
    // have had they been typed here, so none applied is left to undo and
    // what it keeps is still undone.
    let kept = reopened.history_times(user);
    assert!(
        !kept.is_empty() && kept.iter().all(|time| time.undone),
        "{kept:?}"
    );
    assert_eq!(reopened.undo(user), Ok(None));
    assert_eq!(text(&reopened), "one two ");
}
