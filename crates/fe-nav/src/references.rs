//! Find references by word search: every whole-word, case-sensitive use of
//! the word at the caret, found by [`fe_project::search`], with the hits in
//! comments and strings dropped by a parse of their file.
//!
//! This finds uses of a *name*, not of a symbol: a same-named local in
//! another scope is reported too.

use std::{
    cell::RefCell,
    collections::HashSet,
    fs::{self, File},
    io::{self, Read},
    ops::Range,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use fe_buffer::Buffer;
use fe_project::{FileMatches, SearchOptions, SearchQuery, is_word_char, search};
use fe_syntax::{
    HighlightSpan, Language, Syntax, language_by_name, language_name_for_path, symbols,
};

use crate::{
    FileReferences, Location, NavError, NavRequest, Reference, ReferencesSummary,
    search::{in_comment_or_string, word_at},
};

/// The most hits a request finds, the request's own text's included, as
/// project search caps its matches. Hits dropped as comments and strings
/// count towards it.
const MAX_HITS: usize = 10_000;

/// The largest file loaded only to turn its hits into offsets in the text
/// the editor would load, without parsing it. A larger one's hits keep the
/// search's offsets.
const MAX_LOAD_BYTES: usize = 32 << 20;

/// The longest preview of a line of the request's own text, as
/// [`fe_project::LineMatch::preview`] is cut.
const PREVIEW_BYTES: usize = 512;

/// How often the request's cancel flag is passed on to the search while it
/// runs.
const POLL: Duration = Duration::from_millis(5);

/// How much of the request's text is scanned for its word between checks
/// for a cancel.
const SCAN_CHUNK: usize = 64 * 1024;

/// How long one file's parse may take; past it, its hits are unverified.
/// Some inputs never finish parsing (the TSX ones in
/// `fuzz/regressions/syntax_edits`), so a cancel alone isn't enough. It's
/// [`fe_syntax::ParseJob::run`]'s cap for a parse from scratch.
const PARSE_CAP: Duration = if cfg!(test) {
    Duration::from_millis(300)
} else {
    Duration::from_secs(5)
};

/// The query budget of [`Syntax::highlights`] (fe-syntax's `QUERY_BUDGET`):
/// a call that took less can't have run out of it, so its highlights are
/// complete.
const QUERY_BUDGET: Duration = Duration::from_millis(2);

/// How many times a hit's highlights are asked for before it's left
/// unverified, each over a different range, since a query that ran out of
/// time is cached. A thread descheduled during a call makes it look slow,
/// so on a loaded machine one try isn't enough.
const HIGHLIGHT_TRIES: usize = 8;

/// How [`crate::SearchNavigation`] finds references.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReferenceOptions {
    /// The most files parsed per request to verify their hits, besides the
    /// request's own. Files past it report their hits unverified.
    pub verify_files: usize,
}

impl Default for ReferenceOptions {
    fn default() -> ReferenceOptions {
        ReferenceOptions {
            verify_files: 1_000,
        }
    }
}

/// [`crate::NavigationProvider::references`] by word search, for the
/// project at `root`.
///
/// The request's own file comes first, from its text, as one batch. The
/// rest arrive as the search finds them, each batch of matched files
/// verified on this thread before it's handed on: loaded as the editor
/// loads it, then parsed if its language is supported, it's under the
/// language's parse limit, and `options.verify_files` isn't spent. The disk
/// copy of the request's file is skipped.
pub(crate) fn find(
    root: &Path,
    options: &ReferenceOptions,
    req: &NavRequest,
    cancel: &AtomicBool,
    sink: &mut dyn FnMut(Vec<FileReferences>),
) -> Result<ReferencesSummary, NavError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(NavError::Cancelled);
    }
    let Some((_, word)) = word_at(&req.text, req.offset, req.language) else {
        return Ok(ReferencesSummary::default());
    };
    let mut finder = Finder {
        word,
        cancel,
        verify_left: options.verify_files,
        hits_used: 0,
        delivered: false,
        summary: ReferencesSummary::default(),
    };
    let (found, hits) = finder.own_file(req);
    finder.hits_used = hits;
    if let Some(found) = found
        && !finder.cancelled()
    {
        finder.delivered = true;
        sink(vec![found]);
    }
    if finder.cancelled() {
        return finder.finish();
    }
    if finder.summary.truncated {
        // The request's text alone had more than the cap.
        return finder.finish();
    }
    let Ok(root) = fs::canonicalize(root) else {
        return finder.finish();
    };
    let query = SearchQuery {
        pattern: finder.word.clone(),
        regex: false,
        case_sensitive: true,
        whole_word: true,
    };
    // The rest of the cap, and as many again as the request's file has on
    // disk (up to the cap, so the search never holds more than twice it):
    // the search finds those too, and they're skipped. And one more: the
    // search says it's truncated once it reaches its cap, which is so only
    // if there's a hit past ours.
    let disk_copy = finder.disk_copy_hits(&root, &req.rel);
    let search_options = SearchOptions {
        max_matches: search_cap(hits, disk_copy),
        threads: 0,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let done = AtomicBool::new(false);
    let searched = thread::scope(|scope| {
        // The search checks a flag of its own, so the request's is passed
        // on to it, even while no batch arrives.
        scope.spawn(|| {
            while !done.load(Ordering::Relaxed) {
                if cancel.load(Ordering::Relaxed) {
                    stop.store(true, Ordering::Relaxed);
                    return;
                }
                thread::sleep(POLL);
            }
        });
        // Ends the thread above however this ends, a panic included.
        let _done = SetOnDrop(&done);
        search(&root, &query, &search_options, stop.clone(), |batch| {
            let mut out = Vec::new();
            for file in batch {
                if finder.cancelled() {
                    stop.store(true, Ordering::Relaxed);
                    return;
                }
                if finder.hits_used >= MAX_HITS && file.path.strip_prefix(&root) != Ok(&req.rel) {
                    // A hit past the cap, as the disk copy of the request's
                    // file had fewer hits than were allowed for.
                    finder.summary.truncated = true;
                    stop.store(true, Ordering::Relaxed);
                    break;
                }
                out.extend(finder.disk_file(&root, &req.rel, file));
            }
            if finder.cancelled() {
                stop.store(true, Ordering::Relaxed);
                return;
            }
            if !out.is_empty() {
                finder.delivered = true;
                sink(out);
            }
            if finder.cancelled() {
                stop.store(true, Ordering::Relaxed);
            }
        })
    });
    // The pattern is a literal without a line break, so it's valid.
    if let Ok(searched) = searched {
        finder.summary.files_searched = searched.files_searched;
        finder.summary.truncated |= searched.truncated;
    }
    finder.finish()
}

/// The search's cap, after `hits` in the request's text, with `disk_copy`
/// in its file on disk: at most `2 * MAX_HITS + 1`.
fn search_cap(hits: usize, disk_copy: usize) -> usize {
    MAX_HITS - hits.min(MAX_HITS) + disk_copy.min(MAX_HITS) + 1
}

struct SetOnDrop<'a>(&'a AtomicBool);

impl Drop for SetOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// One request's word, and what it may still parse and report.
struct Finder<'a> {
    word: String,
    cancel: &'a AtomicBool,
    /// Files it may still parse, besides the request's own.
    verify_left: usize,
    /// Hits counted towards [`MAX_HITS`] so far, dropped ones included:
    /// the request's text's, and those of the files searched but the
    /// request's own.
    hits_used: usize,
    /// Whether a batch has gone to the sink.
    delivered: bool,
    summary: ReferencesSummary,
}

/// What became of a hit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    /// In a comment or a string.
    Dropped,
    Kept {
        verified: bool,
        is_definition: bool,
    },
}

impl Verdict {
    const UNVERIFIED: Verdict = Verdict::Kept {
        verified: false,
        is_definition: false,
    };
}

impl Finder<'_> {
    fn cancelled(&self) -> bool {
        #[cfg(test)]
        {
            let checks = tests::CANCEL_CHECKS.get() + 1;
            tests::CANCEL_CHECKS.set(checks);
            if checks == tests::CANCEL_AT.get() {
                self.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.cancel.load(Ordering::Relaxed)
    }

    /// The summary; a cancel before any batch went out is an error.
    fn finish(mut self) -> Result<ReferencesSummary, NavError> {
        self.summary.cancelled |= self.cancelled();
        if self.summary.cancelled && !self.delivered {
            return Err(NavError::Cancelled);
        }
        self.summary.word = Some(self.word);
        Ok(self.summary)
    }

    /// The references in the request's own text, of its first [`MAX_HITS`]
    /// whole-word hits (more sets `truncated`), verified by a parse of that
    /// text when its language allows; `None` with none, or when cancelled.
    /// With the number of hits, dropped ones included.
    fn own_file(&mut self, req: &NavRequest) -> (Option<FileReferences>, usize) {
        let mut hits: Vec<(u32, usize, usize)> = Vec::new();
        let found = self.own_refs(req, &mut hits);
        (found, hits.len())
    }

    /// [`Finder::own_file`]'s references, with its hits in `hits`: each
    /// one's row, its line's start and its bytes in the line.
    fn own_refs(
        &mut self,
        req: &NavRequest,
        hits: &mut Vec<(u32, usize, usize)>,
    ) -> Option<FileReferences> {
        let source = req.text.to_string();
        let word = self.word.as_str();
        // The row and line start at `counted`, moved on a chunk at a time.
        let mut rows = Rows::default();
        // Scanned a chunk at a time, checking for a cancel in between,
        // however few matches there are.
        let mut pos = 0;
        'scan: while pos < source.len() {
            if self.cancelled() {
                return None;
            }
            // Candidates starting in `pos..end`, seen whole.
            let end = source.ceil_char_boundary((pos + SCAN_CHUNK).min(source.len()));
            let seen = source.ceil_char_boundary((end + word.len()).min(source.len()));
            let mut at = pos;
            while at < end {
                let Some(found) = source[at..seen].find(word).map(|found| at + found) else {
                    break;
                };
                if found >= end {
                    break;
                }
                if !is_whole_word(&source, found, word.len()) {
                    // Only past its first char, as the search moves on: a
                    // candidate overlapping it may be whole (`$$` in `a$$$`).
                    at = found + source[found..].chars().next().map_or(1, char::len_utf8);
                    continue;
                }
                rows.count(&source, found);
                if hits.len() == MAX_HITS {
                    self.summary.truncated = true;
                    break 'scan;
                }
                hits.push((rows.row, rows.line_start, found - rows.line_start));
                at = found + word.len();
            }
            let next = at.max(end);
            rows.count(&source, next);
            pos = next;
        }
        if hits.is_empty() || self.cancelled() {
            return None;
        }
        let filter = match (req.language).and_then(language_by_name) {
            Some(language) if source.len() <= language.parse_limit() => {
                self.parse(language, &Buffer::new(&source))?
            }
            _ => None,
        };
        let mut refs = Vec::with_capacity(hits.len());
        let len = self.word.len();
        for &(row, line_start, at) in hits.iter() {
            if self.cancelled() {
                return None;
            }
            let range = line_start + at..line_start + at + len;
            let Verdict::Kept {
                verified,
                is_definition,
            } = self.check(filter.as_ref(), &range)
            else {
                continue;
            };
            let (preview, in_preview) = preview(&source, line_start, range.clone());
            refs.push(Reference {
                location: Location {
                    rel: req.rel.clone(),
                    range,
                    row,
                },
                line_number: u64::from(row) + 1,
                preview,
                preview_ranges: in_preview.into_iter().collect(),
                verified,
                is_definition,
            });
        }
        (!refs.is_empty()).then(|| FileReferences {
            rel: req.rel.clone(),
            refs,
        })
    }

    /// How many whole-word hits the request's file has on disk at `rel`,
    /// counted as the search counts them; 0 if it can't be read.
    fn disk_copy_hits(&self, root: &Path, rel: &Path) -> usize {
        let Ok((mut handle, _)) = open_under(root, rel) else {
            return 0;
        };
        let mut head = Vec::with_capacity(2);
        if (&mut handle).take(2).read_to_end(&mut head).is_err() {
            return 0;
        }
        let word = self.word.as_bytes();
        let counted = if utf16_bom(&head) {
            // The search reads it transcoded to UTF-8.
            let reader = Utf16Reader::new(handle, head == [0xFE, 0xFF]);
            count_whole_words(reader, word, self.cancel)
        } else {
            count_whole_words(io::Cursor::new(head).chain(handle), word, self.cancel)
        };
        counted.unwrap_or(0)
    }

    /// The references among a file's search hits, `None` if none is left or
    /// the file is `skip` (the request's own, whose disk copy isn't
    /// reported) or not under `root`.
    fn disk_file(&mut self, root: &Path, skip: &Path, file: FileMatches) -> Option<FileReferences> {
        // `file.rel` is lossy; the path keeps the name's bytes.
        let rel = file.path.strip_prefix(root).ok()?;
        if rel == skip {
            return None;
        }
        // Its hits count towards the cap before any is checked, so dropped
        // ones count too; past the cap, they're left out.
        let mut lines = file.lines;
        let total: usize = lines.iter().map(|line| line.ranges.len()).sum();
        let room = MAX_HITS.saturating_sub(self.hits_used);
        if total > room {
            self.summary.truncated = true;
            let mut left = room;
            lines.retain_mut(|line| {
                line.ranges.truncate(left);
                left -= line.ranges.len();
                !line.ranges.is_empty()
            });
        }
        self.hits_used += total.min(room);
        if lines.is_empty() {
            return None;
        }
        let language = language_name_for_path(rel).and_then(language_by_name);
        let mut loaded = None;
        let mut filter = None;
        if let Ok((handle, len)) = open_under(root, rel) {
            let parse = language
                .filter(|language| len <= language.parse_limit() as u64 && self.verify_left > 0);
            let limit = parse.map_or(MAX_LOAD_BYTES, Language::parse_limit);
            if len <= limit as u64
                && let Some(file) = load(handle, limit, self.cancel)
            {
                if let Some(language) = parse {
                    // Counted when tried, so failures can't exceed it.
                    self.verify_left -= 1;
                    filter = self.parse(language, &file.buffer)?;
                }
                loaded = Some(file);
            }
        }

        let mut refs = Vec::new();
        for line in &lines {
            for hit in &line.ranges {
                if self.cancelled() {
                    return None;
                }
                let start = line.line_offset as usize;
                let raw = start + hit.start..start + hit.end;
                let located = (loaded.as_ref()).and_then(|file| file.locate(&raw, &self.word));
                // The preview is decoded as the file is, so where a hit was
                // found in the text, it's moved as much in the preview.
                let in_preview = match (&loaded, &located) {
                    (Some(file), Some((range, _))) => {
                        let window = file.decoded.map(start + line.preview_start);
                        range.start.checked_sub(window).map(|at| at..at + hit.len())
                    }
                    _ => hit
                        .start
                        .checked_sub(line.preview_start)
                        .map(|at| at..at + hit.len()),
                };
                let preview_ranges: Vec<_> = (in_preview.into_iter())
                    .filter(|range| line.preview.get(range.clone()) == Some(self.word.as_str()))
                    .collect();
                let (range, row, verdict) = match located {
                    Some((range, row)) => {
                        let verdict = self.check(filter.as_ref(), &range);
                        (range, row, verdict)
                    }
                    // Not where the search found it, in a file that changed
                    // since or isn't UTF-8: the search's offsets.
                    None => (
                        raw,
                        u32::try_from(line.line_number.saturating_sub(1)).unwrap_or(u32::MAX),
                        Verdict::UNVERIFIED,
                    ),
                };
                let Verdict::Kept {
                    verified,
                    is_definition,
                } = verdict
                else {
                    continue;
                };
                refs.push(Reference {
                    location: Location {
                        rel: rel.to_path_buf(),
                        range,
                        row,
                    },
                    line_number: line.line_number,
                    preview: line.preview.clone(),
                    preview_ranges,
                    verified,
                    is_definition,
                });
            }
        }
        (!refs.is_empty()).then(|| FileReferences {
            rel: rel.to_path_buf(),
            refs,
        })
    }

    /// Parses `buffer` whole on this thread, for at most [`PARSE_CAP`]: its
    /// [`Filter`] if it has a tree, `Some(None)` if not (too large once
    /// loaded, or the parse failed or overran), `None` if cancelled.
    fn parse(&mut self, language: &'static Language, buffer: &Buffer) -> Option<Option<Filter>> {
        let len = buffer.text().len();
        // Checked again once loaded: invalid UTF-8 grows when replaced.
        if len > language.parse_limit() {
            return Some(None);
        }
        let mut syntax = Syntax::new(language, buffer.current());
        let started = Instant::now();
        let parsed = syntax.parse_until(Duration::MAX, || {
            self.cancelled() || started.elapsed() > PARSE_CAP
        });
        if self.cancelled() {
            return None;
        }
        if !parsed || syntax.failed() || syntax.tree().is_none() {
            return Some(None);
        }
        self.summary.files_verified += 1;
        let definitions = symbols(&syntax)
            .map(|outline| {
                (outline.items.into_iter())
                    .map(|item| item.name_range)
                    .collect()
            })
            .unwrap_or_default();
        Some(Some(Filter {
            syntax,
            len,
            definitions,
            tried: RefCell::default(),
        }))
    }

    /// What becomes of the hit at `range`: dropped if it's in a comment or a
    /// string, kept and verified if it's in neither, and kept unverified
    /// without a filter or if its highlights couldn't be had in full.
    fn check(&mut self, filter: Option<&Filter>, range: &Range<usize>) -> Verdict {
        let Some(filter) = filter else {
            return Verdict::UNVERIFIED;
        };
        let syntax = &filter.syntax;
        let mut tried = filter.tried.borrow_mut();
        let classified = classify(range, filter.len, &mut tried, |query| {
            let started = Instant::now();
            let spans = syntax.highlights(query);
            let took = started.elapsed();
            let missing = syntax.take_missing_languages();
            for name in &missing {
                // Compiled for the next try.
                language_by_name(name);
            }
            Highlighted {
                spans,
                complete: took < QUERY_BUDGET && missing.is_empty() && !syntax.failed(),
            }
        });
        match classified {
            Some(true) => {
                self.summary.dropped += 1;
                Verdict::Dropped
            }
            Some(false) => Verdict::Kept {
                verified: true,
                is_definition: filter.definitions.contains(range),
            },
            None => Verdict::UNVERIFIED,
        }
    }
}

/// One try at a range's highlights.
struct Highlighted {
    spans: Vec<HighlightSpan>,
    /// The query finished in time, with no injected language missing and no
    /// panic: `spans` are all there are.
    complete: bool,
}

/// Whether `word`, in a text of `len` bytes, is in a comment or a string,
/// from the first complete answer `highlight` gives for a range around it;
/// `None` if none of [`HIGHLIGHT_TRIES`] was. Each try asks for a range not
/// in `tried`, the ranges asked of this parse before (by any hit), as an
/// incomplete answer is kept for its range, and would come back quickly.
fn classify(
    word: &Range<usize>,
    len: usize,
    tried: &mut HashSet<Range<usize>>,
    mut highlight: impl FnMut(Range<usize>) -> Highlighted,
) -> Option<bool> {
    for pad in 0..HIGHLIGHT_TRIES {
        let query = word.start.saturating_sub(pad / 2)..(word.end + pad.div_ceil(2)).min(len);
        if !tried.insert(query.clone()) {
            continue;
        }
        let highlighted = highlight(query);
        if highlighted.complete {
            return Some(in_comment_or_string(&highlighted.spans, word));
        }
    }
    None
}

/// A parsed file, its length, its definitions' name ranges, and the
/// ranges its highlights have been asked for.
struct Filter {
    syntax: Syntax,
    len: usize,
    definitions: HashSet<Range<usize>>,
    tried: RefCell<HashSet<Range<usize>>>,
}

/// A file loaded as the editor loads it, and how its bytes map into it.
struct Loaded {
    buffer: Buffer,
    decoded: Decoded,
    /// It starts with a UTF-16 byte order mark: the search read it
    /// transcoded, so its offsets aren't the file's.
    utf16: bool,
}

impl Loaded {
    /// Where the hit at `raw`, bytes in the file, is in the text, and the
    /// row it's on: if the text has `word` there (it doesn't if the file
    /// changed since the search, or the search read it as UTF-16).
    fn locate(&self, raw: &Range<usize>, word: &str) -> Option<(Range<usize>, u32)> {
        if self.utf16 {
            return None;
        }
        let text = self.buffer.text();
        let range = self.decoded.map(raw.start)..self.decoded.map(raw.end);
        let fits = range.len() == word.len()
            && range.end <= text.len()
            && text.is_char_boundary(range.start)
            && text.is_char_boundary(range.end);
        (fits && text.slice_to_string(range.clone()) == word).then(|| {
            let row = text.offset_to_point(range.start).row;
            (range, row)
        })
    }
}

/// How offsets in a file's bytes move once the loader has decoded it
/// ([`fe_buffer::Text::from_reader`]): each `\r\n` loses its `\r`, and each invalid
/// UTF-8 sequence becomes a 3-byte U+FFFD. A lone `\r` becomes a `\n`, the
/// same length.
struct Decoded {
    /// From each offset on, by how much offsets move: ascending.
    shifts: Vec<(usize, isize)>,
}

impl Decoded {
    fn new(raw: &[u8]) -> Decoded {
        let mut shifts = Vec::new();
        let mut delta = 0isize;
        let mut at = 0;
        // As `String::from_utf8_lossy` splits it, which is how the loader
        // replaces invalid sequences.
        for chunk in raw.utf8_chunks() {
            let valid = chunk.valid().as_bytes();
            if valid.contains(&b'\r') {
                for ix in 0..valid.len() {
                    if valid[ix] == b'\r' && valid.get(ix + 1) == Some(&b'\n') {
                        delta -= 1;
                        shifts.push((at + ix + 1, delta));
                    }
                }
            }
            at += valid.len();
            let invalid = chunk.invalid().len();
            if invalid > 0 {
                at += invalid;
                delta += 3 - invalid as isize;
                shifts.push((at, delta));
            }
        }
        Decoded { shifts }
    }

    /// Where offset `raw` of the file is in its decoded text.
    fn map(&self, raw: usize) -> usize {
        let ix = self.shifts.partition_point(|&(from, _)| from <= raw);
        let delta = ix.checked_sub(1).map_or(0, |ix| self.shifts[ix].1);
        raw.saturating_add_signed(delta)
    }
}

/// Whether the `len` bytes at `at` in `text` are a whole word: with no
/// word char on either side, as the search has them ([`is_word_char`]).
fn is_whole_word(text: &str, at: usize, len: usize) -> bool {
    !text[..at].chars().next_back().is_some_and(is_word_char)
        && !text[at + len..].chars().next().is_some_and(is_word_char)
}

/// How many times `word` occurs in what `reader` reads as a whole word,
/// with a char that isn't a word char ([`is_word_char`]), or nothing, on
/// either side; invalid UTF-8 beside it isn't a boundary, as to the search. Read a chunk at a time, however large; `None` if it can't
/// be read or `cancel` is set meanwhile.
fn count_whole_words(mut reader: impl Read, word: &[u8], cancel: &AtomicBool) -> Option<usize> {
    if word.is_empty() {
        return Some(0);
    }
    // The longest UTF-8 char, the context checked on either side.
    const CHAR: usize = 4;
    let mut buf: Vec<u8> = Vec::new();
    // Where in `buf` the next match may start.
    let mut from = 0;
    let mut count = 0;
    let mut chunk = vec![0; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let read = match reader.read(&mut chunk) {
            Ok(read) => read,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return None,
        };
        buf.extend_from_slice(&chunk[..read]);
        let eof = read == 0;
        // Matches starting before `until` have their context in `buf`.
        let until = if eof {
            buf.len()
        } else {
            buf.len().saturating_sub(word.len() + CHAR)
        };
        while from < until {
            let Some(at) = (buf[from..].windows(word.len()))
                .position(|window| window == word)
                .map(|at| from + at)
                .filter(|&at| at < until)
            else {
                from = until;
                break;
            };
            let before = char_before(&buf[at.saturating_sub(CHAR)..at]);
            let after = char_after(&buf[at + word.len()..]);
            let apart =
                |side: Option<Option<char>>| side.is_some_and(|ch| !ch.is_some_and(is_word_char));
            if apart(before) && apart(after) {
                count += 1;
                from = at + word.len();
            } else {
                // A candidate overlapping a rejected one may be whole.
                from = at + 1;
            }
        }
        if eof {
            return Some(count);
        }
        // Keep the bytes a later match's context may need.
        let cut = from.saturating_sub(CHAR);
        buf.drain(..cut);
        from -= cut;
    }
}

/// Whether `bytes` start with a UTF-16 byte order mark, as the search
/// detects one.
fn utf16_bom(bytes: &[u8]) -> bool {
    matches!(bytes, [0xFF, 0xFE, ..] | [0xFE, 0xFF, ..])
}

/// Reads UTF-16 (after its byte order mark) as UTF-8, an unpaired surrogate
/// or odd last byte as U+FFFD.
struct Utf16Reader<R> {
    inner: R,
    big_endian: bool,
    /// Read but not yet decoded: an odd byte, or a high surrogate's two.
    held: Vec<u8>,
    /// Decoded but not yet handed out.
    out: Vec<u8>,
    done: bool,
}

impl<R: Read> Utf16Reader<R> {
    fn new(inner: R, big_endian: bool) -> Self {
        Utf16Reader {
            inner,
            big_endian,
            held: Vec::new(),
            out: Vec::new(),
            done: false,
        }
    }
}

impl<R: Read> Read for Utf16Reader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.out.is_empty() && !self.done {
            let mut bytes = std::mem::take(&mut self.held);
            let start = bytes.len();
            bytes.resize(start + 64 * 1024, 0);
            let read = self.inner.read(&mut bytes[start..])?;
            bytes.truncate(start + read);
            self.done = read == 0;
            let (pairs, odd) = bytes.as_chunks::<2>();
            let mut units: Vec<u16> = (pairs.iter())
                .map(|&pair| match self.big_endian {
                    true => u16::from_be_bytes(pair),
                    false => u16::from_le_bytes(pair),
                })
                .collect();
            let mut held = Vec::new();
            if !self.done
                && let Some(&last) = units.last()
                && (0xD800..0xDC00).contains(&last)
            {
                // A high surrogate, whose pair may come in the next read.
                units.pop();
                held.extend_from_slice(&pairs[units.len()]);
            }
            held.extend_from_slice(odd);
            let mut text: String = char::decode_utf16(units)
                .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
                .collect();
            if self.done && !held.is_empty() {
                text.push(char::REPLACEMENT_CHARACTER);
                held.clear();
            }
            self.held = held;
            self.out = text.into_bytes();
        }
        let n = buf.len().min(self.out.len());
        buf[..n].copy_from_slice(&self.out[..n]);
        self.out.drain(..n);
        Ok(n)
    }
}

/// The char `bytes` end with: `Some(None)` if they're empty, `None` if
/// they end with invalid UTF-8.
fn char_before(bytes: &[u8]) -> Option<Option<char>> {
    if bytes.is_empty() {
        return Some(None);
    }
    (1..=bytes.len().min(4))
        .find_map(|len| std::str::from_utf8(&bytes[bytes.len() - len..]).ok())
        .map(|valid| valid.chars().next_back())
}

/// The char `bytes` start with: `Some(None)` if they're empty, `None` if
/// they start with invalid UTF-8.
fn char_after(bytes: &[u8]) -> Option<Option<char>> {
    if bytes.is_empty() {
        return Some(None);
    }
    (1..=bytes.len().min(4))
        .find_map(|len| std::str::from_utf8(&bytes[..len]).ok())
        .map(|valid| valid.chars().next())
}

/// The preview of the line starting at `line_start` in `source` for the hit
/// at `hit` in it: the line, or a window of it of at most
/// [`PREVIEW_BYTES`] on char boundaries around the hit; and the hit in it,
/// if the window holds it whole. No more than [`PREVIEW_BYTES`] either side
/// of the hit is looked at, however long the line.
fn preview(source: &str, line_start: usize, hit: Range<usize>) -> (String, Option<Range<usize>>) {
    let from = source.floor_char_boundary(line_start.max(hit.start.saturating_sub(PREVIEW_BYTES)));
    let limit = source.floor_char_boundary((hit.end + PREVIEW_BYTES).min(source.len()));
    let line_end = source[hit.end..limit]
        .find('\n')
        .map_or(limit, |at| hit.end + at);
    let line = &source[from..line_end];
    let hit = hit.start - from..hit.end - from;
    if line.len() <= PREVIEW_BYTES {
        return (line.to_owned(), Some(hit));
    }
    let context = PREVIEW_BYTES.saturating_sub(hit.len()) / 2;
    let start = line.floor_char_boundary(hit.start.saturating_sub(context));
    let end = line.floor_char_boundary((start + PREVIEW_BYTES).min(line.len()));
    let window = line[start..end].to_owned();
    let in_window = (hit.end <= end).then(|| hit.start - start..hit.end - start);
    (window, in_window)
}

/// A row and where its line starts, counted up to an offset.
#[derive(Default)]
struct Rows {
    row: u32,
    line_start: usize,
    counted: usize,
}

impl Rows {
    /// Counts the rows of `source` on to `to`.
    fn count(&mut self, source: &str, to: usize) {
        for (ix, byte) in source.as_bytes()[self.counted..to].iter().enumerate() {
            if *byte == b'\n' {
                self.row += 1;
                self.line_start = self.counted + ix + 1;
            }
        }
        self.counted = self.counted.max(to);
    }
}

/// Opens the regular file at `rel` under `root` for reading, as the
/// definitions extraction does, with [`fe_project::open_file_under`]: no
/// symlink is followed on the way, whenever it was swapped in, so a path the
/// search found can't lead out of the project. With its length.
fn open_under(root: &Path, rel: &Path) -> io::Result<(File, u64)> {
    let file = fe_project::open_file_under(root, rel)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::ErrorKind::InvalidInput.into());
    }
    Ok((file, metadata.len()))
}

/// Reads `file` as the loader does ([`Buffer::from_reader`]), unless it has
/// grown past `limit` bytes or `cancel` is set meanwhile.
fn load(file: File, limit: usize, cancel: &AtomicBool) -> Option<Loaded> {
    let mut reader = file.take(limit as u64 + 1);
    let mut raw = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    if raw.len() > limit {
        return None;
    }
    let buffer = Buffer::from_reader(raw.as_slice()).ok()?;
    Some(Loaded {
        decoded: Decoded::new(&raw),
        utf16: utf16_bom(&raw),
        buffer,
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, fs, path::PathBuf, time::Instant};

    use fe_buffer::Text;
    use fe_project::{FileMatches, LineMatch};

    use super::*;

    thread_local! {
        /// How many times a [`Finder`] has checked for a cancel.
        pub(super) static CANCEL_CHECKS: Cell<usize> = const { Cell::new(0) };
        /// The check at which a [`Finder`]'s cancel flag gets set, as if from
        /// another thread; 0 for never.
        pub(super) static CANCEL_AT: Cell<usize> = const { Cell::new(0) };
    }

    fn finder(cancel: &AtomicBool) -> Finder<'_> {
        Finder {
            word: "helper".to_owned(),
            cancel,
            verify_left: 1_000,
            hits_used: 0,
            delivered: false,
            summary: ReferencesSummary::default(),
        }
    }

    /// A TSX input whose parse never ends, in the request's text and on
    /// disk: each parse gives up at the cap, its hits unverified.
    #[test]
    fn a_runaway_parse_is_capped() {
        let dir = tempfile::tempdir().unwrap();
        let text = "x\"\"[[)}x)3,`x";
        fs::write(dir.path().join("b.tsx"), text).unwrap();
        let req = NavRequest {
            root: dir.path().to_path_buf(),
            rel: PathBuf::from("a.tsx"),
            language: Some("tsx"),
            text: Text::new(text),
            offset: 0,
        };
        let started = Instant::now();
        let mut refs = Vec::new();
        let options = ReferenceOptions::default();
        let summary = find(
            dir.path(),
            &options,
            &req,
            &AtomicBool::new(false),
            &mut |batch| refs.extend(batch.into_iter().flat_map(|file| file.refs)),
        )
        .unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
        assert_eq!(refs.len(), 6);
        assert_eq!(summary.files_verified, 0);
        assert!(refs.iter().all(|r| !r.verified));
    }

    #[test]
    fn a_cancel_stops_a_files_hits() {
        let cancel = AtomicBool::new(true);
        let mut finder = finder(&cancel);
        let line = "helper ".repeat(10_000);
        let file = FileMatches {
            path: PathBuf::from("/nonexistent/x.txt"),
            rel: "x.txt".to_owned(),
            lines: vec![LineMatch {
                line_number: 1,
                line_offset: 0,
                ranges: (0..10_000).map(|ix| ix * 7..ix * 7 + 6).collect(),
                preview: line[..512].to_owned(),
                preview_start: 0,
            }],
        };
        assert!(
            finder
                .disk_file(Path::new("/nonexistent"), Path::new("y"), file)
                .is_none()
        );
    }

    #[test]
    fn incomplete_highlights_leave_a_hit_unclassified() {
        let comment = |range: Range<usize>| HighlightSpan {
            range,
            highlight: fe_syntax::Highlight::from_capture("comment").unwrap(),
        };
        let mut tried = HashSet::new();
        let mut asked = Vec::new();
        let none = classify(&(10..16), 100, &mut tried, |query: Range<usize>| {
            asked.push(query.clone());
            Highlighted {
                spans: vec![comment(query)],
                complete: false,
            }
        });
        assert_eq!(none, None);
        assert_eq!(
            asked,
            [10..16, 10..17, 9..17, 9..18, 8..18, 8..19, 7..19, 7..20]
        );

        // Ranges asked for before aren't asked again, as they'd be cached.
        let mut asked = Vec::new();
        let at_end = classify(&(10..16), 16, &mut HashSet::new(), |query| {
            asked.push(query);
            Highlighted {
                spans: Vec::new(),
                complete: asked.len() == 2,
            }
        });
        assert_eq!(at_end, Some(false));
        assert_eq!(asked, [10..16, 9..16]);

        // Nor those asked for another hit of the same parse: `$$` at the end
        // of a text, its hits sharing their padded ranges.
        let mut tried = HashSet::new();
        let mut asked = Vec::new();
        let mut ask = |word: Range<usize>, tried: &mut HashSet<Range<usize>>| {
            classify(&word, 2, tried, |query| {
                asked.push(query);
                Highlighted {
                    spans: Vec::new(),
                    complete: false,
                }
            })
        };
        assert_eq!(ask(0..1, &mut tried), None);
        assert_eq!(ask(1..2, &mut tried), None);
        assert_eq!(asked, [0..1, 0..2, 1..2]);
    }

    /// The request's text is scanned a chunk at a time, with a check for a
    /// cancel after each, whether its matches are rejected or there are
    /// none.
    #[test]
    fn scanning_checks_for_a_cancel() {
        let cancel = AtomicBool::new(false);
        for line in ["helper_two ", "nothing here "] {
            let text = line.repeat(SCAN_CHUNK);
            let req = NavRequest {
                root: PathBuf::from("/nonexistent"),
                rel: PathBuf::from("a.rs"),
                language: Some("rust"),
                text: Text::new(&text),
                offset: 0,
            };
            CANCEL_CHECKS.set(0);
            let (found, hits) = finder(&cancel).own_file(&req);
            assert!(found.is_none() && hits == 0);
            assert!(CANCEL_CHECKS.get() >= line.len(), "{}", CANCEL_CHECKS.get());
        }
    }

    #[test]
    fn utf16_disk_copies_are_counted_transcoded() {
        let dir = tempfile::tempdir().unwrap();
        let text = "helper x helper\u{1F600}helper";
        for (bom, encode) in [
            ([0xFF, 0xFE], u16::to_le_bytes as fn(u16) -> [u8; 2]),
            ([0xFE, 0xFF], u16::to_be_bytes),
        ] {
            let mut bytes = bom.to_vec();
            bytes.extend(text.encode_utf16().flat_map(encode));
            let path = dir.path().join("x.rs");
            fs::write(&path, &bytes).unwrap();
            let cancel = AtomicBool::new(false);
            let root = fs::canonicalize(dir.path()).unwrap();
            assert_eq!(finder(&cancel).disk_copy_hits(&root, Path::new("x.rs")), 3);
            // A few bytes at a time, splitting the surrogate pair.
            let reader = Utf16Reader::new(Trickle(&bytes[2..]), bom == [0xFE, 0xFF]);
            assert_eq!(count_whole_words(reader, b"helper", &cancel), Some(3));
        }
    }

    #[test]
    fn a_long_word_has_no_preview_range_when_cut() {
        let word = "x".repeat(600);
        let line = format!("let {word} = 1;");
        let (window, range) = preview(&line, 0, 4..604);
        assert!(window.len() <= PREVIEW_BYTES);
        assert_eq!(range, None);
        let (_, range) = preview("let x = 1;", 0, 4..5);
        assert_eq!(range, Some(4..5));
        let (line, range) = preview("a\nlet x = 1;\nb", 2, 6..7);
        assert_eq!((line.as_str(), range), ("let x = 1;", Some(4..5)));
        // Only near the hit is looked at, on a line however long.
        let long = format!("{} x {}", "a".repeat(100_000), "b".repeat(100_000));
        let (window, range) = preview(&long, 0, 100_001..100_002);
        assert!(window.len() <= PREVIEW_BYTES);
        assert_eq!(&window[range.unwrap()], "x");
    }

    /// Reads a few bytes at a time, so words and their context straddle
    /// the chunks.
    struct Trickle<'a>(&'a [u8]);

    impl Read for Trickle<'_> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = buf.len().min(self.0.len()).min(3);
            buf[..n].copy_from_slice(&self.0[..n]);
            self.0 = &self.0[n..];
            Ok(n)
        }
    }

    #[test]
    fn whole_words_counted_in_bytes() {
        let mut bytes = "helper xhelper helper_ helper\u{301} \u{e9}helper helper\u{b2} "
            .as_bytes()
            .to_vec();
        bytes.extend(b"\xffhelper\xff helper\xe2\x82 $$$ helper");
        let cancel = AtomicBool::new(false);
        // The first, the one before `\u{b2}` and the last: not those beside
        // invalid UTF-8, which the search doesn't take as a boundary.
        assert_eq!(count_whole_words(&bytes[..], b"helper", &cancel), Some(3));
        assert_eq!(
            count_whole_words(Trickle(&bytes), b"helper", &cancel),
            Some(3)
        );
        // An overlapping candidate after a rejected one, as in `a$$$`.
        assert_eq!(count_whole_words(&b"$$; a$$$"[..], b"$$", &cancel), Some(2));
    }

    #[test]
    fn the_search_cap_is_bounded() {
        assert_eq!(search_cap(1, 0), MAX_HITS);
        assert_eq!(search_cap(1, 5), MAX_HITS + 5);
        assert_eq!(search_cap(0, usize::MAX), 2 * MAX_HITS + 1);
        assert_eq!(search_cap(MAX_HITS, 0), 1);
    }

    /// A cancel set while the request's text is scanned or its hits are
    /// checked, after a long prefix without the word or on one long line
    /// of it, is seen at the next check.
    #[test]
    fn a_cancel_during_a_long_scan_stops_it() {
        let long_prefix = format!("{}\nhelper\n", "x ".repeat(4 * SCAN_CHUNK));
        let long_line = "helper ".repeat(SCAN_CHUNK);
        // While scanning the prefix's 8 chunks; while scanning the long
        // line, then while checking its hits (capped after 2 chunks).
        for (text, ats) in [(&long_prefix, [2, 5]), (&long_line, [2, 20])] {
            for at in ats {
                let cancel = AtomicBool::new(false);
                let req = NavRequest {
                    root: PathBuf::from("/nonexistent"),
                    rel: PathBuf::from("a.txt"),
                    language: None,
                    text: Text::new(text),
                    offset: 0,
                };
                CANCEL_CHECKS.set(0);
                CANCEL_AT.set(at);
                let (found, _) = finder(&cancel).own_file(&req);
                CANCEL_AT.set(0);
                assert!(found.is_none(), "{at}");
                assert_eq!(CANCEL_CHECKS.get(), at, "no more work once cancelled");
            }
        }
    }

    #[test]
    fn decoded_offsets() {
        let decoded = Decoded::new(b"a\r\nb\xffc\rd\xe2\x82");
        let text = "a\nb\u{fffd}c\nd\u{fffd}";
        // a, b, c, d and the end.
        for (raw, expected) in [(0, 0), (3, 2), (5, 6), (7, 8), (10, text.len())] {
            assert_eq!(decoded.map(raw), expected, "{raw}");
        }
    }
}
