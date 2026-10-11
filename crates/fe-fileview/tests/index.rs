//! The background line index against a naive scan of the file's bytes.

use std::fs::{self, OpenOptions};
use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use fe_fileview::{CHUNK_BYTES, FileView, IndexState, LineIndex, Progress, STRIDE};
use rand::{Rng, SeedableRng, rngs::StdRng};

const MIB: usize = 1024 * 1024;

#[derive(Copy, Clone, Debug)]
enum Breaks {
    Lf,
    CrLf,
    /// `\n`, `\r\n` and lone `\r`.
    Mixed,
}

/// About `len` bytes of rows of 0 to 40 pieces (so some rows are empty), with
/// 2-, 3- and 4-byte chars and invalid bytes, ending with a line break or
/// not. Rows are drawn from a pool, which keeps a debug build quick.
fn generate(len: usize, breaks: Breaks, final_newline: bool, rng: &mut StdRng) -> Vec<u8> {
    const PIECES: [&[u8]; 8] = [
        b"text ",
        b"x",
        "é".as_bytes(),
        "€".as_bytes(),
        "😀".as_bytes(),
        b"\xff\xfe",
        b"\x80",
        b"\xe2\x82",
    ];
    let pool: Vec<Vec<u8>> = (0..1000)
        .map(|_| {
            (0..rng.random_range(0..40))
                .flat_map(|_| PIECES[rng.random_range(0..PIECES.len())])
                .copied()
                .collect()
        })
        .collect();
    let mut bytes = Vec::with_capacity(len + 256);
    while bytes.len() < len {
        bytes.extend_from_slice(&pool[rng.random_range(0..pool.len())]);
        let newline: &[u8] = match breaks {
            Breaks::Lf => b"\n",
            Breaks::CrLf => b"\r\n",
            Breaks::Mixed => [&b"\n"[..], b"\r\n", b"\r", b"\n\n"][rng.random_range(0..4)],
        };
        bytes.extend_from_slice(newline);
    }
    if !final_newline {
        while matches!(bytes.last(), Some(b'\n' | b'\r')) {
            bytes.pop();
        }
        bytes.extend_from_slice(b"end");
    } else if bytes.last() != Some(&b'\n') {
        bytes.push(b'\n');
    }
    bytes
}

/// Row starts by a naive scan: 0, and just after every `\n`.
fn naive_starts(bytes: &[u8]) -> Vec<u64> {
    let mut starts = vec![0];
    starts.extend(
        bytes
            .iter()
            .enumerate()
            .filter(|&(_, &b)| b == b'\n')
            .map(|(ix, _)| ix as u64 + 1),
    );
    starts
}

/// The row holding `offset`: the `\n` before it.
fn naive_row(starts: &[u64], offset: u64) -> u32 {
    (starts.partition_point(|&start| start <= offset) - 1) as u32
}

fn open(dir: &tempfile::TempDir, name: &str, bytes: &[u8]) -> (PathBuf, Arc<FileView>) {
    let path = dir.path().join(name);
    fs::write(&path, bytes).unwrap();
    let view = Arc::new(FileView::open(&path).unwrap());
    (path, view)
}

fn wait_until_done(index: &LineIndex) -> Progress {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let progress = index.progress();
        if progress.state != IndexState::Building {
            return progress;
        }
        assert!(Instant::now() < deadline, "still building: {progress:?}");
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn rows_match_a_full_scan() {
    let dir = tempfile::tempdir().unwrap();
    // One thread per file: a debug build walks rows slowly.
    thread::scope(|scope| {
        for (seed, breaks) in [Breaks::Lf, Breaks::CrLf, Breaks::Mixed]
            .into_iter()
            .enumerate()
        {
            for final_newline in [true, false] {
                let dir = &dir;
                scope.spawn(move || {
                    let mut rng =
                        StdRng::seed_from_u64(269 + 2 * seed as u64 + final_newline as u64);
                    check_against_a_full_scan(dir, breaks, final_newline, &mut rng);
                });
            }
        }
    });
}

fn check_against_a_full_scan(
    dir: &tempfile::TempDir,
    breaks: Breaks,
    final_newline: bool,
    rng: &mut StdRng,
) {
    let what = format!("{breaks:?}, final newline {final_newline}");
    let bytes = generate(8 * MIB, breaks, final_newline, rng);
    let starts = naive_starts(&bytes);
    let (_, view) = open(dir, &format!("{breaks:?}-{final_newline}.txt"), &bytes);
    let index = LineIndex::build(Arc::clone(&view));

    let progress = wait_until_done(&index);
    assert_eq!(progress.state, IndexState::Complete, "{what}");
    assert_eq!(progress.scanned, bytes.len() as u64, "{what}");
    assert_eq!(progress.rows as usize + 1, starts.len(), "{what}");

    for (row, &start) in starts.iter().enumerate() {
        assert_eq!(
            index.row_start(row as u32).unwrap(),
            Some(start),
            "{what}: row {row}"
        );
    }
    assert_eq!(index.row_start(starts.len() as u32).unwrap(), None);

    let len = bytes.len() as u64;
    for i in 0..1000 {
        let offset = match i {
            0 => 0,
            1 => len,
            2 => len + 10,
            _ => rng.random_range(0..len),
        };
        assert_eq!(
            index.row_for_offset(offset).unwrap(),
            Some(naive_row(&starts, offset.min(len))),
            "{what}: offset {offset}"
        );
    }
}

#[test]
fn answers_while_building() {
    let mut rng = StdRng::seed_from_u64(2690);
    let dir = tempfile::tempdir().unwrap();
    let bytes = generate(8 * MIB, Breaks::Mixed, true, &mut rng);
    let starts = naive_starts(&bytes);
    let (_, view) = open(&dir, "stepped.txt", &bytes);
    let chunks = bytes.len().div_ceil(CHUNK_BYTES as usize);

    let (index, mut step) = LineIndex::build_stepped(view);
    let mut last = index.progress();
    assert_eq!((last.rows, last.scanned), (0, 0));
    let mut steps = 0;
    loop {
        let progress = index.progress();
        assert!(progress.rows >= last.rows, "{progress:?} after {last:?}");
        assert_eq!(progress.state, IndexState::Building, "before step {steps}");
        let found_before = last.rows;
        last = progress;

        let rows = progress.rows;
        // Every row the last step found (so each row once over the build),
        // from a stride back so a checkpoint is crossed, and a sample of the
        // older rows.
        let near = found_before.saturating_sub(STRIDE + 2)..=rows;
        let sample = (0..50).map(|_| rng.random_range(0..=rows));
        for row in near.chain(sample) {
            let found = index.row_start(row).unwrap();
            assert_eq!(found, Some(starts[row as usize]), "step {steps}: row {row}");
        }
        for row in rows + 1..rows + 4 {
            assert_eq!(
                index.row_start(row).unwrap(),
                None,
                "step {steps}: row {row}"
            );
        }
        for _ in 0..50 {
            let offset = rng.random_range(0..bytes.len() as u64);
            if let Some(row) = index.row_for_offset(offset).unwrap() {
                assert_eq!(row, naive_row(&starts, offset), "step {steps}: {offset}");
                assert!(row <= rows);
            }
        }

        let Some(after) = step.next() else { break };
        steps += 1;
        let expected = if steps == chunks {
            IndexState::Complete
        } else {
            IndexState::Building
        };
        assert_eq!(after.state, expected, "after step {steps} of {chunks}");
        if after.state == IndexState::Complete {
            break;
        }
    }
    assert_eq!(steps, chunks);
    assert!(step.next().is_none());
    let done = index.progress();
    assert_eq!(done.state, IndexState::Complete);
    assert_eq!(done.rows as usize + 1, starts.len());
}

#[test]
fn stops_on_truncation_and_drop() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = "a log line é😀\n".repeat(16 * MIB / 18).into_bytes();
    let (path, view) = open(&dir, "truncated.log", &bytes);
    let starts = naive_starts(&bytes);

    let (index, mut step) = LineIndex::build_stepped(Arc::clone(&view));
    for _ in 0..4 {
        assert_eq!(step.next().unwrap().state, IndexState::Building);
    }
    let file = OpenOptions::new().write(true).open(&path).unwrap();
    file.set_len(8 * MIB as u64 + 100).unwrap();
    while step.next().is_some() {}
    let progress = index.progress();
    assert_eq!(progress.state, IndexState::Changed, "{progress:?}");
    assert!(progress.scanned <= 8 * MIB as u64 + 100, "{progress:?}");
    // What was found before the cut still answers, or reports the change.
    for row in [0, 1, STRIDE + 1, progress.rows - 1, progress.rows] {
        match index.row_start(row) {
            Ok(Some(start)) => assert_eq!(start, starts[row as usize]),
            Ok(None) => panic!("row {row} was found"),
            Err(_) => {}
        }
    }
    let _ = index.row_for_offset(progress.scanned);
    drop(index);

    // Dropped mid-build, the thread stops without another step.
    fs::write(&path, &bytes).unwrap();
    let view = Arc::new(FileView::open(&path).unwrap());
    let (index, mut step) = LineIndex::build_stepped(view);
    step.next().unwrap();
    step.next().unwrap();
    assert_eq!(index.progress().state, IndexState::Building);
    drop(index);
    assert!(
        step.exited_within(Duration::from_secs(1)),
        "the thread is still running"
    );
    assert!(step.next().is_none());

    // A free-running build dropped at once doesn't hang or panic either.
    let view = Arc::new(FileView::open(&path).unwrap());
    drop(LineIndex::build(view));
}

#[test]
fn checkpoints_are_sparse() {
    let lines = 1_000_000;
    let mut bytes = Vec::new();
    for line in 0..lines {
        bytes.extend_from_slice(format!("{line}\n").as_bytes());
    }
    let dir = tempfile::tempdir().unwrap();
    let (_, view) = open(&dir, "short-lines.txt", &bytes);
    let cached = view.cached_bytes();

    let index = LineIndex::build(Arc::clone(&view));
    let progress = wait_until_done(&index);
    assert_eq!(progress.state, IndexState::Complete);
    assert_eq!(progress.rows, lines);
    assert!(
        index.checkpoints() <= (lines / STRIDE + 1) as usize,
        "{} checkpoints",
        index.checkpoints()
    );
    assert_eq!(
        view.cached_bytes(),
        cached,
        "the scan went through the cache"
    );
}

#[test]
fn an_index_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LineIndex>();
}

#[test]
fn an_empty_file_and_one_without_breaks() {
    let dir = tempfile::tempdir().unwrap();
    let (_, view) = open(&dir, "empty.txt", b"");
    let index = LineIndex::build(view);
    assert_eq!(
        index.progress(),
        Progress {
            rows: 0,
            scanned: 0,
            state: IndexState::Complete
        }
    );
    assert_eq!(index.row_start(0).unwrap(), Some(0));
    assert_eq!(index.row_start(1).unwrap(), None);
    assert_eq!(index.row_for_offset(5).unwrap(), Some(0));

    let (_, view) = open(&dir, "one-row.txt", &vec![b'x'; 3 * CHUNK_BYTES as usize]);
    let (index, mut step) = LineIndex::build_stepped(view);
    assert_eq!(index.row_for_offset(0).unwrap(), None, "no break found yet");
    while step.next().is_some() {}
    assert_eq!(index.progress().rows, 0);
    assert_eq!(index.row_for_offset(CHUNK_BYTES).unwrap(), Some(0));
    assert_eq!(index.row_start(0).unwrap(), Some(0));
}

#[test]
fn a_stepped_empty_file_has_no_worker() {
    let dir = tempfile::tempdir().unwrap();
    let (_, view) = open(&dir, "empty-stepped.txt", b"");
    let (index, mut step) = LineIndex::build_stepped(view);
    assert_eq!(index.progress().state, IndexState::Complete);
    assert!(step.next().is_none());
    assert!(step.exited_within(Duration::from_millis(100)));
    drop(index);
    assert!(step.exited_within(Duration::from_millis(100)));
}
