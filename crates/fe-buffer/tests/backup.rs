//! Backups of unsaved buffers: the on-disk store and restoring from it.

use std::{fs, io::ErrorKind, path::PathBuf};

use fe_buffer::{Backup, BackupId, BackupMeta, BackupStore, Buffer, DiskStamp, LineEnding, Origin};
use fe_text::Encoding;

fn user(buffer: &mut Buffer, range: std::ops::Range<usize>, text: &str) {
    buffer.edit([(range, text)], Origin::User);
    buffer.end_transaction();
}

fn write(buffer: &Buffer) -> Vec<u8> {
    let mut out = Vec::new();
    buffer.write_to(&mut out).unwrap();
    out
}

fn windows_1252(text: &str) -> Vec<u8> {
    let (bytes, _, unmappable) = Encoding::for_label(b"windows-1252").unwrap().encode(text);
    assert!(!unmappable);
    bytes.into_owned()
}

fn utf16le_bom(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

/// Opens `bytes` written to `path` and edits it, as the editor would.
fn edited(path: &std::path::Path, bytes: &[u8]) -> Buffer {
    fs::write(path, bytes).unwrap();
    let mut buffer = Buffer::from_file(fs::File::open(path).unwrap()).unwrap();
    user(&mut buffer, 0..0, "Noël ");
    buffer
}

#[cfg(unix)]
fn mode(path: &std::path::Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn write_then_read_round_trips_text_and_meta() {
    let files = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    let dir = state.path().join("nested/backups");
    let store = BackupStore::new(&dir);
    assert_eq!(store.ids().unwrap(), []);

    let mut cases: Vec<(Buffer, Option<PathBuf>, &str)> = Vec::new();
    for (name, bytes, label) in [
        (
            "lf.txt",
            "héllo → 日本語 😀\nsecond\n".as_bytes().to_vec(),
            "UTF-8",
        ),
        ("crlf.txt", b"one\r\ntwo\r\nthree".to_vec(), "UTF-8"),
        ("bom.txt", b"\xEF\xBB\xBFbom\n".to_vec(), "UTF-8 with BOM"),
        (
            "french.txt",
            windows_1252("Le café, à côté.\r\n"),
            "windows-1252",
        ),
        ("wide.txt", utf16le_bom("héllo 😀\r\nworld\r\n"), "UTF-16LE"),
        // Malformed UTF-8: read as U+FFFD, which a save must still refuse.
        (
            "lossy.txt",
            b"\xEF\xBB\xBFok \xFF here\n".to_vec(),
            "UTF-8 with BOM",
        ),
    ] {
        let path = files.path().join(name);
        let buffer = edited(&path, &bytes);
        assert_eq!(buffer.encoding().label(), label, "{name}");
        cases.push((buffer, Some(path), name));
    }
    let mut untitled = Buffer::new("");
    user(
        &mut untitled,
        0..0,
        "scratch\n\ttabbed \"quoted\" \\ text\n",
    );
    cases.push((untitled, None, "Untitled \"1\" \\ ⌘\n"));
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = files
            .path()
            .join(std::ffi::OsStr::from_bytes(b"not-utf8-\xFF.txt"));
        // APFS refuses non-UTF-8 names with EILSEQ (92); no other target gets a fallback.
        #[cfg(target_vendor = "apple")]
        const EILSEQ: Option<i32> = Some(92);
        #[cfg(not(target_vendor = "apple"))]
        const EILSEQ: Option<i32> = None;
        let buffer = match fs::write(&path, b"bytes\n") {
            Ok(()) => edited(&path, b"bytes\n"),
            Err(error) if EILSEQ.is_some() && error.raw_os_error() == EILSEQ => {
                // Round-trip the path without a disk file.
                let mut buffer = Buffer::new("");
                buffer.set_disk_stamp(None);
                user(&mut buffer, 0..0, "Noël bytes\n");
                buffer
            }
            Err(error) => panic!("creating {path:?}: {error}"),
        };
        cases.push((buffer, Some(path), "not-utf8"));
    }
    // A file deleted since it was read: known missing.
    let mut gone = Buffer::new("gone");
    gone.set_disk_stamp(None);
    cases.push((gone, Some(files.path().join("gone.txt")), "gone"));

    for (buffer, path, title) in &cases {
        let id = BackupId::new();
        let meta = BackupMeta::for_buffer(buffer, path.clone(), *title);
        assert_eq!(meta.disk_stamp, buffer.disk_stamp());
        store.write(id, &meta, buffer.text()).unwrap();
        let backup = store.read(id).unwrap();
        assert_eq!(backup.id, id);
        assert_eq!(backup.meta, meta, "{title}");
        assert_eq!(
            backup.text.to_string(),
            buffer.text().to_string(),
            "{title}"
        );

        // Restored, it saves exactly what the buffer would have.
        let disk = path.as_ref().and_then(|path| fs::File::open(path).ok());
        let restored = Buffer::from_backup(disk, &backup).unwrap();
        assert_eq!(restored.encoding(), buffer.encoding(), "{title}");
        assert_eq!(restored.line_ending(), buffer.line_ending(), "{title}");
        match buffer.write_to(Vec::new()) {
            Ok(()) => assert!(write(&restored) == write(buffer), "{title}"),
            Err(error) => {
                assert_eq!(error.kind(), ErrorKind::InvalidData, "{title}");
                let restored = restored.write_to(Vec::new()).unwrap_err();
                assert_eq!(restored.to_string(), error.to_string(), "{title}");
            }
        }

        #[cfg(unix)]
        for ext in ["txt", "toml"] {
            assert_eq!(mode(&dir.join(format!("{id}.{ext}"))), 0o600, "{id}.{ext}");
        }
    }
    let lossy = &cases.iter().find(|case| case.2 == "lossy.txt").unwrap().0;
    assert_eq!(
        lossy.write_to(Vec::new()).unwrap_err().kind(),
        ErrorKind::InvalidData
    );
    assert_eq!(store.ids().unwrap().len(), cases.len());
    #[cfg(unix)]
    assert_eq!(mode(&dir), 0o700);
    // Only the pairs and the lock: no temporary files left behind.
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 2 * cases.len() + 1);
    #[cfg(unix)]
    assert_eq!(mode(&dir.join(".lock")), 0o600);
}

#[test]
fn rewriting_a_backup_replaces_it() {
    let state = tempfile::tempdir().unwrap();
    let store = BackupStore::new(state.path());
    let mut buffer = Buffer::new("first");
    let id = BackupId::new();
    store
        .write(
            id,
            &BackupMeta::for_buffer(&buffer, None, "a"),
            buffer.text(),
        )
        .unwrap();
    user(&mut buffer, 5..5, " and second");
    buffer.set_line_ending(LineEnding::CrLf);
    let meta = BackupMeta::for_buffer(&buffer, None, "b");
    store.write(id, &meta, buffer.text()).unwrap();
    let backup = store.read(id).unwrap();
    assert_eq!(backup.text.to_string(), "first and second");
    assert_eq!(backup.meta, meta);
    assert_eq!(store.ids().unwrap(), [id]);
}

#[test]
fn a_backup_whose_text_and_meta_disagree_is_invalid() {
    let state = tempfile::tempdir().unwrap();
    let store = BackupStore::new(state.path());
    let id = BackupId::new();
    let buffer = Buffer::new("some text");
    store
        .write(
            id,
            &BackupMeta::for_buffer(&buffer, None, "t"),
            buffer.text(),
        )
        .unwrap();
    // Text the meta wasn't written with, of the same length or not.
    for other in ["else text", "other text, longer"] {
        fs::write(state.path().join(format!("{id}.txt")), other).unwrap();
        assert_eq!(store.read(id).unwrap_err().kind(), ErrorKind::InvalidData);
    }
    fs::write(state.path().join(format!("{id}.toml")), "version = \"x\"").unwrap();
    assert_eq!(store.read(id).unwrap_err().kind(), ErrorKind::InvalidData);
    assert_eq!(
        store.read(BackupId::new()).unwrap_err().kind(),
        ErrorKind::NotFound
    );
}

#[test]
fn half_written_backups_are_not_listed_and_are_pruned() {
    let state = tempfile::tempdir().unwrap();
    let dir = state.path();
    let store = BackupStore::new(dir);
    let buffer = Buffer::new("text");
    let ids: Vec<BackupId> = (0..5).map(|_| BackupId::new()).collect();
    for &id in &ids {
        let meta = BackupMeta::for_buffer(&buffer, None, id.to_string());
        store.write(id, &meta, buffer.text()).unwrap();
    }
    let [a, no_meta, no_text, d, e] = ids[..] else {
        unreachable!()
    };
    fs::remove_file(dir.join(format!("{no_meta}.toml"))).unwrap();
    fs::remove_file(dir.join(format!("{no_text}.txt"))).unwrap();
    // A crashed process's temporary file, and files the store didn't name.
    let stray_temp = dir.join(format!(".{a}.txt.4294967295-0.tmp"));
    fs::write(&stray_temp, "half").unwrap();
    for other in [
        "notes.txt",
        "01-2-3.txt",
        "x.toml",
        ".notes.tmp",
        ".x.txt.1-2.tmp",
    ] {
        fs::write(dir.join(other), "keep me").unwrap();
    }

    assert_eq!(store.ids().unwrap(), [a, d, e]);
    assert_eq!(store.read(no_meta).unwrap_err().kind(), ErrorKind::NotFound);
    assert_eq!(store.read(no_text).unwrap_err().kind(), ErrorKind::NotFound);

    store
        .prune(|id| *id == a || *id == e || *id == no_meta)
        .unwrap();
    assert_eq!(store.ids().unwrap(), [a, e]);
    let mut left: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    left.sort();
    let mut expected = vec![
        format!("{a}.toml"),
        format!("{a}.txt"),
        format!("{e}.toml"),
        format!("{e}.txt"),
        "01-2-3.txt".into(),
        ".notes.tmp".into(),
        ".x.txt.1-2.tmp".into(),
        ".lock".into(),
        "notes.txt".into(),
        "x.toml".into(),
    ];
    expected.sort();
    assert_eq!(left, expected);

    store.remove(a).unwrap();
    store.remove(a).unwrap();
    assert_eq!(store.ids().unwrap(), [e]);
    store.prune(|_| false).unwrap();
    assert_eq!(store.ids().unwrap(), []);

    // A store whose directory was never made is empty.
    let unmade = BackupStore::new(dir.join("unmade"));
    assert_eq!(unmade.ids().unwrap(), []);
    unmade.prune(|_| true).unwrap();
}

fn back_up(store: &BackupStore, buffer: &Buffer, path: Option<PathBuf>) -> Backup {
    let id = BackupId::new();
    let meta = BackupMeta::for_buffer(buffer, path, "Untitled-1");
    store.write(id, &meta, buffer.text()).unwrap();
    store.read(id).unwrap()
}

#[test]
fn restoring_over_the_disk_file_is_one_undoable_edit() {
    let files = tempfile::tempdir().unwrap();
    let store = BackupStore::new(files.path().join("backups"));
    let path = files.path().join("f.txt");
    fs::write(&path, "one\ntwo\nthree\n").unwrap();
    let mut buffer = Buffer::from_file(fs::File::open(&path).unwrap()).unwrap();
    user(&mut buffer, 4..7, "TWO");
    user(&mut buffer, 14..14, "four\n");
    let backup = back_up(&store, &buffer, Some(path.clone()));

    let mut restored = Buffer::from_backup(Some(fs::File::open(&path).unwrap()), &backup).unwrap();
    assert_eq!(restored.text().to_string(), "one\nTWO\nthree\nfour\n");
    assert!(restored.is_dirty());
    restored
        .undo(|origin| *origin == Origin::Host("backup".into()))
        .unwrap()
        .unwrap();
    assert_eq!(restored.text().to_string(), "one\ntwo\nthree\n");
    assert!(!restored.is_dirty());
    restored.redo(|_| true).unwrap().unwrap();
    assert_eq!(restored.text().to_string(), "one\nTWO\nthree\nfour\n");

    // The file changed since the backup: the backup goes over the new text,
    // and undo returns to it.
    fs::write(&path, "one\ntwo\nthree\nand more\n").unwrap();
    let mut restored = Buffer::from_backup(Some(fs::File::open(&path).unwrap()), &backup).unwrap();
    assert_eq!(restored.text().to_string(), "one\nTWO\nthree\nfour\n");
    assert!(restored.is_dirty());
    restored.undo(|_| true).unwrap().unwrap();
    assert_eq!(restored.text().to_string(), "one\ntwo\nthree\nand more\n");
    assert!(!restored.is_dirty());

    // A backup that saves just what the file holds is still a restore:
    // dirty, with one undo to the file.
    let same = back_up(&store, &restored, Some(path.clone()));
    let mut restored = Buffer::from_backup(Some(fs::File::open(&path).unwrap()), &same).unwrap();
    assert_eq!(restored.text().to_string(), "one\ntwo\nthree\nand more\n");
    assert!(restored.is_dirty());
    let undone = restored.undo(|_| true).unwrap();
    assert!(undone.is_some());
    assert!(!restored.is_dirty());
    assert_eq!(restored.redo(|_| true).unwrap(), undone);
    assert!(restored.is_dirty());
    // Retyping the same text doesn't undo the restore; a save ends it.
    user(&mut restored, 0..1, "o");
    assert!(restored.is_dirty());
    restored.did_save();
    assert!(!restored.is_dirty());
}

#[test]
fn restoring_the_same_text_in_another_save_format_is_dirty() {
    let files = tempfile::tempdir().unwrap();
    let store = BackupStore::new(files.path().join("backups"));
    let path = files.path().join("f.txt");
    fs::write(&path, "plain\r\n").unwrap();
    let buffer = Buffer::from_file(fs::File::open(&path).unwrap()).unwrap();
    let original = write(&buffer);
    let id = BackupId::new();
    let changes: [fn(&mut BackupMeta); 3] = [
        |meta| meta.encoding.bom = true,
        |meta| meta.encoding.encoding = Encoding::for_label(b"utf-16le").unwrap(),
        |meta| meta.line_ending = LineEnding::Lf,
    ];
    for change in changes {
        let mut meta = BackupMeta::for_buffer(&buffer, Some(path.clone()), "f.txt");
        change(&mut meta);
        store.write(id, &meta, buffer.text()).unwrap();
        let backup = store.read(id).unwrap();
        let mut restored =
            Buffer::from_backup(Some(fs::File::open(&path).unwrap()), &backup).unwrap();
        assert_eq!(restored.text().to_string(), "plain\n");
        assert!(restored.is_dirty(), "{meta:?}");
        assert_eq!(restored.encoding(), meta.encoding);
        assert_eq!(restored.line_ending(), meta.line_ending);
        let restored_bytes = write(&restored);
        assert_ne!(restored_bytes, original);

        // One undo: the file's text and save format, clean.
        let undone = restored
            .undo(|origin| *origin == Origin::Host("backup".into()))
            .unwrap();
        assert!(undone.is_some(), "{meta:?}");
        assert!(!restored.is_dirty(), "{meta:?}");
        assert_eq!(restored.encoding(), buffer.encoding());
        assert_eq!(restored.line_ending(), buffer.line_ending());
        assert_eq!(write(&restored), original);
        // Redo: the backup's again.
        assert_eq!(restored.redo(|_| true).unwrap(), undone);
        assert!(restored.is_dirty(), "{meta:?}");
        assert_eq!(write(&restored), restored_bytes);
    }
}

#[test]
fn restoring_without_a_file_is_untitled_style() {
    let files = tempfile::tempdir().unwrap();
    let store = BackupStore::new(files.path());
    let mut buffer = Buffer::new("");
    user(&mut buffer, 0..0, "draft\nideas\n");
    let backup = back_up(&store, &buffer, None);
    assert_eq!(backup.meta.path, None);
    assert_eq!(backup.meta.title, "Untitled-1");

    let mut restored = Buffer::from_backup(None, &backup).unwrap();
    assert_eq!(restored.text().to_string(), "draft\nideas\n");
    assert!(restored.is_dirty());
    assert!(!restored.disk_stamp_known());
    restored.undo(|_| true).unwrap().unwrap();
    assert_eq!(restored.text().to_string(), "");
    assert!(!restored.is_dirty());

    // A file deleted since: the backup over no text, still known missing
    // or as it was, so a file made there since isn't overwritten unasked.
    let path = files.path().join("deleted.txt");
    fs::write(&path, "was here\n").unwrap();
    let mut buffer = Buffer::from_file(fs::File::open(&path).unwrap()).unwrap();
    user(&mut buffer, 0..0, "edited, ");
    let backup = back_up(&store, &buffer, Some(path.clone()));
    fs::remove_file(&path).unwrap();
    let restored = Buffer::from_backup(None, &backup).unwrap();
    assert_eq!(restored.text().to_string(), "edited, was here\n");
    assert!(restored.is_dirty());
    assert!(restored.disk_stamp_known());
    assert_eq!(restored.disk_stamp(), backup.meta.disk_stamp);
}

#[test]
fn the_restored_disk_stamp_is_the_backups() {
    let files = tempfile::tempdir().unwrap();
    let store = BackupStore::new(files.path().join("backups"));
    let path = files.path().join("f.txt");
    fs::write(&path, "original\n").unwrap();
    let mut buffer = Buffer::from_file(fs::File::open(&path).unwrap()).unwrap();
    user(&mut buffer, 0..0, "unsaved ");
    let backup = back_up(&store, &buffer, Some(path.clone()));
    let stamp = DiskStamp::of(&path).unwrap();
    assert!(stamp.is_some());
    assert_eq!(backup.meta.disk_stamp, stamp);

    // Unchanged on disk: a save's check (`SaveJob::run`, comparing the
    // file's stamp with the buffer's) passes.
    let restored = Buffer::from_backup(Some(fs::File::open(&path).unwrap()), &backup).unwrap();
    assert!(restored.disk_stamp_known());
    assert_eq!(restored.disk_stamp(), stamp);
    assert_eq!(DiskStamp::of(&path).unwrap(), restored.disk_stamp());

    // Changed on disk since the backup: the restored buffer, read from the
    // new file, still has the backup's stamp, so the save is refused.
    fs::write(&path, "changed elsewhere, longer\n").unwrap();
    let restored = Buffer::from_backup(Some(fs::File::open(&path).unwrap()), &backup).unwrap();
    assert_eq!(restored.disk_stamp(), stamp);
    let now = DiskStamp::of(&path).unwrap();
    assert!(now.is_some());
    assert_ne!(now, restored.disk_stamp());
}
