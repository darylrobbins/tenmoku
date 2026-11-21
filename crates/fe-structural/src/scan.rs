//! Scanning a project for a pattern: the files of the pattern's language
//! that the project walk shows, prefiltered by the pattern's literals,
//! parsed and searched on worker threads, their matches streamed to a sink
//! in batches.

use std::{
    collections::HashMap,
    io::Read,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender, TryRecvError},
    },
    time::{Duration, Instant},
};

use fe_project::{PathFilter, WalkControl, WalkedFile, walk_files};

use crate::{
    matcher::{Match, MatchOptions, Stopped},
    pattern::Pattern,
    prefilter::Prefilter,
    rewrite::{Edit, EditsError, Rewrite, RewriteError},
};

/// The most bytes of a match's first line kept in [`MatchRow::first_line`].
pub const FIRST_LINE_BYTES: usize = 512;

/// The most files in one batch handed to the sink.
const BATCH_FILES: usize = 64;

/// How a [`scan`] runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanOptions {
    /// Worker threads walking, parsing and searching. `None`: the machine's
    /// available parallelism up to 8, or up to 4 for a [`wasm`](Self::wasm)
    /// language. `Some(1)` scans on one thread in path order.
    pub threads: Option<usize>,
    /// The pattern's language is a WebAssembly grammar (one
    /// [`fe_syntax::register_language`] loaded), whose parsers each hold a
    /// store: fewer threads by default. The caller knows; fe-syntax doesn't
    /// say.
    pub wasm: bool,
    /// Files larger than this are skipped unread and counted in
    /// [`ScanSummary::too_large`]. Default 2 MiB.
    pub max_file_bytes: u64,
    /// The most matches reported; once reached the scan stops and sets
    /// [`ScanSummary::truncated`] if any were left out. Default 10 000.
    pub max_matches: usize,
    /// How long one file's parse and search (and rewrite check) may take;
    /// past it the file is dropped and counted in [`ScanSummary::timed_out`].
    /// Default 2 s.
    pub file_deadline: Duration,
    /// How matches are found in each file.
    pub match_options: MatchOptions,
}

impl Default for ScanOptions {
    fn default() -> Self {
        ScanOptions {
            threads: None,
            wasm: false,
            max_file_bytes: 2 << 20,
            max_matches: 10_000,
            file_deadline: Duration::from_secs(2),
            match_options: MatchOptions::default(),
        }
    }
}

impl ScanOptions {
    /// The threads the scan runs on: [`threads`](Self::threads), or the
    /// default for the language, at least one.
    pub fn thread_count(&self) -> usize {
        let cap = if self.wasm { 4 } else { 8 };
        self.threads
            .unwrap_or_else(|| {
                std::thread::available_parallelism()
                    .map_or(1, |n| n.get())
                    .min(cap)
            })
            .max(1)
    }
}

/// A file's matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileResult {
    /// The path as the walk reports it: under the root as given.
    pub path: PathBuf,
    /// `path` relative to the root, empty when the root is the file.
    pub rel: PathBuf,
    /// In file order (outer matches before the ones inside them).
    pub matches: Vec<MatchRow>,
}

/// One match in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MatchRow {
    /// The matched node's bytes in the file.
    pub range: Range<usize>,
    /// The line the match starts on, zero-based.
    pub start_line: usize,
    /// The byte in that line it starts at, zero-based.
    pub start_col: usize,
    /// The match's text up to the end of its first line (without the line
    /// break, or a `\r` before it), cut to at most [`FIRST_LINE_BYTES`] on a
    /// char boundary.
    pub first_line: String,
    /// The lines the match spans, at least one. A line break ending the
    /// match doesn't start another.
    pub line_count: usize,
    /// Each named metavariable and the text it covered, in the pattern's
    /// order.
    pub captures: Vec<(String, String)>,
    /// With a rewrite: what replaces the match. `None` without one, and for
    /// a match inside another, which isn't rewritten (see
    /// [`Pattern::edits`]).
    pub rewritten: Option<String>,
    /// The rewrite check found that this replacement alone adds syntax
    /// errors to the file ([`Edit::flagged`]).
    pub flagged: bool,
}

/// What a [`scan`] came to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanSummary {
    /// Regular files the walk reached (ignored and filtered-out ones aren't).
    pub files_walked: usize,
    /// Of those, the files of the pattern's language, by path.
    pub files_of_language: usize,
    /// Whether the pattern had literals to prefilter by. A pattern of only
    /// metavariables has none, and every file of the language is parsed.
    pub prefiltered: bool,
    /// Files of the language skipped unparsed for lacking a literal.
    pub files_prefiltered_out: usize,
    /// Files handed to the parser.
    pub files_parsed: usize,
    /// Files the parser gave up on (a WebAssembly grammar past its own
    /// deadline, or no parser for the language).
    pub parse_failed: usize,
    /// Files past [`ScanOptions::file_deadline`], dropped.
    pub timed_out: usize,
    /// Files over [`ScanOptions::max_file_bytes`], skipped unread.
    pub too_large: usize,
    /// Files that aren't UTF-8 text, skipped: invalid UTF-8, or holding a
    /// NUL byte (UTF-16 with or without a byte order mark, binary files).
    pub non_utf8: usize,
    /// Matches reported to the sink.
    pub matches: usize,
    /// The cancel flag was set when the scan ended.
    pub cancelled: bool,
    /// Matches were left out at [`ScanOptions::max_matches`].
    pub truncated: bool,
    /// Files that couldn't be opened or read, with the reason.
    pub errors: Vec<(PathBuf, String)>,
    /// The rewrite wasn't compiled for this pattern and doesn't fit it:
    /// nothing was scanned.
    pub rewrite_error: Option<RewriteError>,
}

/// Searches the files under `root` for `pattern`: those the project walk
/// shows (gitignore rules, hidden files) and `filter` lets through, of the
/// pattern's language by path. Each file under the size cap is read,
/// checked to be UTF-8 and to hold every literal of the pattern
/// ([`crate::prefilter_literals`]), then parsed and searched with
/// [`ScanOptions::file_deadline`] for both. With `rewrite`, each match's
/// replacement is worked out too, as [`Pattern::edits`] does.
///
/// Files with matches are handed to `sink` on the calling thread, in
/// batches as the workers finish them, each file whole in one batch: in
/// path order on one thread, in no set order on several. Once `cancel` is
/// set the scan stops: no further batch is handed over, and files in flight
/// are dropped. It stops too once [`ScanOptions::max_matches`] are
/// reported.
pub fn scan(
    root: &Path,
    pattern: &Pattern,
    rewrite: Option<&Rewrite>,
    options: &ScanOptions,
    filter: &PathFilter,
    cancel: Arc<AtomicBool>,
    mut sink: impl FnMut(Vec<FileResult>),
) -> ScanSummary {
    if let Some(rewrite) = rewrite
        && let Err(error) = Rewrite::compile(pattern, rewrite.template())
    {
        return ScanSummary {
            rewrite_error: Some(error),
            cancelled: cancel.load(Ordering::Relaxed),
            ..ScanSummary::default()
        };
    }
    let threads = options.thread_count();
    let prefilter = Prefilter::new(pattern);
    let shared = Shared {
        pattern,
        rewrite,
        options,
        prefilter: &prefilter,
        cancel: &cancel,
        done: AtomicBool::new(false),
        counts: Counts::default(),
        reported: AtomicUsize::new(0),
        truncated: AtomicBool::new(false),
        errors: Mutex::new(Vec::new()),
    };
    let (sender, receiver) = mpsc::sync_channel::<FileResult>(threads * 2);
    let (walked, delivered) = std::thread::scope(|scope| {
        let walker = scope.spawn(|| {
            let sender = sender;
            walk_files(root, threads, filter, cancel.clone(), |file| {
                shared.visit(file, &sender)
            })
        });
        let delivered = deliver(receiver, &cancel, &mut sink);
        let walked = walker
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic));
        (walked, delivered)
    });
    let mut errors = walked.errors;
    errors.extend(
        shared
            .errors
            .into_inner()
            .unwrap_or_else(|e| e.into_inner()),
    );
    let counts = &shared.counts;
    ScanSummary {
        files_walked: counts.walked.load(Ordering::Relaxed),
        files_of_language: counts.of_language.load(Ordering::Relaxed),
        prefiltered: !prefilter.is_empty(),
        files_prefiltered_out: counts.prefiltered_out.load(Ordering::Relaxed),
        files_parsed: counts.parsed.load(Ordering::Relaxed),
        parse_failed: counts.parse_failed.load(Ordering::Relaxed),
        timed_out: counts.timed_out.load(Ordering::Relaxed),
        too_large: counts.too_large.load(Ordering::Relaxed),
        non_utf8: counts.non_utf8.load(Ordering::Relaxed),
        matches: delivered,
        cancelled: cancel.load(Ordering::Relaxed),
        truncated: shared.truncated.load(Ordering::Relaxed),
        errors,
        rewrite_error: None,
    }
}

/// Hands the workers' files to `sink` in batches until they're done or
/// `cancel` is set, then drops the receiver so a worker blocked sending
/// gives up: how many matches were handed over.
fn deliver(
    receiver: Receiver<FileResult>,
    cancel: &AtomicBool,
    sink: &mut impl FnMut(Vec<FileResult>),
) -> usize {
    let mut delivered = 0;
    while let Ok(first) = receiver.recv() {
        let mut batch = vec![first];
        let mut closed = false;
        while batch.len() < BATCH_FILES {
            match receiver.try_recv() {
                Ok(file) => batch.push(file),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    closed = true;
                    break;
                }
            }
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        delivered += batch.iter().map(|file| file.matches.len()).sum::<usize>();
        sink(batch);
        if closed || cancel.load(Ordering::Relaxed) {
            break;
        }
    }
    delivered
}

#[derive(Default)]
struct Counts {
    walked: AtomicUsize,
    of_language: AtomicUsize,
    prefiltered_out: AtomicUsize,
    parsed: AtomicUsize,
    parse_failed: AtomicUsize,
    timed_out: AtomicUsize,
    too_large: AtomicUsize,
    non_utf8: AtomicUsize,
}

fn bump(count: &AtomicUsize) {
    count.fetch_add(1, Ordering::Relaxed);
}

/// What the workers share.
struct Shared<'a> {
    pattern: &'a Pattern,
    rewrite: Option<&'a Rewrite>,
    options: &'a ScanOptions,
    prefilter: &'a Prefilter,
    cancel: &'a AtomicBool,
    /// [`ScanOptions::max_matches`] were reported: stop.
    done: AtomicBool,
    counts: Counts,
    /// Matches reserved under the cap by files sent or being sent.
    reported: AtomicUsize,
    truncated: AtomicBool,
    errors: Mutex<Vec<(PathBuf, String)>>,
}

/// Why a file's search ended without results.
enum Ended {
    /// The scan is over (cancelled, or at the match cap).
    Stop,
    /// The file was skipped; the walk goes on.
    Skip,
}

impl Shared<'_> {
    fn stopping(&self) -> bool {
        self.cancel.load(Ordering::Relaxed) || self.done.load(Ordering::Relaxed)
    }

    fn visit(&self, file: WalkedFile, sender: &SyncSender<FileResult>) -> WalkControl {
        if self.stopping() {
            return WalkControl::Stop;
        }
        bump(&self.counts.walked);
        let matches = match self.search(&file) {
            Ok(matches) => matches,
            Err(Ended::Skip) => return WalkControl::Continue,
            Err(Ended::Stop) => return WalkControl::Stop,
        };
        if matches.is_empty() {
            return WalkControl::Continue;
        }
        let WalkedFile { path, rel, .. } = file;
        match sender.send(FileResult { path, rel, matches }) {
            Ok(()) if !self.stopping() => WalkControl::Continue,
            // The receiver is gone (cancelled), or the cap was reached.
            _ => WalkControl::Stop,
        }
    }

    /// The file's match rows: empty for none.
    fn search(&self, walked: &WalkedFile) -> Result<Vec<MatchRow>, Ended> {
        let language = self.pattern.language();
        if fe_syntax::language_name_for_path(&walked.path) != Some(language.name()) {
            return Err(Ended::Skip);
        }
        bump(&self.counts.of_language);
        let Some(bytes) = self.read(walked)? else {
            return Err(Ended::Skip);
        };
        // A NUL byte: UTF-16 (or UTF-32) without a byte order mark, whose
        // ASCII is valid UTF-8 between its zero bytes, or a binary file.
        let source = match std::str::from_utf8(&bytes) {
            Ok(source) if memchr::memchr(0, &bytes).is_none() => source,
            _ => {
                bump(&self.counts.non_utf8);
                return Err(Ended::Skip);
            }
        };
        if !self.prefilter.passes(source.as_bytes()) {
            bump(&self.counts.prefiltered_out);
            return Err(Ended::Skip);
        }
        if self.stopping() {
            return Err(Ended::Stop);
        }
        let deadline = Instant::now() + self.options.file_deadline;
        let timed_out = || {
            bump(&self.counts.timed_out);
            Ended::Skip
        };
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        let mut stop = || self.stopping() || Instant::now() >= deadline;
        bump(&self.counts.parsed);
        let mut stopped = false;
        let tree = parse(language, &walked.path, source, || {
            stopped = stop();
            stopped
        });
        let Some(tree) = tree else {
            if self.stopping() {
                return Err(Ended::Stop);
            }
            if stopped {
                return Err(timed_out());
            }
            bump(&self.counts.parse_failed);
            return Err(Ended::Skip);
        };
        let opts = &self.options.match_options;
        let found = self.pattern.find_all(source, &tree, opts, &mut stop);
        // What's left under the cap; it only shrinks as other files report.
        let budget = self
            .options
            .max_matches
            .saturating_sub(self.reported.load(Ordering::Relaxed));
        let edits = match (&found, self.rewrite) {
            (Ok(found), Some(rewrite)) if !found.is_empty() && budget > 0 => {
                match self.pattern.edits(rewrite, source, &tree, opts, &mut stop) {
                    Ok(edits) => Ok(edits.edits),
                    Err(EditsError::Stopped(stopped)) => Err(stopped),
                    // Checked before the scan started.
                    Err(EditsError::WrongPattern(_)) => Ok(Vec::new()),
                }
            }
            _ => Ok(Vec::new()),
        };
        let (found, edits) = match (found, edits) {
            (Ok(found), Ok(edits)) => (found, edits),
            (Err(Stopped::Cancelled), _) | (_, Err(Stopped::Cancelled)) => {
                return Err(if self.stopping() {
                    Ended::Stop
                } else {
                    timed_out()
                });
            }
            (Err(Stopped::ParseFailed), _) | (_, Err(Stopped::ParseFailed)) => {
                bump(&self.counts.parse_failed);
                return Err(Ended::Skip);
            }
        };
        if found.is_empty() {
            return Ok(Vec::new());
        }
        let take = found.len().min(budget);
        let Some(mut rows) = rows(source, &found[..take], edits, &mut stop) else {
            return Err(if self.stopping() {
                Ended::Stop
            } else {
                timed_out()
            });
        };
        // At most `take`: the budget was read before this reservation.
        let room = self.reserve(found.len());
        if room == 0 {
            return Err(Ended::Stop);
        }
        rows.truncate(room);
        Ok(rows)
    }

    /// The file's bytes, or `None` if it was skipped (too large, unreadable).
    fn read(&self, walked: &WalkedFile) -> Result<Option<Vec<u8>>, Ended> {
        let max = self.options.max_file_bytes;
        let error = |error: std::io::Error| {
            self.errors
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((walked.path.clone(), error.to_string()));
            Ok(None)
        };
        let len = match walked.file.metadata() {
            Ok(metadata) => metadata.len(),
            Err(err) => return error(err),
        };
        if len > max {
            bump(&self.counts.too_large);
            return Ok(None);
        }
        let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
        // One byte past the cap: a file that grew since its size was read.
        if let Err(err) = (&walked.file)
            .take(max.saturating_add(1))
            .read_to_end(&mut bytes)
        {
            return error(err);
        }
        if bytes.len() as u64 > max {
            bump(&self.counts.too_large);
            return Ok(None);
        }
        Ok(Some(bytes))
    }

    /// Reserves up to `wanted` matches under the cap: how many this file
    /// may report. Fewer than `wanted` sets `truncated` and ends the scan.
    fn reserve(&self, wanted: usize) -> usize {
        let max = self.options.max_matches;
        let before = self
            .reported
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |reported| {
                Some(reported.saturating_add(wanted).min(max))
            })
            .unwrap_or_else(|reported| reported);
        let room = max.saturating_sub(before).min(wanted);
        if room < wanted {
            self.truncated.store(true, Ordering::Relaxed);
            self.done.store(true, Ordering::Relaxed);
        }
        room
    }
}

/// How many rows are built between asks of the stop callback.
const ROWS_STOP_EVERY: usize = 256;

/// The rows for `found`, matches in `source`, with `edits` (in file order,
/// from [`Pattern::edits`]) given to the matches they replace. `stop` is
/// asked before the first row and every [`ROWS_STOP_EVERY`]; `None` once it
/// says `true`.
fn rows(
    source: &str,
    found: &[Match],
    edits: Vec<Edit>,
    stop: &mut dyn FnMut() -> bool,
) -> Option<Vec<MatchRow>> {
    let mut edits: HashMap<(usize, usize), Edit> = edits
        .into_iter()
        .map(|edit| ((edit.range.start, edit.range.end), edit))
        .collect();
    let bytes = source.as_bytes();
    // Matches come in tree order, so their starts never go back: the line
    // and its start are carried forward, each byte looked at once.
    let (mut at, mut line, mut line_start) = (0, 0, 0);
    let mut out = Vec::with_capacity(found.len());
    for (ix, matched) in found.iter().enumerate() {
        if ix.is_multiple_of(ROWS_STOP_EVERY) && stop() {
            return None;
        }
        let Range { start, end } = matched.range.clone();
        if start < at {
            (at, line, line_start) = (0, 0, 0);
        }
        let skipped = &bytes[at..start];
        line += memchr::memchr_iter(b'\n', skipped).count();
        if let Some(nl) = memchr::memrchr(b'\n', skipped) {
            line_start = at + nl + 1;
        }
        at = start;
        out.push({
            let text = &source[start..end];
            // Cut first, so a long line is never looked at past the cut.
            let first = truncate(text, FIRST_LINE_BYTES);
            let first = first.split('\n').next().unwrap_or_default();
            let first = first.strip_suffix('\r').unwrap_or(first);
            let spanned = text.strip_suffix('\n').unwrap_or(text);
            let edit = edits.remove(&(start, end));
            MatchRow {
                range: start..end,
                start_line: line,
                start_col: start - line_start,
                first_line: first.to_owned(),
                line_count: 1 + memchr::memchr_iter(b'\n', spanned.as_bytes()).count(),
                captures: matched
                    .captures
                    .iter()
                    .map(|(name, ranges)| (name.to_owned(), capture_text(source, ranges)))
                    .collect(),
                flagged: edit.as_ref().is_some_and(|edit| edit.flagged),
                rewritten: edit.map(|edit| edit.text),
            }
        });
    }
    Some(out)
}

/// [`fe_syntax::parse_str`], or, with the `test-support` feature, a failure
/// for a file [`testing::fail_parses_named`] names.
fn parse(
    language: &fe_syntax::Language,
    path: &Path,
    source: &str,
    out_of_time: impl FnMut() -> bool,
) -> Option<tree_sitter::Tree> {
    #[cfg(feature = "test-support")]
    if testing::parse_fails(path) {
        return None;
    }
    let _ = path;
    fe_syntax::parse_str(language, source, out_of_time)
}

/// Test builds' hooks into the scan.
#[cfg(feature = "test-support")]
pub mod testing {
    use std::{
        ffi::OsString,
        path::Path,
        sync::{Mutex, PoisonError},
    };

    static FAILING: Mutex<Vec<OsString>> = Mutex::new(Vec::new());

    /// From now on, in this process, the parse of every file called `name`
    /// fails as a parser giving up does (a WebAssembly grammar past its
    /// deadline): counted in [`ScanSummary::parse_failed`](super::ScanSummary::parse_failed).
    pub fn fail_parses_named(name: &str) {
        FAILING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(name.into());
    }

    pub(super) fn parse_fails(path: &Path) -> bool {
        let Some(name) = path.file_name() else {
            return false;
        };
        FAILING
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .any(|failing| failing == name)
    }
}

/// What a capture's ranges cover: from the first's start to the last's end.
fn capture_text(source: &str, ranges: &[Range<usize>]) -> String {
    match (ranges.first(), ranges.last()) {
        (Some(first), Some(last)) => source
            .get(first.start..last.end)
            .unwrap_or_default()
            .to_owned(),
        _ => String::new(),
    }
}

/// `text` cut to at most `max` bytes, on a char boundary.
fn truncate(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(pattern: &Pattern, source: &str) -> Vec<Match> {
        pattern.find_in_text(source).unwrap()
    }

    fn rust_pattern(source: &str) -> Pattern {
        Pattern::compile(fe_syntax::language_by_name("rust").unwrap(), source).unwrap()
    }

    #[test]
    fn rows_carry_lines_and_columns_forward() {
        let source = "fn a() { foo(1); foo(2);\n\n  foo(3); }\n";
        let pattern = rust_pattern("foo($A)");
        let rows = rows(source, &found(&pattern, source), Vec::new(), &mut || false).unwrap();
        let places: Vec<_> = rows.iter().map(|r| (r.start_line, r.start_col)).collect();
        assert_eq!(places, [(0, 9), (0, 17), (2, 2)]);
    }

    #[test]
    fn rows_stop_when_asked() {
        let source = "fn a() { foo(1); }\n";
        let pattern = rust_pattern("foo($A)");
        let matches = found(&pattern, source);
        assert!(rows(source, &matches, Vec::new(), &mut || true).is_none());
    }

    #[test]
    fn first_line_is_cut_on_a_char_boundary() {
        assert_eq!(truncate("aé", 2), "a");
        assert_eq!(truncate("aé", 3), "aé");
    }
}
