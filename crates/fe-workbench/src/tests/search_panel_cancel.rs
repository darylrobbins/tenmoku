//! Cancelling a search run while its batch's offsets are being decoded: a
//! rerun or a dropped panel sets the run's flag, which decoding checks
//! between files, between lines and every `CHECK_BYTES` of a line it reads
//! or decodes; once it's set nothing more is decoded, and the batch isn't
//! sent.

use std::{
    cell::Cell,
    fs,
    ops::Range,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

use fe_buffer::Buffer;
use fe_project::{FileMatches, LineMatch};

use crate::search_panel::{CHECK_BYTES, Cancelled, decode_batch, publish};

/// How many `x`s come before the long line's match.
const LONG: usize = 4 * CHECK_BYTES;

/// A run's cancel flag, set at the `at`th check as a rerun or a dropped
/// panel would set it then, and the checks made.
struct Checks {
    at: usize,
    calls: Cell<usize>,
    flag: AtomicBool,
}

impl Checks {
    fn new(at: usize) -> Self {
        Self {
            at,
            calls: Cell::new(0),
            flag: AtomicBool::new(false),
        }
    }

    fn cancelled(&self) -> bool {
        let n = self.calls.get() + 1;
        self.calls.set(n);
        if n == self.at {
            self.flag.store(true, Ordering::Relaxed);
        }
        self.flag.load(Ordering::Relaxed)
    }
}

/// A line whose match `range` (in bytes on disk) is previewed as `preview`
/// from `preview_start`.
fn line(number: u64, offset: u64, range: Range<usize>, preview: &str, start: usize) -> LineMatch {
    LineMatch {
        line_number: number,
        line_offset: offset,
        ranges: vec![range],
        preview: preview.into(),
        preview_start: start,
    }
}

/// Plain lines that start each file, enough to put its invalid bytes past
/// the 64 KiB the loader detects the encoding from, so it reads the files as
/// UTF-8, lossily, as the batch's offsets assume.
fn padding() -> (Vec<u8>, u64, u64) {
    let lines = 64 * 1024 / 4 + 1;
    (b"pad\n".repeat(lines as usize), lines, 4 * lines)
}

/// A batch of three files, each line starting with an invalid byte that
/// decodes to a 3-byte U+FFFD, so decoding moves every match 2 bytes on: two
/// short lines, one with `LONG` bytes before its match, and one more short
/// one; and the batch decoded. Each file's lines come after the padding.
fn batch(root: &Path) -> (Vec<FileMatches>, Vec<FileMatches>) {
    let (pad, n, off) = padding();
    let mut long = b"\xFF".to_vec();
    long.extend(std::iter::repeat_n(b'x', LONG));
    long.extend(b"ab\n");
    let files = [
        (
            "a.txt",
            b"\xFFab\n\xFEcd\n".to_vec(),
            vec![
                line(n + 1, off, 1..3, "\u{FFFD}ab", 0),
                line(n + 2, off + 4, 1..3, "\u{FFFD}cd", 0),
            ],
            vec![
                line(n + 1, off, 3..5, "\u{FFFD}ab", 0),
                line(n + 2, off + 4, 3..5, "\u{FFFD}cd", 0),
            ],
        ),
        (
            "long.txt",
            long,
            vec![line(n + 1, off, 1 + LONG..3 + LONG, "ab", 1 + LONG)],
            vec![line(n + 1, off, 3 + LONG..5 + LONG, "ab", 3 + LONG)],
        ),
        (
            "b.txt",
            b"\xFFef\n".to_vec(),
            vec![line(n + 1, off, 1..3, "\u{FFFD}ef", 0)],
            vec![line(n + 1, off, 3..5, "\u{FFFD}ef", 0)],
        ),
    ];
    let mut searched = Vec::new();
    let mut decoded = Vec::new();
    for (name, tail, lines, decoded_lines) in files {
        let path = root.join(name);
        let bytes = [pad.as_slice(), &tail].concat();
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            Buffer::from_reader_detecting(bytes.as_slice())
                .unwrap()
                .encoding()
                .label(),
            "UTF-8"
        );
        let file = |lines| FileMatches {
            path: path.clone(),
            rel: name.into(),
            lines,
        };
        searched.push(file(lines));
        decoded.push(file(decoded_lines));
    }
    (searched, decoded)
}

fn lines(batch: &[FileMatches]) -> Vec<LineMatch> {
    batch.iter().flat_map(|file| file.lines.clone()).collect()
}

/// Decodes `batch` with the flag set at check `at`: whether it finished,
/// the batch as it was left, and the checks made.
fn decode_with(
    batch: &[FileMatches],
    at: usize,
) -> (Result<(), Cancelled>, Vec<FileMatches>, usize) {
    let checks = Checks::new(at);
    let mut batch = batch.to_vec();
    let result = decode_batch(&mut batch, &|| checks.cancelled());
    (result, batch, checks.calls.get())
}

/// Whether `batch` is sent with the flag set at check `at`, and as what.
fn published(batch: &[FileMatches], at: usize) -> Option<Vec<FileMatches>> {
    let checks = Checks::new(at);
    let mut sent = None;
    publish(batch.to_vec(), &|| checks.cancelled(), |batch| {
        sent = Some(batch)
    });
    sent
}

#[test]
fn cancelling_during_decoding_stops_the_batch() {
    let dir = tempfile::tempdir().unwrap();
    let (searched, decoded) = batch(dir.path());

    // Never cancelled, every line is decoded and the batch is sent.
    let (result, all, total) = decode_with(&searched, usize::MAX);
    assert_eq!(result, Ok(()));
    assert_eq!(all, decoded);
    assert_eq!(published(&searched, usize::MAX), Some(decoded.clone()));

    // The long line is read and decoded a `CHECK_BYTES` piece at a time, each
    // checked first: after its file's and its line's checks, at least
    // `LONG / CHECK_BYTES` each for reading and decoding.
    let (_, _, before_long) = decode_with(&searched[..1], usize::MAX);
    let (_, _, long_checks) = decode_with(&searched[1..2], usize::MAX);
    assert!(
        long_checks >= 2 + 2 * (LONG / CHECK_BYTES),
        "{long_checks} checks"
    );

    let searched_lines = lines(&searched);
    let decoded_lines = lines(&decoded);
    let mut done_before = 0;
    for at in 1..=total {
        let (result, batch, calls) = decode_with(&searched, at);
        assert_eq!(result, Err(Cancelled), "the flag set at check {at}");
        assert_eq!(calls, at, "nothing is checked, nor done, past check {at}");
        // The lines decoded before the flag was set lead; the rest, the one
        // it was set in among them, are as the search gave them.
        let batch = lines(&batch);
        let done = batch
            .iter()
            .zip(&decoded_lines)
            .take_while(|(line, decoded)| line == decoded)
            .count();
        assert_eq!(
            batch[done..],
            searched_lines[done..],
            "the flag set at check {at}"
        );
        assert!(done >= done_before);
        done_before = done;
        // Set inside the long line, reading or decoding it, the flag stops
        // it there: only the first file's two lines are decoded.
        if (before_long + 1..=before_long + long_checks).contains(&at) {
            assert_eq!(done, 2, "the flag set at check {at}, in the long line");
        }
        assert_eq!(published(&searched, at), None, "the flag set at check {at}");
    }
    // Set after the last file is decoded, just before it'd be sent.
    assert_eq!(published(&searched, total + 1), None);
}
