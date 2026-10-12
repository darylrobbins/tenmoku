//! Saving from a [`SaveSnapshot`] off the buffer's thread, and reloading a
//! buffer from disk as one undoable edit.

use std::io::ErrorKind;

use fe_buffer::{Bias, Buffer, BufferEvent, DiskStamp, LineEnding, Origin, SaveSnapshot, Text};
use fe_text::Encoding;

const FRENCH: &str = "Le café est très bon. Une idée naïve, déjà vue à côté de l'église.\n";

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

fn user(buffer: &mut Buffer, range: std::ops::Range<usize>, text: &str) {
    buffer.edit([(range, text)], Origin::User);
    buffer.end_transaction();
}

fn undo(buffer: &mut Buffer) {
    buffer.undo(|o| *o == Origin::User).unwrap().unwrap();
}

/// Writes `snapshot` on another thread, as a save job does.
fn write_elsewhere(snapshot: SaveSnapshot) -> std::io::Result<Vec<u8>> {
    std::thread::spawn(move || {
        let mut out = Vec::new();
        snapshot.write_to(&mut out).map(|()| out)
    })
    .join()
    .unwrap()
}

fn write(buffer: &Buffer) -> std::io::Result<Vec<u8>> {
    let mut out = Vec::new();
    buffer.write_to(&mut out).map(|()| out)
}

#[test]
fn snapshot_writes_like_write_to() {
    fn assert_send_static<T: Send + 'static>() {}
    assert_send_static::<SaveSnapshot>();

    let files: [(Vec<u8>, &str); 4] = [
        ("héllo → 日本語 😀\nsecond\n".into(), "UTF-8"),
        ("one\r\ntwo\r\nthree".into(), "UTF-8"),
        (windows_1252(FRENCH), "windows-1252"),
        (utf16le_bom("héllo 😀\r\nworld\r\n"), "UTF-16LE"),
    ];
    for (bytes, label) in files {
        let mut buffer = Buffer::from_reader_detecting(&bytes[..]).unwrap();
        assert_eq!(buffer.encoding().label(), label);
        user(&mut buffer, 0..0, "Noël ");
        let expected = write(&buffer).unwrap();
        assert_ne!(expected, bytes, "{label}: the edit is written");
        assert!(
            write_elsewhere(buffer.save_snapshot()).unwrap() == expected,
            "{label}"
        );
    }
    let crlf = Buffer::from_reader_detecting(&b"a\r\nb\r\n"[..]).unwrap();
    assert_eq!(crlf.line_ending(), LineEnding::CrLf);
    assert_eq!(
        write_elsewhere(crlf.save_snapshot()).unwrap(),
        b"a\r\nb\r\n"
    );

    // Malformed bytes read as U+FFFD, still in the text.
    let lossy = Buffer::from_reader_detecting(&b"\xEF\xBB\xBFok \xFF here"[..]).unwrap();
    // A char windows-1252 can't hold.
    let mut unmappable = Buffer::from_reader_detecting(&windows_1252(FRENCH)[..]).unwrap();
    user(&mut unmappable, 3..3, "😀");
    for buffer in [lossy, unmappable] {
        let direct = write(&buffer).unwrap_err();
        let elsewhere = write_elsewhere(buffer.save_snapshot()).unwrap_err();
        assert_eq!(direct.kind(), ErrorKind::InvalidData);
        assert_eq!(elsewhere.kind(), ErrorKind::InvalidData);
        assert_eq!(elsewhere.to_string(), direct.to_string());
    }
}

#[test]
fn saved_snapshot_dirty_state() {
    // Edits after the snapshot: dirty, until they're undone.
    let mut buffer = Buffer::new("hello world");
    user(&mut buffer, 0..0, "say ");
    let snapshot = buffer.save_snapshot();
    let events = buffer.subscribe();
    user(&mut buffer, 9..9, ",");
    user(&mut buffer, 15..15, "!");
    buffer.did_save_snapshot(&snapshot, None);
    assert!(buffer.is_dirty());
    let saved: Vec<_> = events
        .drain()
        .into_iter()
        .filter_map(|event| match event {
            BufferEvent::Saved { version } => Some(version),
            BufferEvent::Edited { .. } => None,
        })
        .collect();
    assert_eq!(saved, [snapshot.version()]);
    undo(&mut buffer);
    assert!(buffer.is_dirty());
    undo(&mut buffer);
    assert!(!buffer.is_dirty(), "back to the saved snapshot's text");
    undo(&mut buffer);
    assert!(buffer.is_dirty(), "before the saved text");

    // Same length edits, retyped back.
    let mut buffer = Buffer::new("abc");
    let snapshot = buffer.save_snapshot();
    user(&mut buffer, 0..1, "X");
    buffer.did_save_snapshot(&snapshot, None);
    assert!(buffer.is_dirty());
    user(&mut buffer, 0..1, "a");
    assert!(!buffer.is_dirty());

    // No edits since: as did_save.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    std::fs::write(&path, "x").unwrap();
    let stamp = DiskStamp::of(&path).unwrap();
    assert!(stamp.is_some());
    let mut a = Buffer::new("one");
    let mut b = Buffer::new("one");
    user(&mut a, 3..3, " two");
    user(&mut b, 3..3, " two");
    let (a_events, b_events) = (a.subscribe(), b.subscribe());
    let snapshot = a.save_snapshot();
    a.did_save_snapshot(&snapshot, stamp);
    b.did_save();
    assert!(!a.is_dirty() && !b.is_dirty());
    assert_eq!(a.disk_stamp(), stamp);
    for (events, version) in [(a_events, a.version()), (b_events, b.version())] {
        assert!(matches!(
            events.drain()[..],
            [BufferEvent::Saved { version: saved }] if saved == version
        ));
    }
    for buffer in [&mut a, &mut b] {
        undo(buffer);
        assert!(buffer.is_dirty());
        buffer.redo(|o| *o == Origin::User).unwrap().unwrap();
        assert!(!buffer.is_dirty());
    }
}

#[test]
fn reload_is_one_edit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("f.txt");
    let old = "first row\nmiddle row\nlast row\n";
    std::fs::write(&path, old).unwrap();
    let mut buffer = Buffer::from_file(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(buffer.disk_stamp(), DiskStamp::of(&path).unwrap());
    assert!(buffer.disk_stamp().is_some());
    let events = buffer.subscribe();

    let snapshot = buffer.snapshot();
    let first = snapshot.anchor_at(6, Bias::Right);
    let last_offset = old.find("last").unwrap() + 2;
    let last = snapshot.anchor_at(last_offset, Bias::Left);

    // Another program rewrites the middle row, in windows-1252 with CRLF.
    let new = "first row\r\nthe café row, longer\r\nlast row\r\n";
    std::fs::write(&path, windows_1252(new)).unwrap();
    let (file, stamp) = Buffer::read_file(std::fs::File::open(&path).unwrap()).unwrap();
    assert_eq!(Some(stamp), DiskStamp::of(&path).unwrap());
    let id = buffer.reload(file, Some(stamp), Origin::Format);
    assert!(id.is_some());
    let normalized = new.replace("\r\n", "\n");
    assert_eq!(buffer.text().to_string(), normalized);
    assert_eq!(buffer.line_ending(), LineEnding::CrLf);
    assert_eq!(buffer.encoding().label(), "windows-1252");
    assert!(!buffer.is_dirty());
    assert_eq!(buffer.disk_stamp(), Some(stamp));
    assert_eq!(write(&buffer).unwrap(), windows_1252(new));

    // One edit, over the middle row only.
    let edits: Vec<_> = events
        .drain()
        .into_iter()
        .filter_map(|event| match event {
            BufferEvent::Edited {
                transaction,
                origin,
                edits,
                ..
            } => Some((transaction, origin, edits)),
            BufferEvent::Saved { .. } => None,
        })
        .collect();
    assert_eq!(edits.len(), 1);
    assert_eq!((Some(edits[0].0), &edits[0].1), (id, &Origin::Format));
    assert_eq!(edits[0].2.len(), 1);
    let current = buffer.snapshot();
    assert_eq!(current.resolve(&first), 6);
    assert_eq!(current.resolve(&last), normalized.find("last").unwrap() + 2);

    // One undo step of its origin brings the old text back, dirty.
    assert!(buffer.undo(|o| *o == Origin::User).unwrap().is_none());
    assert_eq!(buffer.undo(|o| *o == Origin::Format).unwrap(), id);
    assert_eq!(buffer.text().to_string(), old);
    assert!(buffer.is_dirty());
    buffer.redo(|o| *o == Origin::Format).unwrap().unwrap();
    assert!(!buffer.is_dirty());

    // Identical text: no edit, but adopted and clean.
    user(&mut buffer, 0..0, "x");
    user(&mut buffer, 0..1, "");
    buffer.set_line_ending(LineEnding::Lf);
    assert!(buffer.is_dirty(), "line ending changed");
    events.drain();
    let version = buffer.version();
    let file = Text::from_reader_detecting(&windows_1252(new)[..]).unwrap();
    assert_eq!(buffer.reload(file, None, Origin::User), None);
    assert_eq!(buffer.version(), version);
    assert!(!buffer.is_dirty());
    assert_eq!(buffer.line_ending(), LineEnding::CrLf);
    assert_eq!(buffer.disk_stamp(), None);
    assert!(matches!(events.drain()[..], [BufferEvent::Saved { .. }]));

    // Multibyte chars at the edges of the change stay whole.
    let mut buffer = Buffer::new("aé日b");
    let file = Text::from_reader_detecting("aè日b".as_bytes()).unwrap();
    buffer.reload(file, None, Origin::User).unwrap();
    assert_eq!(buffer.text().to_string(), "aè日b");
    let mut buffer = Buffer::new("x");
    let file = Text::from_reader_detecting("".as_bytes()).unwrap();
    buffer.reload(file, None, Origin::User).unwrap();
    assert_eq!(buffer.text().to_string(), "");
}

#[test]
fn reload_compares_long_texts_by_window() {
    let row = "0123456789abcdef".repeat(8) + "\n";
    let head = row.repeat(3000);
    let tail = "é".repeat(50_000) + &row.repeat(3000);
    let mut buffer = Buffer::new(&format!("{head}old{tail}"));
    let events = buffer.subscribe();
    let file = Text::from_reader_detecting(format!("{head}newer{tail}").as_bytes()).unwrap();
    buffer.reload(file, None, Origin::User).unwrap();
    let BufferEvent::Edited { edits, .. } = &events.drain()[0] else {
        panic!("an edit first");
    };
    assert_eq!(edits.len(), 1);
    assert_eq!(edits[0].old, head.len()..head.len() + 3);
    assert_eq!(edits[0].new, head.len()..head.len() + 5);
    assert_eq!(buffer.text().to_string(), format!("{head}newer{tail}"));
}

#[test]
fn reload_splits_an_open_transaction_of_its_origin() {
    let mut buffer = Buffer::new("one\ntwo\n");
    let outer = buffer.start_transaction(Origin::User);
    buffer.start_transaction(Origin::User);
    buffer.edit([(0..0, "> ")], Origin::User);
    let file = Text::from_reader_detecting("> one\nTWO\n".as_bytes()).unwrap();
    let reload = buffer.reload(file, None, Origin::User).unwrap();
    assert_ne!(reload, outer);
    // Typing over the reloaded text, still inside the caller's nesting.
    buffer.edit([(6..9, "2")], Origin::User);
    let inner = buffer.open_transaction().unwrap();
    assert_ne!(inner, outer);
    assert_eq!(buffer.end_transaction(), None, "still nested");
    assert_eq!(buffer.end_transaction(), Some(inner));
    assert_eq!(buffer.open_transaction(), None);

    // Undo goes back through them in order, without conflicts.
    assert_eq!(buffer.undo(|o| *o == Origin::User).unwrap(), Some(inner));
    assert_eq!(buffer.text().to_string(), "> one\nTWO\n");
    assert!(!buffer.is_dirty());
    assert_eq!(buffer.undo(|o| *o == Origin::User).unwrap(), Some(reload));
    assert_eq!(buffer.text().to_string(), "> one\ntwo\n");
    assert_eq!(buffer.undo(|o| *o == Origin::User).unwrap(), Some(outer));
    assert_eq!(buffer.text().to_string(), "one\ntwo\n");

    // Another origin's open transaction is set aside, and kept open.
    let mut buffer = Buffer::new("one\n");
    let format = buffer.start_transaction(Origin::Format);
    let file = Text::from_reader_detecting("ONE\n".as_bytes()).unwrap();
    let reload = buffer.reload(file, None, Origin::User).unwrap();
    assert_ne!(reload, format);
    assert_eq!(buffer.open_transaction(), Some(format));
}
