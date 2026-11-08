//! Building the index in the background: walking the project and
//! extracting each file's definitions on worker threads.
//!
//! The walk runs on a thread of its own and queues paths for extraction,
//! which runs on the workers, or on the calling thread with one thread.
//! The workers queue what they extract for the calling thread, which
//! batches it for the sink. Both queues are bounded: a slow sink holds up
//! extraction, and slow extraction holds up the walk, so what's waiting is
//! at most [`PATH_QUEUE`] paths and [`DONE_QUEUE`] files, besides the batch
//! being filled and one file per worker. The walk's own threads, in
//! `fe_project`, hold up too, while the walk thread is blocked queueing:
//! they list at most a few of [`FinderIndex::walk`]'s batches ahead.
//!
//! When the receiving end of a queue goes, because extraction stopped for
//! a cancel or a panic, whatever was blocked sending to it gives up, so a
//! build always ends, and the walk's threads stop with it.

use std::{
    num::NonZero,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering, fence},
        mpsc::{self, Receiver, RecvTimeoutError, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

use fe_project::FinderIndex;

use crate::{
    FileSymbols, Skipped,
    extract::{extract_in, symbols_language},
};

/// The most files in one batch handed to the sink.
pub const MAX_BATCH_FILES: usize = 256;

/// The most paths waiting for extraction.
const PATH_QUEUE: usize = 256;

/// The most extracted files waiting to be batched.
const DONE_QUEUE: usize = 256;

/// A batch is handed on once its first file has waited this long, so
/// queries see files soon after they're indexed.
const FLUSH_AFTER: Duration = Duration::from_millis(100);

/// How a [`build`] runs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildOptions {
    /// Threads extracting definitions. 0 means half the available cores, at
    /// least 1, so a build never takes every core from the UI. 1 extracts
    /// on the calling thread, in the walk's batch order.
    pub threads: usize,
}

impl BuildOptions {
    fn worker_threads(&self) -> usize {
        match self.threads {
            0 => (thread::available_parallelism().map_or(1, NonZero::get) / 2).max(1),
            threads => threads,
        }
    }
}

/// How many files were skipped, by [`Skipped`] reason.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SkipCounts {
    pub unsupported: usize,
    pub too_large: usize,
    pub binary: usize,
    pub unreadable: usize,
    pub parse_failed: usize,
}

impl SkipCounts {
    pub fn total(&self) -> usize {
        self.unsupported + self.too_large + self.binary + self.unreadable + self.parse_failed
    }

    fn add(&mut self, skipped: Skipped) {
        *match skipped {
            Skipped::Unsupported => &mut self.unsupported,
            Skipped::TooLarge => &mut self.too_large,
            Skipped::Binary => &mut self.binary,
            Skipped::Unreadable(_) => &mut self.unreadable,
            Skipped::ParseFailed => &mut self.parse_failed,
        } += 1;
    }
}

/// What a [`build`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildSummary {
    /// Files the walk listed.
    pub files_seen: usize,
    /// Files handed to the sink, and their symbols.
    pub files_indexed: usize,
    /// Files the walk listed and the sink didn't get. A file whose language
    /// has no definitions is skipped before it's opened. Unless the build
    /// was cancelled, `files_seen == files_indexed + skipped.total()`.
    pub skipped: SkipCounts,
    pub symbols: usize,
    pub cancelled: bool,
    pub elapsed: Duration,
}

/// Walks `root` as the project tree shows it, with
/// [`FinderIndex::walk`], and hands every file's definitions to `sink`.
///
/// Runs on the calling thread, which should be a background thread, with
/// threads scoped to the call: one walks, and `options.threads` workers
/// extract the paths as the walk lists them, so walking and parsing
/// overlap. Only paths whose language has definitions, judged by file
/// name, are opened.
///
/// `sink` is called on the calling thread only, with batches of at most
/// [`MAX_BATCH_FILES`] files, so the caller can [`crate::SymbolIndex::extend`]
/// an index others are already querying. A batch is handed on once it's
/// full, or once its first file has waited 100 ms. `cancel` is checked
/// before each file; once it's set, `sink` isn't called again, even when
/// it was set from inside `sink`.
pub fn build(
    root: impl AsRef<Path>,
    options: &BuildOptions,
    cancel: Arc<AtomicBool>,
    sink: impl FnMut(Vec<FileSymbols>),
) -> BuildSummary {
    build_with(
        root.as_ref(),
        options,
        &cancel,
        &Counters::default(),
        sink,
        &extract_in,
    )
}

/// What the walk thread counts.
#[derive(Debug, Default)]
struct Counters {
    /// Paths listed.
    seen: AtomicUsize,
    /// Paths listed whose language has no definitions.
    unsupported: AtomicUsize,
    /// Paths queued for extraction.
    queued: AtomicUsize,
}

/// [`build`], extracting with `extract` and counting into `counters`.
fn build_with<E>(
    root: &Path,
    options: &BuildOptions,
    cancel: &Arc<AtomicBool>,
    counters: &Counters,
    sink: impl FnMut(Vec<FileSymbols>),
    extract: &E,
) -> BuildSummary
where
    E: Fn(&Path, &Path) -> Result<FileSymbols, Skipped> + Sync,
{
    let started = Instant::now();
    let mut out = Out {
        summary: BuildSummary::default(),
        batch: Vec::new(),
        first: None,
        cancel,
        sink,
    };
    // Resolved once, as the walk resolves it, so paths are read from the
    // directory that was walked.
    if let Ok(root) = dunce::canonicalize(root)
        && !cancelled(cancel)
    {
        let root = root.as_path();
        let (paths_tx, paths_rx) = mpsc::sync_channel(PATH_QUEUE);
        thread::scope(|scope| {
            scope.spawn(move || walk(root, cancel, counters, paths_tx));
            match options.worker_threads() {
                1 => extract_here(root, paths_rx, &mut out, extract),
                threads => extract_on_workers(root, threads, paths_rx, &mut out, extract),
            }
        });
        out.flush();
    }
    let mut summary = out.summary;
    summary.files_seen = counters.seen.load(Ordering::Relaxed);
    summary.skipped.unsupported += counters.unsupported.load(Ordering::Relaxed);
    summary.cancelled = cancelled(cancel);
    summary.elapsed = started.elapsed();
    summary
}

/// Lists the project's paths and queues those whose language has
/// definitions, until the walk ends, it's cancelled or extraction stops.
fn walk(root: &Path, cancel: &Arc<AtomicBool>, counters: &Counters, paths: SyncSender<PathBuf>) {
    FinderIndex::walk(root, cancel.clone(), |batch| {
        for rel in batch {
            if cancelled(cancel) {
                return;
            }
            counters.seen.fetch_add(1, Ordering::Relaxed);
            if symbols_language(&rel).is_none() {
                counters.unsupported.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            // Blocks while the queue is full; fails once extraction has
            // stopped and dropped the receiver. Short of a cancel, only a
            // panic stops it: panicking here too stops the walk's threads
            // at once, rather than after listing the rest of the project,
            // and the scope passes extraction's panic on.
            if paths.send(rel).is_err() {
                // Sees the cancel a worker saw before it dropped the
                // receiver, which the failed send may have read relaxed.
                fence(Ordering::Acquire);
                assert!(cancelled(cancel), "extraction stopped");
                return;
            }
            counters.queued.fetch_add(1, Ordering::Relaxed);
        }
    });
}

/// Extracts on the calling thread, in the walk's order.
fn extract_here<E>(
    root: &Path,
    paths: Receiver<PathBuf>,
    out: &mut Out<'_, impl FnMut(Vec<FileSymbols>)>,
    extract: &E,
) where
    E: Fn(&Path, &Path) -> Result<FileSymbols, Skipped>,
{
    loop {
        if cancelled(out.cancel) {
            return;
        }
        match paths.recv_timeout(out.wait()) {
            Ok(rel) => {
                if cancelled(out.cancel) {
                    return;
                }
                out.record(extract(root, &rel));
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        out.flush_if_due();
    }
}

/// Extracts on `threads` workers while the calling thread batches their
/// results.
fn extract_on_workers<E>(
    root: &Path,
    threads: usize,
    paths: Receiver<PathBuf>,
    out: &mut Out<'_, impl FnMut(Vec<FileSymbols>)>,
    extract: &E,
) where
    E: Fn(&Path, &Path) -> Result<FileSymbols, Skipped> + Sync,
{
    // Dropped with the last worker, which stops the walk.
    let paths = Arc::new(Mutex::new(paths));
    let (done_tx, done_rx) = mpsc::sync_channel(DONE_QUEUE);
    let cancel = out.cancel;
    thread::scope(|scope| {
        // Owned here, so a panicking sink drops it, which stops the workers
        // before the scope waits for them.
        let done_rx = done_rx;
        for _ in 0..threads {
            let paths = paths.clone();
            let done_tx = done_tx.clone();
            scope.spawn(move || {
                // A poisoned lock means another worker panicked; the scope
                // passes that panic on.
                while let Some(rel) = paths.lock().ok().and_then(|paths| paths.recv().ok()) {
                    if cancelled(cancel) || done_tx.send(extract(root, &rel)).is_err() {
                        return;
                    }
                }
            });
        }
        drop(paths);
        drop(done_tx);
        loop {
            match done_rx.recv_timeout(out.wait()) {
                Ok(done) => out.record(done),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            out.flush_if_due();
        }
    });
}

/// The build's results so far, and the batch the sink gets next.
struct Out<'a, F> {
    summary: BuildSummary,
    batch: Vec<FileSymbols>,
    /// When the batch's first file came in.
    first: Option<Instant>,
    cancel: &'a AtomicBool,
    sink: F,
}

impl<F: FnMut(Vec<FileSymbols>)> Out<'_, F> {
    fn record(&mut self, extracted: Result<FileSymbols, Skipped>) {
        match extracted {
            Ok(file) => {
                self.first.get_or_insert_with(Instant::now);
                self.batch.push(file);
                if self.batch.len() == MAX_BATCH_FILES {
                    self.flush();
                }
            }
            Err(skipped) => self.summary.skipped.add(skipped),
        }
    }

    /// How long to wait for the next file before the batch is due. With
    /// nothing batched, a while, to check the cancel flag now and then.
    fn wait(&self) -> Duration {
        self.first.map_or(FLUSH_AFTER, |first| {
            FLUSH_AFTER.saturating_sub(first.elapsed())
        })
    }

    fn flush_if_due(&mut self) {
        if self
            .first
            .is_some_and(|first| first.elapsed() >= FLUSH_AFTER)
        {
            self.flush();
        }
    }

    /// Hands the batch to the sink, unless the build is cancelled.
    fn flush(&mut self) {
        self.first = None;
        if self.batch.is_empty() || cancelled(self.cancel) {
            return;
        }
        let batch = std::mem::take(&mut self.batch);
        self.summary.files_indexed += batch.len();
        self.summary.symbols += batch.iter().map(|file| file.symbols.len()).sum::<usize>();
        (self.sink)(batch);
    }
}

/// What [`extract_all`] read, for [`crate::SymbolIndex::update`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Extracted {
    pub files: Vec<FileSymbols>,
    /// The files that couldn't be extracted, and why.
    pub failed: Vec<(PathBuf, Skipped)>,
}

/// Extracts the files at `paths`, relative to `root`, as [`crate::extract`]
/// does, for a [`crate::Reindex`]: on `threads` threads scoped to the call
/// (0 means half the available cores, at least 1; 1 extracts on the
/// calling thread), in `paths`' order. Run it without holding the index.
///
/// `root` is [`crate::SymbolIndex::canonical_root`], the root the index's
/// file set was resolved to once, and isn't resolved again here: each file
/// is opened from it a component at a time without following a symlink,
/// so a root replaced by a symlink since (to a directory outside the
/// project) fails as [`Skipped::Unreadable`] rather than being followed.
/// A root reached through a symlink fails the same way, so don't pass
/// [`crate::SymbolIndex::root`].
///
/// Every result is held until the end, so for a long list, such as a
/// rescan's, extract and [`crate::SymbolIndex::update`] it in chunks (of
/// [`MAX_BATCH_FILES`], say), which also keeps each update short.
///
/// `cancel` is checked before each file; once it's set, the files not yet
/// started are left out of both lists, so nothing tells their symbols are
/// stale: extract the whole `Reindex` again later.
pub fn extract_all(
    root: impl AsRef<Path>,
    paths: &[PathBuf],
    threads: usize,
    cancel: &AtomicBool,
) -> Extracted {
    let mut out = Extracted::default();
    if paths.is_empty() || cancelled(cancel) {
        return out;
    }
    let root = root.as_ref();
    let next = AtomicUsize::new(0);
    // Takes the next path until none are left or the cancel is set.
    let work = || {
        let mut done = Vec::new();
        while !cancelled(cancel) {
            let ix = next.fetch_add(1, Ordering::Relaxed);
            let Some(rel) = paths.get(ix) else {
                break;
            };
            done.push((ix, extract_in(root, rel)));
        }
        done
    };
    let threads = (BuildOptions { threads }.worker_threads()).min(paths.len());
    let mut done = if threads == 1 {
        work()
    } else {
        thread::scope(|scope| {
            let workers: Vec<_> = (0..threads).map(|_| scope.spawn(work)).collect();
            (workers.into_iter())
                .flat_map(|worker| {
                    worker
                        .join()
                        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
                })
                .collect()
        })
    };
    done.sort_unstable_by_key(|&(ix, _)| ix);
    for (ix, result) in done {
        match result {
            Ok(file) => out.files.push(file),
            Err(skipped) => out.failed.push((paths[ix].clone(), skipped)),
        }
    }
    out
}

fn cancelled(cancel: &AtomicBool) -> bool {
    cancel.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    //! Backpressure and batch timing, with stand-in extraction.

    use super::*;

    /// `count` empty Rust files.
    fn project(count: usize) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..count {
            std::fs::write(dir.path().join(format!("f{n:04}.rs")), "").unwrap();
        }
        dir
    }

    fn empty(rel: &Path) -> Result<FileSymbols, Skipped> {
        Ok(FileSymbols {
            rel: rel.to_path_buf(),
            language: "rust",
            len: 0,
            modified: None,
            symbols: Vec::new(),
            truncated: false,
        })
    }

    fn no_cancel() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn a_slow_sink_holds_up_extraction() {
        let dir = project(1_500);
        let started = AtomicUsize::new(0);
        let extract = |_: &Path, rel: &Path| {
            started.fetch_add(1, Ordering::Relaxed);
            empty(rel)
        };
        let threads = 2;
        let mut delivered = 0;
        let summary = build_with(
            dir.path(),
            &BuildOptions { threads },
            &no_cancel(),
            &Counters::default(),
            |batch| {
                delivered += batch.len();
                if delivered == batch.len() {
                    // Unbounded, every file would be extracted meanwhile.
                    thread::sleep(Duration::from_millis(300));
                    let started = started.load(Ordering::Relaxed);
                    let bound = delivered + MAX_BATCH_FILES + DONE_QUEUE + threads;
                    assert!(started <= bound, "{started} extracted, at most {bound}");
                }
            },
            &extract,
        );
        assert_eq!(delivered, 1_500);
        assert_eq!(summary.files_indexed, 1_500);
    }

    #[test]
    fn slow_extraction_holds_up_the_walk() {
        let dir = project(1_500);
        for threads in [1, 2] {
            let counters = Counters::default();
            let started = AtomicUsize::new(0);
            let extract = |_: &Path, rel: &Path| {
                // Each worker's first file is slow. Unbounded, the walk would
                // queue every path meanwhile.
                if started.fetch_add(1, Ordering::Relaxed) < threads {
                    thread::sleep(Duration::from_millis(300));
                }
                let queued = counters.queued.load(Ordering::Relaxed);
                let bound = started.load(Ordering::Relaxed) + PATH_QUEUE + threads;
                assert!(queued <= bound, "{queued} queued, at most {bound}");
                empty(rel)
            };
            let summary = build_with(
                dir.path(),
                &BuildOptions { threads },
                &no_cancel(),
                &counters,
                |_| {},
                &extract,
            );
            assert_eq!(summary.files_indexed, 1_500, "{threads} threads");
        }
    }

    #[test]
    fn partial_batches_are_handed_on_in_time() {
        let dir = project(16);
        for threads in [1, 2] {
            // 25 ms a file of steady arrivals, about 400 ms in all: several
            // flush intervals, though far fewer files than a full batch.
            let extract = |_: &Path, rel: &Path| {
                thread::sleep(Duration::from_millis(25 * threads as u64));
                empty(rel)
            };
            let mut sizes = Vec::new();
            build_with(
                dir.path(),
                &BuildOptions { threads },
                &no_cancel(),
                &Counters::default(),
                |batch| sizes.push(batch.len()),
                &extract,
            );
            assert_eq!(sizes.iter().sum::<usize>(), 16, "{threads} threads");
            assert!(sizes.len() >= 3, "{threads} threads: {sizes:?}");
        }
    }

    #[test]
    fn a_panicking_sink_ends_the_build() {
        let dir = project(600);
        for threads in [1, 2] {
            let result = std::panic::catch_unwind(|| {
                build_with(
                    dir.path(),
                    &BuildOptions { threads },
                    &no_cancel(),
                    &Counters::default(),
                    |_| panic!("sink"),
                    &|_: &Path, rel: &Path| empty(rel),
                )
            });
            assert!(result.is_err(), "{threads} threads");
        }
    }
}
