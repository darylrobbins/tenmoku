//! `FileText` against `Text` over the same bytes, and against a file that
//! changes underneath it.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::ops::Range;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use fe_fileview::{CHUNK_BYTES, FileText, FileView, IndexState, LineIndex, ReadErrorKind};
use fe_text::{Bias, Point, Text, TextSource};
use rand::{Rng, SeedableRng, rngs::StdRng};

const MIB: usize = 1024 * 1024;

/// About `len` bytes of rows drawn from a pool of rows of 0 to 80 pieces, so
/// some rows are empty, each followed by `newline`. Without a final newline
/// the file ends in `end`.
fn generate(
    len: usize,
    pieces: &[&[u8]],
    newline: &[u8],
    final_newline: bool,
    rng: &mut StdRng,
) -> Vec<u8> {
    let pool: Vec<Vec<u8>> = (0..1000)
        .map(|_| {
            (0..rng.random_range(0..80))
                .flat_map(|_| pieces[rng.random_range(0..pieces.len())])
                .copied()
                .collect()
        })
        .collect();
    let mut bytes = Vec::with_capacity(len + 1024);
    while bytes.len() < len {
        bytes.extend_from_slice(&pool[rng.random_range(0..pool.len())]);
        bytes.extend_from_slice(newline);
    }
    if !final_newline {
        bytes.extend_from_slice(b"end");
    }
    bytes
}

const VALID: [&[u8]; 5] = [
    b"text ",
    b"x",
    "é".as_bytes(),
    "€".as_bytes(),
    "😀".as_bytes(),
];

fn open(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> Arc<FileView> {
    let path = dir.path().join(name);
    fs::write(&path, bytes).unwrap();
    Arc::new(FileView::open(&path).unwrap())
}

/// A snapshot of a complete index of `view`.
fn complete(view: &Arc<FileView>) -> FileText {
    let index = Arc::new(LineIndex::build(Arc::clone(view)));
    let deadline = Instant::now() + Duration::from_secs(30);
    while index.progress().state == IndexState::Building {
        assert!(Instant::now() < deadline, "still building");
        thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(index.progress().state, IndexState::Complete);
    FileText::new(Arc::clone(view), index)
}

fn joined(source: &dyn TextSource, range: Range<usize>) -> String {
    source.chunks_in_range(range).collect()
}

/// Each row's start, length and text, as `Text` gives them.
fn assert_rows_match(source: &dyn TextSource, text: &Text) {
    assert_eq!(source.len(), text.len());
    assert_eq!(source.row_count(), text.row_count());
    assert_eq!(source.max_point(), text.max_point());
    for row in 0..=text.row_count() {
        let context = format!("row {row}");
        assert_eq!(source.row_start(row), text.row_start(row), "{context}");
        assert_eq!(source.row_len(row), text.row_len(row), "{context}");
        assert_eq!(
            source.row_to_string(row),
            text.row_to_string(row),
            "{context}"
        );
    }
}

#[test]
fn matches_text_on_valid_lf_files() {
    let dir = tempfile::tempdir().unwrap();
    // One thread per file: a debug build walks rows slowly.
    thread::scope(|scope| {
        for final_newline in [true, false] {
            let dir = &dir;
            scope.spawn(move || {
                let mut rng = StdRng::seed_from_u64(271 + final_newline as u64);
                let bytes = generate(8 * MIB, &VALID, b"\n", final_newline, &mut rng);
                let contents = String::from_utf8(bytes).unwrap();
                let view = open(dir, &format!("{final_newline}.txt"), contents.as_bytes());
                let source = complete(&view);
                let text = Text::new(&contents);
                assert_rows_match(&source, &text);
                assert_matches_at_random(&source, &text, &mut rng);
            });
        }
    });
}

/// 1000 random offsets, points and ranges.
fn assert_matches_at_random(source: &dyn TextSource, text: &Text, rng: &mut StdRng) {
    let len = text.len();
    for _ in 0..1000 {
        let offset = rng.random_range(0..=len + 8);
        let context = format!("offset {offset}");
        assert_eq!(
            source.offset_to_point(offset),
            text.offset_to_point(offset),
            "{context}"
        );
        for bias in [Bias::Left, Bias::Right] {
            assert_eq!(
                source.clip_offset(offset, bias),
                text.clip_offset(offset, bias),
                "{context}, {bias:?}"
            );
        }
        if offset <= len {
            assert_eq!(
                source.is_char_boundary(offset),
                text.is_char_boundary(offset),
                "{context}"
            );
            if text.is_char_boundary(offset) {
                assert_eq!(source.char_at(offset), text.char_at(offset), "{context}");
            }
        }

        let row = rng.random_range(0..text.row_count() + 2);
        let point = Point::new(row, rng.random_range(0..200));
        assert_eq!(
            source.point_to_offset(point),
            text.point_to_offset(point),
            "{point:?}"
        );

        let a = text.clip_offset(rng.random_range(0..=len), Bias::Left);
        // Mostly short ranges, and now and then one across chunks.
        let span = if rng.random_bool(0.9) {
            300
        } else {
            3 * CHUNK_BYTES as usize
        };
        let b = text.clip_offset(a + rng.random_range(0..=span), Bias::Left);
        let expected = text.slice_to_string(a..b);
        assert_eq!(source.slice_to_string(a..b), expected, "{a}..{b}");
        assert_eq!(joined(source, a..b), expected, "{a}..{b}");
    }
}

#[test]
fn crlf_and_invalid_bytes_keep_file_offsets() {
    let mut rng = StdRng::seed_from_u64(2712);
    let pieces: [&[u8]; 9] = [
        b"text ",
        b"x",
        "é".as_bytes(),
        "😀".as_bytes(),
        b"a\rb",
        b"\xff\xfe",
        b"\x80",
        b"\xe2\x82",
        b"\xf0\x9f\x98",
    ];
    let bytes = generate(MIB + 1000, &pieces, b"\r\n", false, &mut rng);
    let dir = tempfile::tempdir().unwrap();
    let source = complete(&open(&dir, "crlf.txt", &bytes));

    // Rows by a naive scan: split at `\n`, and drop each `\r` before one.
    let mut rows = Vec::new();
    let mut start = 0;
    for (ix, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            rows.push(start..ix - 1);
            start = ix + 1;
        }
    }
    rows.push(start..bytes.len());
    assert_eq!(source.len(), bytes.len());
    assert_eq!(source.row_count() as usize, rows.len());

    let mut texts = Vec::new();
    for (row, range) in rows.iter().enumerate() {
        let row = row as u32;
        assert_eq!(source.row_start(row), range.start, "row {row}");
        assert_eq!(source.row_len(row) as usize, range.len(), "row {row}");
        let text = source.row_to_string(row);
        let mut expected = String::new();
        for chunk in bytes[range.clone()].utf8_chunks() {
            expected.push_str(chunk.valid());
            expected.extend(std::iter::repeat_n('?', chunk.invalid().len()));
        }
        assert_eq!(text, expected, "row {row}");
        texts.push(text);
    }
    // No piece holds a `?`, so each one read is an invalid byte.
    let invalid: usize = bytes.utf8_chunks().map(|chunk| chunk.invalid().len()).sum();
    let marks: usize = texts.iter().map(|text| text.matches('?').count()).sum();
    assert!(invalid > 1000);
    assert_eq!(marks, invalid);

    for _ in 0..1000 {
        let row = rng.random_range(0..rows.len());
        let range = &rows[row];
        let a = rng.random_range(range.clone().start..=range.end);
        let b = rng.random_range(a..=range.end);
        assert_eq!(
            source.slice_to_string(a..b).len(),
            b - a,
            "row {row}, {a}..{b}"
        );

        // From inside one row to inside a later one: each break reads as `\n`.
        if row + 1 == rows.len() {
            continue;
        }
        let last = (row + rng.random_range(1..4)).min(rows.len() - 1);
        let to = rows[last].start + texts[last].len() / 2;
        let to = source.clip_offset(to, Bias::Left);
        let from = source.clip_offset(a, Bias::Left);
        let mut expected = texts[row][from - range.start..].to_string();
        for text in &texts[row + 1..last] {
            expected.push('\n');
            expected.push_str(text);
        }
        expected.push('\n');
        expected.push_str(&texts[last][..to - rows[last].start]);
        assert_eq!(source.slice_to_string(from..to), expected, "{from}..{to}");
    }

    // Ranges ending inside a char that straddles a chunk edge keep their
    // length too: one row of `x`, a 😀 across the edge, invalid bytes and a
    // CRLF, and a € across the next edge.
    let edge = CHUNK_BYTES as usize;
    let mut bytes = vec![b'x'; edge - 2];
    bytes.extend_from_slice("😀".as_bytes());
    bytes.extend_from_slice(b"\xff\xe2\x82\r\n");
    bytes.resize(2 * edge - 1, b'y');
    bytes.extend_from_slice("€\r\nend".as_bytes());
    let source = complete(&open(&dir, "edges.txt", &bytes));
    assert_eq!(source.row_len(0) as usize, edge + 5);
    for edge in [edge, 2 * edge] {
        for a in edge - 6..edge + 3 {
            for b in a..edge + 4 {
                let row = source.offset_to_point(a).row;
                let row_end = source.row_start(row) + source.row_len(row) as usize;
                if b <= row_end {
                    assert_eq!(source.slice_to_string(a..b).len(), b - a, "{a}..{b}");
                    assert_eq!(joined(&source, a..b).len(), b - a, "{a}..{b}");
                }
            }
        }
    }
    let first = source.row_to_string(0);
    assert!(
        first.ends_with("xx😀???"),
        "{:?}",
        &first[first.len() - 12..]
    );
    assert_eq!(source.slice_to_string(edge - 4..edge + 8), "xx😀???\ny");
}

#[test]
fn unreadable_context_keeps_readable_text() {
    let dir = tempfile::tempdir().unwrap();
    let chunk = CHUNK_BYTES as usize;
    let path = dir.path().join("two.txt");
    fs::write(&path, b"x\n".repeat(chunk)).unwrap();
    let view = Arc::new(FileView::open(&path).unwrap());
    let text = complete(&view);
    OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(CHUNK_BYTES)
        .unwrap();

    // The first chunk is still there; only the context after it is gone.
    assert_eq!(text.char_at(chunk - 1), Some('\n'));
    assert_eq!(text.char_at(chunk - 2), Some('x'));
    assert!(text.is_char_boundary(chunk - 1));
    assert_eq!(text.slice_to_string(0..chunk), "x\n".repeat(chunk / 2));
    assert_eq!(text.error(), Some(ReadErrorKind::Changed));
    assert_eq!(text.slice_to_string(chunk..chunk + 10), "");
}

#[test]
fn oversized_and_reversed_ranges_are_clamped() {
    let dir = tempfile::tempdir().unwrap();
    for bytes in [&b""[..], b"one\r\ntwo"] {
        let text = complete(&open(&dir, "range.txt", bytes));
        let all = if bytes.is_empty() { "" } else { "one\ntwo" };
        assert_eq!(text.slice_to_string(0..usize::MAX), all);
        assert_eq!(joined(&text, 0..usize::MAX), all);
        #[allow(clippy::reversed_empty_ranges)]
        let reversed = 5..2;
        assert_eq!(text.slice_to_string(reversed.clone()), "");
        assert_eq!(joined(&text, reversed), "");
        assert_eq!(text.slice_to_string(usize::MAX - 1..usize::MAX), "");
        assert_eq!(text.char_at(usize::MAX), None);
        assert!(!text.is_char_boundary(usize::MAX));
        assert_eq!(text.clip_offset(usize::MAX, Bias::Right), text.len());
        assert_eq!(text.error(), None);
    }
}

#[test]
fn snapshots_while_building() {
    let mut rng = StdRng::seed_from_u64(2713);
    let mut bytes = generate(MIB / 2, &VALID, b"\n", true, &mut rng);
    // A row longer than a chunk, so one step finds no `\n`.
    bytes.extend(std::iter::repeat_n(b'y', CHUNK_BYTES as usize + 100));
    bytes.extend(generate(MIB / 4, &VALID, b"\n", false, &mut rng));
    let contents = String::from_utf8(bytes).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let view = open(&dir, "growing.txt", contents.as_bytes());
    let (index, steps) = LineIndex::build_stepped(Arc::clone(&view));
    let index = Arc::new(index);

    let mut snapshots = vec![FileText::new(Arc::clone(&view), Arc::clone(&index))];
    assert_eq!((snapshots[0].len(), snapshots[0].row_count()), (0, 1));
    for progress in steps {
        let snapshot = FileText::new(Arc::clone(&view), Arc::clone(&index));
        let scanned = &contents.as_bytes()[..progress.scanned as usize];
        let end = if progress.state == IndexState::Complete {
            contents.len()
        } else {
            scanned
                .iter()
                .rposition(|&b| b == b'\n')
                .map_or(0, |ix| ix + 1)
        };
        let prefix = Text::new(&contents[..end]);
        assert_eq!(snapshot.slice_to_string(0..end), contents[..end]);
        assert_rows_match(&snapshot, &prefix);
        snapshots.push(snapshot);
    }
    assert_eq!(index.progress().state, IndexState::Complete);

    // Earlier snapshots still read as they did, and each later one extends
    // the one before.
    for pair in snapshots.windows(2) {
        let (earlier, later) = (&pair[0], &pair[1]);
        let last = earlier.row_count() - 1;
        assert!(later.len() >= earlier.len());
        assert!(later.row_count() > last);
        assert_eq!(earlier.row_len(last), 0);
        assert_eq!(earlier.row_start(last), earlier.len());
        assert_eq!(later.row_start(last), earlier.len());
        let text = earlier.slice_to_string(0..earlier.len());
        assert_eq!(later.slice_to_string(0..earlier.len()), text);
        assert_eq!(text, contents[..earlier.len()]);
        assert_eq!(earlier.offset_to_point(earlier.len()), Point::new(last, 0));
    }
    assert!(snapshots.iter().all(|snapshot| snapshot.error().is_none()));
}

#[test]
fn changed_file_never_panics() {
    let mut rng = StdRng::seed_from_u64(2714);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("log.txt");
    let pieces: [&[u8]; 4] = [b"a log line ", "é😀".as_bytes(), b"\r", b"\xff"];
    fs::write(&path, generate(4 * MIB, &pieces, b"\n", true, &mut rng)).unwrap();
    let view = Arc::new(FileView::open(&path).unwrap());
    let text = complete(&view);
    let len = text.len();
    let rows = text.row_count();
    // Warm a little of the cache, so some reads see stale bytes. Nothing
    // else is cached before the truncation, so reads past it fail.
    text.slice_to_string(0..1000);
    assert_eq!(text.error(), None);

    let file = OpenOptions::new().write(true).open(&path).unwrap();
    for i in 0..1000 {
        if i == 0 {
            file.set_len(MIB as u64).unwrap();
            text.slice_to_string(2 * MIB..2 * MIB + 10);
            assert_eq!(text.error(), Some(ReadErrorKind::Changed));
        }
        if i == 500 {
            let rewritten = generate(6 * MIB, &pieces, b"\r\n", false, &mut rng);
            (&file).write_all(&rewritten).unwrap();
        }
        let offset = rng.random_range(0..=len + 100);
        // Ranges up to two chunks long, some of them backwards.
        let span = 2 * CHUNK_BYTES as usize;
        let other = rng.random_range(offset.saturating_sub(1000)..=offset + span);
        let row = rng.random_range(0..=rows + 2);
        let bias = if rng.random_bool(0.5) {
            Bias::Left
        } else {
            Bias::Right
        };
        match rng.random_range(0..10) {
            0 => _ = text.row_start(row),
            1 => _ = text.row_len(row),
            2 => _ = text.row_to_string(row),
            3 => _ = text.offset_to_point(offset),
            4 => _ = text.point_to_offset(Point::new(row, rng.random_range(0..500))),
            5 => _ = text.clip_offset(offset, bias),
            6 => _ = text.is_char_boundary(offset),
            7 => _ = text.char_at(offset),
            8 => _ = text.slice_to_string(offset..other),
            _ => _ = joined(&text, offset..offset + rng.random_range(0..span)),
        }
        assert_eq!((text.len(), text.row_count()), (len, rows));
    }
    assert_eq!(text.error(), Some(ReadErrorKind::Changed));
}

#[test]
fn a_file_text_is_send_and_sync() {
    fn shared<T: Send + Sync + Clone>() {}
    shared::<FileText>();
    let dir = tempfile::tempdir().unwrap();
    let text = complete(&open(&dir, "short.txt", b"one\r\ntwo\n\xffthree"));
    let handle = text.share();
    let (rows, last) = thread::spawn(move || (handle.row_count(), handle.row_to_string(2)))
        .join()
        .unwrap();
    assert_eq!((rows, last.as_str()), (3, "?three"));
    assert_eq!(text.slice_to_string(0..text.len()), "one\ntwo\n?three");
    assert_eq!(text.char_at(3), Some('\n'));
    assert_eq!(text.char_at(4), Some('\n'));
    assert!(!text.is_char_boundary(4));
    assert_eq!(text.clip_offset(4, Bias::Left), 3);
    assert_eq!(text.clip_offset(4, Bias::Right), 5);
}
