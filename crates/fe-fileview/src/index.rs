//! A sparse line index built on a background thread.
//!
//! [`LineIndex::build`] scans the file once, from offset 0 to the view's
//! recorded length, and keeps the start offset of every [`STRIDE`]th row. A
//! query finds the checkpoint at or before what it wants and walks forward
//! over at most `STRIDE - 1` line breaks through the view's cached reads. The
//! scan reads [`CHUNK_BYTES`] at a time into one buffer of its own, bypassing
//! the chunk cache, so indexing a 4 GB file never evicts the chunks the screen
//! is reading. Its memory is that buffer plus the checkpoints: 8 bytes per 64
//! rows.
//!
//! Rows follow `fe_text::Text`: the row count is the number of `\n` plus one,
//! and `\r\n` is a two-byte line break, so a row starts just after its `\n`
//! either way. A lone `\r` is not a line break here. `Text` normalises it to
//! `\n` on load, so a file with lone `\r`s has more rows in a rope than in
//! this index.
//!
//! Queries never wait for the scan: they answer from what it has found so far
//! and return `None` for what it hasn't reached. Callers poll
//! [`LineIndex::progress`]; there is no callback.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use crate::{CHUNK_BYTES, FileView, ReadError};

/// Every `STRIDE`th row's start offset is kept.
pub const STRIDE: u32 = 64;

/// Rows are `u32`, so at most `u32::MAX` rows: one more than the line breaks.
const MAX_BREAKS: u32 = u32::MAX - 1;

/// Where the scan is. Everything but `Building` is final.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum IndexState {
    /// The scan is still running.
    Building,
    /// The whole recorded length was scanned.
    Complete,
    /// A read came back short of the recorded length, or hit end of file
    /// inside it: the file changed on disk since the view was opened.
    Changed,
    /// The file has more than `u32::MAX` rows. Counting stopped at the line
    /// break that would have made one more.
    TooManyRows,
    /// Any other I/O error.
    Failed,
}

/// What [`LineIndex::progress`] reports.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// The line breaks found so far. Once `Complete`, the file has `rows + 1`
    /// rows.
    pub rows: u32,
    /// The bytes scanned so far, from offset 0.
    pub scanned: u64,
    pub state: IndexState,
}

/// A sparse index of a [`FileView`]'s row starts, built on a background thread
/// named `"line index"`. `Send + Sync`.
///
/// Dropping it cancels the scan: the thread checks between chunks, so it exits
/// after at most one more chunk. The drop doesn't wait for that.
#[derive(Debug)]
pub struct LineIndex {
    view: Arc<FileView>,
    shared: Arc<Shared>,
}

/// Scans one chunk of a [`LineIndex::build_stepped`] build per call to
/// [`Iterator::next`], which returns the progress after that chunk, or `None`
/// once the scan has ended. Dropping it lets the rest of the scan run freely.
#[doc(hidden)]
#[derive(Debug)]
pub struct Step {
    shared: Arc<Shared>,
}

#[derive(Debug)]
struct Shared {
    cancel: AtomicBool,
    scan: Mutex<Scan>,
    /// Only for a stepped build.
    gate: Option<Gate>,
}

/// What the scan has found, published after each chunk.
#[derive(Debug)]
struct Scan {
    rows: u32,
    scanned: u64,
    state: IndexState,
    /// The offset of the last `\n` counted.
    last_newline: Option<u64>,
    /// `checkpoints[k]` is the start of row `k * STRIDE`.
    checkpoints: Vec<u64>,
}

#[derive(Debug, Default)]
struct Gate {
    turns: Mutex<Turns>,
    changed: Condvar,
}

#[derive(Debug, Default)]
struct Turns {
    /// Chunks the stepper has allowed.
    granted: u64,
    /// Chunks the thread has finished.
    done: u64,
    /// The stepper is gone, so every chunk is allowed.
    open: bool,
    /// The thread has returned.
    exited: bool,
}

impl LineIndex {
    /// Starts indexing `view` on a new thread and returns at once.
    pub fn build(view: Arc<FileView>) -> LineIndex {
        LineIndex::start(view, None)
    }

    /// A build that scans one chunk each time the returned [`Step`] is
    /// advanced, so tests can see an index that is still building. For tests
    /// only.
    #[doc(hidden)]
    pub fn build_stepped(view: Arc<FileView>) -> (LineIndex, Step) {
        let index = LineIndex::start(view, Some(Gate::default()));
        let step = Step {
            shared: Arc::clone(&index.shared),
        };
        (index, step)
    }

    fn start(view: Arc<FileView>, gate: Option<Gate>) -> LineIndex {
        let empty = view.is_empty();
        let shared = Arc::new(Shared {
            cancel: AtomicBool::new(false),
            scan: Mutex::new(Scan {
                rows: 0,
                scanned: 0,
                state: if empty {
                    IndexState::Complete
                } else {
                    IndexState::Building
                },
                last_newline: None,
                checkpoints: vec![0],
            }),
            gate,
        });
        let spawned = if empty {
            Ok(())
        } else {
            let (view, shared) = (Arc::clone(&view), Arc::clone(&shared));
            thread::Builder::new()
                .name("line index".into())
                .spawn(move || run(&view, &shared))
                .map(drop)
        };
        if spawned.is_err() {
            // No thread: nothing will scan, and a stepper must not wait.
            shared.lock().state = IndexState::Failed;
            shared.exit();
        }
        LineIndex { view, shared }
    }

    /// How far the scan has got.
    pub fn progress(&self) -> Progress {
        self.shared.lock().progress()
    }

    /// The start offset of `row`, or `None` if the scan hasn't reached it:
    /// `row` is past [`Progress::rows`]. Row 0 starts at 0, and every other
    /// row just after a `\n`. Once `Complete`, the last row is
    /// `Progress::rows`.
    ///
    /// Walks over at most `STRIDE - 1` line breaks from the nearest
    /// checkpoint, through the view's cached reads, so it reads every byte of
    /// up to 63 rows.
    pub fn row_start(&self, row: u32) -> Result<Option<u64>, ReadError> {
        let (from, limit) = {
            let scan = self.shared.lock();
            if row > scan.rows {
                return Ok(None);
            }
            let limit = scan.last_newline.map_or(0, |newline| newline + 1);
            if row == scan.rows {
                return Ok(Some(limit));
            }
            (scan.checkpoints[(row / STRIDE) as usize], limit)
        };
        self.walk_breaks(from, row % STRIDE, limit).map(Some)
    }

    /// The row holding byte `offset`, clamped to the recorded length: the
    /// number of `\n` before it. A `\n` (and the `\r` of a `\r\n`) belongs
    /// to the row it ends.
    ///
    /// Until the scan is `Complete`, an offset after the last `\n` found is
    /// `None`, and so is every offset while none has been found.
    ///
    /// Binary-searches the checkpoints, then counts the line breaks between
    /// the nearest one and `offset` through the view's cached reads, so it
    /// reads every byte in between, however long those rows are.
    pub fn row_for_offset(&self, offset: u64) -> Result<Option<u32>, ReadError> {
        let offset = offset.min(self.view.len());
        let (k, from) = {
            let scan = self.shared.lock();
            let after_last = scan.last_newline.is_none_or(|newline| offset > newline);
            if after_last {
                return Ok((scan.state == IndexState::Complete).then_some(scan.rows));
            }
            let k = scan.checkpoints.partition_point(|&start| start <= offset) - 1;
            (k, scan.checkpoints[k])
        };
        let breaks = self.count_breaks(from..offset)?;
        if breaks >= STRIDE {
            // The cached bytes no longer match what the scan saw.
            return Err(ReadError::Changed);
        }
        Ok(Some(k as u32 * STRIDE + breaks))
    }

    /// How many checkpoints the index holds.
    #[doc(hidden)]
    pub fn checkpoints(&self) -> usize {
        self.shared.lock().checkpoints.len()
    }

    /// The offset just after the `breaks`th `\n` from `from`, which the scan
    /// found before `limit`.
    fn walk_breaks(&self, mut from: u64, mut breaks: u32, limit: u64) -> Result<u64, ReadError> {
        while breaks > 0 {
            if from >= limit {
                return Err(ReadError::Changed);
            }
            let index = from / CHUNK_BYTES;
            let chunk = self.view.chunk(index)?;
            let start = (from - index * CHUNK_BYTES) as usize;
            let end = (limit - index * CHUNK_BYTES).min(chunk.len() as u64) as usize;
            if end <= start {
                // A cached chunk shorter than the scan's: the file changed.
                return Err(ReadError::Changed);
            }
            match nth_newline(&chunk[start..end], breaks) {
                Ok(newline) => {
                    from = index * CHUNK_BYTES + (start + newline) as u64 + 1;
                    breaks = 0;
                }
                Err(found) => {
                    from = index * CHUNK_BYTES + end as u64;
                    breaks -= found;
                }
            }
        }
        Ok(from)
    }

    /// The `\n` in `range`, stopping early at `STRIDE`.
    fn count_breaks(&self, range: std::ops::Range<u64>) -> Result<u32, ReadError> {
        let mut breaks = 0;
        let mut at = range.start;
        while at < range.end && breaks < STRIDE {
            let index = at / CHUNK_BYTES;
            let chunk = self.view.chunk(index)?;
            let start = (at - index * CHUNK_BYTES) as usize;
            let end = (range.end - index * CHUNK_BYTES).min(chunk.len() as u64) as usize;
            if end <= start {
                return Err(ReadError::Changed);
            }
            breaks += count_newlines(&chunk[start..end]);
            at += (end - start) as u64;
        }
        Ok(breaks)
    }
}

impl Drop for LineIndex {
    fn drop(&mut self) {
        self.shared.cancel.store(true, Ordering::Relaxed);
        if let Some(gate) = &self.shared.gate {
            // Taking the lock orders the store before a waiting thread's
            // next look at `cancel`.
            let _turns = gate.lock();
            gate.changed.notify_all();
        }
    }
}

impl Step {
    /// Waits up to `timeout` for the thread to return, and says whether it
    /// has.
    pub fn exited_within(&self, timeout: Duration) -> bool {
        let Some(gate) = &self.shared.gate else {
            return true;
        };
        let deadline = Instant::now() + timeout;
        let mut turns = gate.lock();
        while !turns.exited {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            turns = gate
                .changed
                .wait_timeout(turns, left)
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
        true
    }
}

impl Iterator for Step {
    type Item = Progress;

    /// Lets the thread scan one chunk and waits for it.
    fn next(&mut self) -> Option<Progress> {
        let gate = self.shared.gate.as_ref()?;
        if self.shared.lock().state != IndexState::Building {
            return None;
        }
        let mut turns = gate.lock();
        turns.granted += 1;
        gate.changed.notify_all();
        while turns.done < turns.granted {
            if turns.exited {
                // Cancelled before scanning this chunk.
                return None;
            }
            turns = gate
                .changed
                .wait(turns)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        drop(turns);
        Some(self.shared.lock().progress())
    }
}

impl Drop for Step {
    fn drop(&mut self) {
        if let Some(gate) = &self.shared.gate {
            gate.lock().open = true;
            gate.changed.notify_all();
        }
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Scan> {
        // Nothing panics while holding the lock.
        self.scan
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Waits for a stepper's turn, if there is one. False once cancelled.
    fn wait_turn(&self) -> bool {
        let Some(gate) = &self.gate else {
            return !self.cancelled();
        };
        let mut turns = gate.lock();
        while !turns.open && turns.done >= turns.granted && !self.cancelled() {
            turns = gate
                .changed
                .wait(turns)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        !self.cancelled()
    }

    fn chunk_done(&self) {
        if let Some(gate) = &self.gate {
            gate.lock().done += 1;
            gate.changed.notify_all();
        }
    }

    fn exit(&self) {
        if let Some(gate) = &self.gate {
            gate.lock().exited = true;
            gate.changed.notify_all();
        }
    }
}

impl Scan {
    fn progress(&self) -> Progress {
        Progress {
            rows: self.rows,
            scanned: self.scanned,
            state: self.state,
        }
    }
}

impl Gate {
    fn lock(&self) -> MutexGuard<'_, Turns> {
        self.turns
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Marks the thread as returned however it returns.
struct Exit<'a>(&'a Shared);

impl Drop for Exit<'_> {
    fn drop(&mut self) {
        self.0.exit();
    }
}

/// The scan thread.
fn run(view: &FileView, shared: &Shared) {
    let _exit = Exit(shared);
    let len = view.len();
    let mut buf = vec![0; CHUNK_BYTES.min(len) as usize];
    let mut counter = Counter::default();
    let mut checkpoints = Vec::new();
    let mut at = 0;
    while at < len {
        if !shared.wait_turn() {
            return;
        }
        let want = CHUNK_BYTES.min(len - at) as usize;
        let state = match view.read_uncached(&mut buf[..want], at) {
            Ok(()) => {
                let counted = counter.count(at, &buf[..want], &mut checkpoints);
                at += want as u64;
                if !counted {
                    IndexState::TooManyRows
                } else if at == len {
                    IndexState::Complete
                } else {
                    IndexState::Building
                }
            }
            Err(ReadError::Changed) => IndexState::Changed,
            Err(ReadError::Io(_)) => IndexState::Failed,
        };
        {
            let mut scan = shared.lock();
            scan.rows = counter.rows;
            scan.scanned = at;
            scan.last_newline = counter.last_newline;
            scan.checkpoints.append(&mut checkpoints);
            scan.state = state;
        }
        shared.chunk_done();
        if state != IndexState::Building {
            return;
        }
    }
}

/// Counts line breaks 64 bytes at a time, through a bitmask of where the `\n`
/// are.
#[derive(Debug, Default)]
struct Counter {
    rows: u32,
    last_newline: Option<u64>,
}

impl Counter {
    /// Counts the `\n` in `bytes`, which start at `base`, and pushes the start
    /// of every `STRIDE`th row. False if counting stopped at [`MAX_BREAKS`].
    fn count(&mut self, base: u64, bytes: &[u8], checkpoints: &mut Vec<u64>) -> bool {
        let (blocks, rest) = bytes.as_chunks::<64>();
        let mut at = base;
        for block in blocks {
            if !self.add(at, newlines(block), checkpoints) {
                return false;
            }
            at += 64;
        }
        let mask = rest
            .iter()
            .enumerate()
            .fold(0, |mask, (ix, &b)| mask | (u64::from(b == b'\n') << ix));
        self.add(at, mask, checkpoints)
    }

    /// Adds the `\n` at bits of `mask`, counted from `at`.
    fn add(&mut self, at: u64, mut mask: u64, checkpoints: &mut Vec<u64>) -> bool {
        if mask == 0 {
            return true;
        }
        let mut found = mask.count_ones();
        let room = MAX_BREAKS - self.rows;
        let full = found > room;
        if full {
            if room == 0 {
                return false;
            }
            // Keep only the first `room` breaks.
            let last = nth_bit(mask, room);
            mask &= !0 >> (63 - last);
            found = room;
        }
        // The break that makes the count a multiple of STRIDE, 1-based. A
        // block holds at most 64, so at most one checkpoint.
        let next = STRIDE - self.rows % STRIDE;
        if next <= found {
            checkpoints.push(at + u64::from(nth_bit(mask, next)) + 1);
        }
        self.rows += found;
        self.last_newline = Some(at + 63 - u64::from(mask.leading_zeros()));
        !full
    }
}

/// The `\n` in `block`, as one bit per byte.
fn newlines(block: &[u8; 64]) -> u64 {
    let mut mask = 0;
    for (ix, word) in block.as_chunks::<8>().0.iter().enumerate() {
        // Gathers each byte's high bit into the top byte, in byte order.
        let bits = ((newline_bytes(word) >> 7).wrapping_mul(0x0102_0408_1020_4080)) >> 56;
        mask |= bits << (ix * 8);
    }
    mask
}

/// `0x80` in each byte of the 8-byte `word` that is `\n`, and 0 in the
/// others. The sum can't carry from one byte into the next, so unlike the
/// usual has-a-zero-byte test this one is exact.
fn newline_bytes(word: &[u8; 8]) -> u64 {
    const LOW: u64 = 0x0101_0101_0101_0101;
    const HIGH: u64 = 0x8080_8080_8080_8080;
    let word = u64::from_le_bytes(*word);
    let x = word ^ (LOW * u64::from(b'\n'));
    !(((x & !HIGH) + !HIGH) | x) & HIGH
}

/// The index of the `n`th `\n` in `bytes`, 1-based, or how many there are
/// if fewer.
fn nth_newline(bytes: &[u8], mut n: u32) -> Result<usize, u32> {
    let wanted = n;
    let (words, rest) = bytes.as_chunks::<8>();
    let mut at = 0;
    for word in words {
        let mask = newline_bytes(word);
        let found = mask.count_ones();
        if found >= n {
            return Ok(at + nth_bit(mask, n) as usize / 8);
        }
        n -= found;
        at += 8;
    }
    for (ix, &b) in rest.iter().enumerate() {
        if b == b'\n' {
            n -= 1;
            if n == 0 {
                return Ok(at + ix);
            }
        }
    }
    Err(wanted - n)
}

fn count_newlines(bytes: &[u8]) -> u32 {
    let (words, rest) = bytes.as_chunks::<8>();
    let count: u32 = words
        .iter()
        .map(|word| newline_bytes(word).count_ones())
        .sum();
    count + rest.iter().filter(|&&b| b == b'\n').count() as u32
}

/// The position of the `n`th set bit of `mask`, 1-based. `mask` has at least
/// `n` set bits.
fn nth_bit(mut mask: u64, n: u32) -> u32 {
    for _ in 1..n {
        mask &= mask - 1;
    }
    mask.trailing_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counting_stops_at_the_last_row_a_u32_can_number() {
        let mut checkpoints = Vec::new();
        let mut counter = Counter {
            rows: MAX_BREAKS - 2,
            last_newline: None,
        };
        // Breaks at bytes 1, 3, 5 and 7: only two more fit.
        assert!(!counter.count(100, b"a\nb\nc\nd\n", &mut checkpoints));
        assert_eq!(counter.rows, MAX_BREAKS);
        assert_eq!(counter.last_newline, Some(103));

        let mut counter = Counter {
            rows: MAX_BREAKS,
            last_newline: Some(7),
        };
        assert!(counter.count(0, b"no breaks", &mut checkpoints));
        assert!(!counter.count(0, b"\n", &mut checkpoints));
        assert_eq!((counter.rows, counter.last_newline), (MAX_BREAKS, Some(7)));
    }

    #[test]
    fn newline_searches_match_a_byte_loop() {
        let bytes = b"\n\x0b\x8a\nab\n\n\n\xff\x0a\x09cdefghijklmnopqrstuvwxyz\n\r\n\x8a\n";
        let positions: Vec<usize> = (0..bytes.len()).filter(|&ix| bytes[ix] == b'\n').collect();
        for start in 0..bytes.len() {
            let rest = &bytes[start..];
            let ahead: Vec<usize> = positions
                .iter()
                .filter(|&&ix| ix >= start)
                .map(|ix| ix - start)
                .collect();
            assert_eq!(count_newlines(rest), ahead.len() as u32);
            for n in 1..=ahead.len() as u32 + 1 {
                let expected = ahead.get(n as usize - 1).copied().ok_or(ahead.len() as u32);
                assert_eq!(nth_newline(rest, n), expected, "from {start}, n {n}");
            }
        }
        let mut block = [b'x'; 64];
        for &ix in &[0, 7, 8, 30, 63] {
            block[ix] = b'\n';
        }
        block[9] = 0x8a;
        assert_eq!(newlines(&block), 1 | 1 << 7 | 1 << 8 | 1 << 30 | 1 << 63);
    }

    #[test]
    fn nth_bit_counts_from_the_lowest() {
        assert_eq!(nth_bit(0b1011_0100, 1), 2);
        assert_eq!(nth_bit(0b1011_0100, 3), 5);
        assert_eq!(nth_bit(1 << 63, 1), 63);
    }
}
