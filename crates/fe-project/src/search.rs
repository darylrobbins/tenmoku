//! Project search: every line of the project's files that matches a pattern,
//! with ripgrep's searcher and regex matcher, streamed in batches while the
//! walk goes on, capped and cancellable.

use std::{
    fmt, fs,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, TryRecvError},
    },
    thread,
};

use grep_matcher::{LineMatchKind, LineTerminator, Match, Matcher, NoError};
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{
    BinaryDetection, MmapChoice, Searcher, SearcherBuilder, Sink, SinkContext, SinkMatch,
};
use ignore::{
    DirEntry, WalkState,
    gitignore::{Gitignore, GitignoreBuilder},
};

use crate::listing;

mod replace;

pub use replace::Replacer;

/// The most files in one batch handed to the sink.
const BATCH_FILES: usize = 64;
/// The most matches in one batch, unless a single file has more.
const BATCH_MATCHES: usize = 1_000;
/// The most bytes of a line kept in [`LineMatch::preview`].
const PREVIEW: usize = 512;
/// The longest line a file may have; a file with a longer one is reported in
/// [`SearchSummary::errors`].
const HEAP_LIMIT: usize = 16 << 20;

/// What to search for.
///
/// Searches are line-oriented: a pattern never matches across a line break,
/// and a pattern that names one (such as `a\nb`) is invalid. Lines end at
/// `\n`, and a `\r` before it is part of the terminator.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchQuery {
    pub pattern: String,
    /// A regular expression (Rust's `regex` syntax) rather than a literal.
    pub regex: bool,
    pub case_sensitive: bool,
    /// Matches only where the text before and after the match isn't a word
    /// character (`grep-regex`'s `word`): `foo` matches `foo.bar` and
    /// `(foo)`, not `foobar` or `_foo`.
    pub whole_word: bool,
}

/// Whether `ch` is a word character to [`SearchQuery::whole_word`]:
/// Unicode's `\w` (letters, marks, decimal digits, connector punctuation
/// and joiners), so code matching words in memory agrees with the search.
pub fn is_word_char(ch: char) -> bool {
    regex_syntax::is_word_character(ch)
}

impl SearchQuery {
    /// A case-insensitive literal search for `pattern`: the defaults.
    pub fn literal(pattern: impl Into<String>) -> SearchQuery {
        SearchQuery {
            pattern: pattern.into(),
            regex: false,
            case_sensitive: false,
            whole_word: false,
        }
    }

    /// The searcher's matcher, which picks the lines to look at, holding the
    /// canonical matcher, which finds the ranges in a line without its
    /// terminator. Both match `^` and `$` at line starts and ends, not only
    /// the buffer's, and never match a `\n`. A `\r` not before `\n` is
    /// content.
    fn matcher(&self) -> Result<LineSelector, SearchError> {
        let build = |crlf: bool| {
            RegexMatcherBuilder::new()
                .fixed_strings(!self.regex)
                .case_insensitive(!self.case_sensitive)
                .word(self.whole_word)
                .multi_line(true)
                // `crlf` sets the line terminator too, to `\r\n`, which would
                // keep a `\r` from matching; so it's set back to `\n` after.
                // It also keeps `.` from matching `\r`, unless `.` matches
                // any char: the ban on the line terminator then takes `\n`
                // out of it, which leaves `.` as it is without `crlf`.
                .crlf(crlf)
                .dot_matches_new_line(crlf)
                .line_terminator(Some(b'\n'))
                .build(&self.pattern)
                .map_err(|err| SearchError::InvalidPattern(err.to_string()))
        };
        let line = build(false)?;
        let candidates = if self.regex && sees_past_line_ends(&self.pattern) {
            None
        } else {
            Some(build(true)?)
        };
        Ok(LineSelector { candidates, line })
    }
}

/// Whether `pattern` has a construct that, searched over a buffer of lines
/// with `crlf`, could fail to match a line that it matches once the line's
/// terminator is stripped: a text anchor (`\A`, `\z`, or `^` or `$` with
/// multi-line mode turned off), or turning off dot-all (`.` then misses
/// `\r`) or CRLF mode (`$` then misses the end of a line before `\r\n`).
/// Without those, every difference only lets the buffer search match more.
/// A pattern that doesn't parse is taken to have one.
fn sees_past_line_ends(pattern: &str) -> bool {
    use regex_syntax::ast::{self, AssertionKind, Ast, Flag, GroupKind};

    struct Finder;
    impl ast::Visitor for Finder {
        type Output = ();
        type Err = ();

        fn finish(self) -> Result<(), ()> {
            Ok(())
        }

        fn visit_pre(&mut self, ast: &Ast) -> Result<(), ()> {
            let flags = match ast {
                Ast::Assertion(assertion) => {
                    return match assertion.kind {
                        AssertionKind::StartText | AssertionKind::EndText => Err(()),
                        _ => Ok(()),
                    };
                }
                Ast::Flags(set) => &set.flags,
                Ast::Group(group) => match &group.kind {
                    GroupKind::NonCapturing(flags) => flags,
                    _ => return Ok(()),
                },
                _ => return Ok(()),
            };
            let off = [Flag::MultiLine, Flag::DotMatchesNewLine, Flag::CRLF]
                .into_iter()
                .any(|flag| flags.flag_state(flag) == Some(false));
            if off { Err(()) } else { Ok(()) }
        }
    }

    regex_syntax::ast::parse::Parser::new()
        .parse(pattern)
        .map_or(true, |ast| ast::visit(&ast, Finder).is_err())
}

/// The matcher the searcher picks lines with, which the sink then searches
/// with the canonical matcher. It picks every line the canonical matcher
/// matches without its terminator, and maybe a few more (`needle\r` on
/// `needle\r\n`, `a$` on `a\rb`), which then have no ranges and aren't
/// reported.
///
/// The searcher hands it lines without their `\n` only, so it can't tell
/// the `\r` of a `\r\n` from a `\r` at the end of an unterminated last
/// line, and it picks a line ending in `\r` that matches either way.
struct LineSelector {
    /// The query built with `crlf`, which finds candidate lines in a buffer
    /// of lines: `$` matches before `\r\n`, and `.` still matches a `\r`.
    /// `None` where it could miss lines ([`sees_past_line_ends`]): every
    /// line is then a candidate.
    candidates: Option<RegexMatcher>,
    /// The canonical matcher, for a line without its terminator.
    line: RegexMatcher,
}

impl Matcher for LineSelector {
    type Captures = grep_matcher::NoCaptures;
    type Error = grep_matcher::NoError;

    fn find_at(&self, line: &[u8], at: usize) -> Result<Option<Match>, NoError> {
        if let Some(found) = self.line.find_at(line, at)? {
            return Ok(Some(found));
        }
        match line.strip_suffix(b"\r") {
            Some(line) if at <= line.len() => self.line.find_at(line, at),
            _ => Ok(None),
        }
    }

    /// As [`LineSelector::find_at`]; what the searcher's checks call.
    fn shortest_match_at(&self, line: &[u8], at: usize) -> Result<Option<usize>, NoError> {
        if let Some(end) = self.line.shortest_match_at(line, at)? {
            return Ok(Some(end));
        }
        match line.strip_suffix(b"\r") {
            Some(line) if at <= line.len() => self.line.shortest_match_at(line, at),
            _ => Ok(None),
        }
    }

    fn new_captures(&self) -> Result<grep_matcher::NoCaptures, NoError> {
        Ok(grep_matcher::NoCaptures::new())
    }

    fn line_terminator(&self) -> Option<LineTerminator> {
        Some(LineTerminator::byte(b'\n'))
    }

    fn find_candidate_line(&self, buffer: &[u8]) -> Result<Option<LineMatchKind>, NoError> {
        let Some(candidates) = &self.candidates else {
            // The searcher asks again from the end of the line.
            return Ok((!buffer.is_empty()).then_some(LineMatchKind::Candidate(0)));
        };
        // Never confirmed: the searcher would report the line as it is.
        let at = match candidates.find_candidate_line(buffer)? {
            None => return Ok(None),
            Some(LineMatchKind::Confirmed(at) | LineMatchKind::Candidate(at)) => at,
        };
        // Past the last terminator, as an empty match can be, isn't a line,
        // and nothing matched before it.
        if at == buffer.len() && buffer.ends_with(b"\n") {
            return Ok(None);
        }
        Ok(Some(LineMatchKind::Candidate(at)))
    }
}

/// A glob that doesn't parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobError {
    pub glob: String,
    pub message: String,
}

impl fmt::Display for GlobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid glob `{}`: {}", self.glob, self.message)
    }
}

impl std::error::Error for GlobError {}

/// Narrows a search to some paths by include and exclude globs, which are
/// gitignore-style and relative to the root, as ripgrep's `-g` writes them:
/// `*.rs` matches at any depth, `dir/**` is everything under `dir`, and a
/// trailing `/` matches directories only. Matching is case-sensitive.
///
/// A file or directory matching an exclude glob, itself or through a parent,
/// is skipped, and an excluded directory isn't walked. With includes, only
/// files matching one (itself or through a parent) are searched; directories
/// are walked regardless. Exclude wins. Filters only narrow: ignored and
/// hidden files stay skipped.
#[derive(Clone, Debug, Default)]
pub struct PathFilter {
    include: Option<Arc<Gitignore>>,
    exclude: Option<Arc<Gitignore>>,
}

impl PathFilter {
    /// `root` is what the globs are relative to. Paths are matched already
    /// relative to it, so the matchers hold no root to strip a second time.
    pub fn new(_root: &Path, include: &[&str], exclude: &[&str]) -> Result<PathFilter, GlobError> {
        Ok(PathFilter {
            include: build_globs(include)?,
            exclude: build_globs(exclude)?,
        })
    }

    fn is_empty(&self) -> bool {
        self.include.is_none() && self.exclude.is_none()
    }

    /// Whether `rel`, relative to the root, is excluded.
    fn excludes(&self, rel: &Path, is_dir: bool) -> bool {
        self.exclude
            .as_ref()
            .is_some_and(|globs| globs.matched_path_or_any_parents(rel, is_dir).is_ignore())
    }

    /// Whether the file `rel` passes the include globs.
    fn includes(&self, rel: &Path) -> bool {
        self.include
            .as_ref()
            .is_none_or(|globs| globs.matched_path_or_any_parents(rel, false).is_ignore())
    }
}

fn build_globs(globs: &[&str]) -> Result<Option<Arc<Gitignore>>, GlobError> {
    if globs.is_empty() {
        return Ok(None);
    }
    let mut builder = GitignoreBuilder::new("");
    builder.case_insensitive(false).map_err(|err| GlobError {
        glob: String::new(),
        message: err.to_string(),
    })?;
    for glob in globs {
        if let Some(message) = unclosed_class(glob) {
            return Err(GlobError {
                glob: (*glob).to_string(),
                message: message.to_string(),
            });
        }
        builder.add_line(None, glob).map_err(|err| GlobError {
            glob: (*glob).to_string(),
            message: err.to_string(),
        })?;
    }
    let built = builder.build().map_err(|err| GlobError {
        glob: String::new(),
        message: err.to_string(),
    })?;
    Ok(Some(Arc::new(built)))
}

/// Why `glob` has a `[` that never closes, which the glob parser would take
/// as a literal.
fn unclosed_class(glob: &str) -> Option<&'static str> {
    let mut chars = glob.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            '[' => {
                if matches!(chars.peek(), Some('!' | '^')) {
                    chars.next();
                }
                // A `]` first in the class is a member.
                if chars.peek() == Some(&']') {
                    chars.next();
                }
                loop {
                    match chars.next() {
                        Some(']') => break,
                        Some(_) => {}
                        None => return Some("unclosed character class"),
                    }
                }
            }
            _ => {}
        }
    }
    None
}

/// How much to search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchOptions {
    /// The most match ranges delivered; once there are this many, the search
    /// stops and [`SearchSummary::truncated`] is set.
    pub max_matches: usize,
    /// Walker threads; 0 is the walker's default parallelism. 1 walks on
    /// the calling thread, in path order, so results are deterministic.
    pub threads: usize,
}

impl Default for SearchOptions {
    fn default() -> SearchOptions {
        SearchOptions {
            max_matches: 10_000,
            threads: 0,
        }
    }
}

/// A file's matching lines, in line order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileMatches {
    pub path: PathBuf,
    /// Relative to the root, `/`-separated, lossy where a name isn't UTF-8.
    pub rel: String,
    pub lines: Vec<LineMatch>,
}

/// A matching line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineMatch {
    /// 1-based.
    pub line_number: u64,
    /// The byte offset of the line's start in the file. A UTF-8 byte order
    /// mark is part of the first line.
    pub line_offset: u64,
    /// Every match on the line, as byte ranges in the line, in order: at
    /// least one, though a pattern that matches the empty string (such as
    /// `^$`) gives empty ranges. Ranges outside [`LineMatch::preview`] are
    /// reported too. A UTF-16 file (found by its byte order mark) is
    /// searched transcoded to UTF-8, and its ranges and preview are in the
    /// transcoded line.
    pub ranges: Vec<Range<usize>>,
    /// The line without its terminator, as lossy UTF-8. When that is over
    /// 512 bytes, it's cut to a window of at most 512 bytes on char
    /// boundaries that holds the chars the first match touches (or as many
    /// as fit, from its start). Where the line isn't valid UTF-8, each
    /// replacement character shifts the text after it.
    pub preview: String,
    /// Where the preview starts in the line: a range `r` of the line is at
    /// `r.start - preview_start .. r.end - preview_start` in the preview.
    pub preview_start: usize,
}

/// How a search went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchSummary {
    /// Files whose search began, whatever came of it.
    pub files_searched: usize,
    /// Files delivered to the sink.
    pub files_matched: usize,
    /// Match ranges delivered to the sink.
    pub matches: usize,
    /// Files abandoned for holding a NUL byte.
    pub binary_skipped: usize,
    /// Files that couldn't be searched: unreadable, or with a line over
    /// 16 MiB. Their matches, if any, aren't delivered. Directories the
    /// walk can't read are skipped without an error, as the finder skips
    /// them.
    pub errors: Vec<(PathBuf, String)>,
    /// The match cap stopped the search before it had searched every file,
    /// or left matches out: there may be more than were delivered.
    pub truncated: bool,
    /// `cancel` was set before the search ended.
    pub cancelled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchError {
    /// The pattern isn't a valid regex, or names a line break.
    InvalidPattern(String),
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchError::InvalidPattern(message) => write!(f, "invalid pattern: {message}"),
        }
    }
}

impl std::error::Error for SearchError {}

/// Searches every file under `root` that the project tree shows (the ignore
/// rules of [`crate::FileTree`] with ignored entries hidden, as
/// [`crate::FinderIndex`] has them) for `query`, and hands the matches to
/// `sink` in batches as it goes.
///
/// Runs on the calling thread, which the caller owns: typically a
/// background thread. Walker threads search the files and the calling
/// thread calls `sink`, so `sink` is never called concurrently.
///
/// - All of a file's matches arrive together, in line order. Files arrive
///   in the order their searches end (in path order with `threads: 1`).
///   Files without matches aren't delivered.
/// - A batch holds at most 64 files and, unless one file has more, 1,000
///   matches. It's handed over when full, when no more results are waiting
///   (except with `threads: 1`) and when the walk ends.
/// - Once `max_matches` matches are found, no new file is searched, and
///   exactly that many match ranges are delivered: the last line delivered
///   may keep only some of its ranges.
/// - Once `cancel` is set, `sink` isn't called again (even when it's set
///   from inside `sink`) and the search stops soon after: `cancel` is
///   checked before each file and at each match.
/// - A file with a NUL byte is abandoned. Files aren't memory-mapped.
///
/// An invalid pattern is an error, returned before anything is read.
///
/// The walk is [`crate::FinderIndex::walk`]'s, with its known limits:
/// ignore files are opened following symlinks, an ignore file that is a
/// FIFO blocks the walk, and directories are listed by path. Files
/// themselves are opened with [`crate::open_file_under`], following no
/// symlink below the root, so a directory swapped for one since the walk
/// listed it can't lead the search out of the project; and only regular
/// files are searched.
pub fn search(
    root: impl AsRef<Path>,
    query: &SearchQuery,
    options: &SearchOptions,
    cancel: Arc<AtomicBool>,
    sink: impl FnMut(Vec<FileMatches>),
) -> Result<SearchSummary, SearchError> {
    search_inner(
        root.as_ref(),
        query,
        options,
        None,
        &PathFilter::default(),
        cancel,
        sink,
    )
}

/// [`search()`] over only the paths `filter` lets through.
pub fn search_filtered(
    root: impl AsRef<Path>,
    query: &SearchQuery,
    options: &SearchOptions,
    filter: &PathFilter,
    cancel: Arc<AtomicBool>,
    sink: impl FnMut(Vec<FileMatches>),
) -> Result<SearchSummary, SearchError> {
    search_inner(root.as_ref(), query, options, None, filter, cancel, sink)
}

/// [`search()`], but for the file at `excluded`, relative to `root`, whose
/// text is searched another way, such as from memory: it isn't opened, so
/// none of its matches is delivered or counts towards
/// [`SearchOptions::max_matches`]. It's still counted in
/// [`SearchSummary::files_searched`] when the walk reaches it.
pub fn search_excluding(
    root: impl AsRef<Path>,
    query: &SearchQuery,
    options: &SearchOptions,
    excluded: &Path,
    cancel: Arc<AtomicBool>,
    sink: impl FnMut(Vec<FileMatches>),
) -> Result<SearchSummary, SearchError> {
    search_inner(
        root.as_ref(),
        query,
        options,
        Some(excluded),
        &PathFilter::default(),
        cancel,
        sink,
    )
}

fn search_inner(
    root: &Path,
    query: &SearchQuery,
    options: &SearchOptions,
    excluded: Option<&Path>,
    filter: &PathFilter,
    cancel: Arc<AtomicBool>,
    sink: impl FnMut(Vec<FileMatches>),
) -> Result<SearchSummary, SearchError> {
    let matcher = query.matcher()?;
    // Files are opened under the root without following a symlink, so it's
    // reached through none.
    let open_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let shared = Shared {
        root,
        open_root: &open_root,
        excluded,
        filter,
        matcher,
        cancel: &cancel,
        max: options.max_matches,
        found: AtomicUsize::new(0),
        files_searched: AtomicUsize::new(0),
        binary_skipped: AtomicUsize::new(0),
        truncated: AtomicBool::new(false),
    };
    let mut collector = Collector {
        sink,
        cancel: &cancel,
        max: options.max_matches,
        batch: Vec::new(),
        batch_matches: 0,
        taken: 0,
        summary: SearchSummary::default(),
    };
    if !cancel.load(Ordering::Relaxed) {
        if options.threads == 1 {
            search_sequential(&shared, &mut collector);
        } else {
            search_parallel(&shared, options.threads, &mut collector);
        }
        collector.flush();
    }
    let mut summary = collector.summary;
    summary.files_searched = shared.files_searched.load(Ordering::Relaxed);
    summary.binary_skipped = shared.binary_skipped.load(Ordering::Relaxed);
    summary.truncated |= shared.truncated.load(Ordering::Relaxed);
    summary.cancelled = cancel.load(Ordering::Relaxed);
    Ok(summary)
}

/// Walks on the calling thread, in path order, flushing full batches only.
fn search_sequential<F: FnMut(Vec<FileMatches>)>(
    shared: &Shared<'_>,
    collector: &mut Collector<'_, F>,
) {
    let mut searchers = Searchers::new();
    let walker = shared
        .walk_builder()
        .sort_by_file_name(|a, b| a.cmp(b))
        .build();
    for entry in walker {
        // Unreadable entries are skipped, as the tree shows nothing for them.
        let Ok(entry) = entry else {
            continue;
        };
        match shared.search_entry(&mut searchers, &entry) {
            Outcome::Quit => break,
            Outcome::Skip => {}
            Outcome::Found(file) => collector.add(file),
            Outcome::Error(path, message) => collector.summary.errors.push((path, message)),
        }
    }
}

/// Searches on the walker's threads, scoped to this call, and collects on
/// the calling thread.
fn search_parallel<F: FnMut(Vec<FileMatches>)>(
    shared: &Shared<'_>,
    threads: usize,
    collector: &mut Collector<'_, F>,
) {
    let (tx, rx) = mpsc::channel();
    let walker = shared.walk_builder().threads(threads).build_parallel();
    thread::scope(|scope| {
        scope.spawn(|| {
            walker.run(|| {
                let tx = tx.clone();
                let mut searchers = Searchers::new();
                Box::new(move |entry| {
                    let Ok(entry) = entry else {
                        return WalkState::Continue;
                    };
                    match shared.search_entry(&mut searchers, &entry) {
                        Outcome::Quit => WalkState::Quit,
                        Outcome::Skip => WalkState::Continue,
                        outcome => match tx.send(outcome) {
                            Ok(()) => WalkState::Continue,
                            // The collector stopped listening.
                            Err(_) => WalkState::Quit,
                        },
                    }
                })
            });
            drop(tx);
        });
        loop {
            let outcome = match rx.try_recv() {
                Ok(outcome) => outcome,
                Err(TryRecvError::Empty) => {
                    collector.flush();
                    match rx.recv() {
                        Ok(outcome) => outcome,
                        Err(_) => break,
                    }
                }
                Err(TryRecvError::Disconnected) => break,
            };
            if shared.cancel.load(Ordering::Relaxed) {
                break;
            }
            match outcome {
                Outcome::Found(file) => collector.add(file),
                Outcome::Error(path, message) => collector.summary.errors.push((path, message)),
                Outcome::Quit | Outcome::Skip => {}
            }
        }
        // Dropping the receiver makes the walker's threads quit at their
        // next send.
        drop(rx);
    });
}

/// One walker thread's searchers.
struct Searchers {
    /// For files without a UTF-16 byte order mark, searched as they are on
    /// disk (a UTF-8 BOM included), so offsets are file offsets.
    plain: Searcher,
    /// For files with a UTF-16 BOM, transcoded to UTF-8. Every line is
    /// passed to the sink, which counts the source bytes before each match.
    utf16: Searcher,
}

impl Searchers {
    fn new() -> Searchers {
        let mut builder = SearcherBuilder::new();
        builder
            .line_number(true)
            .binary_detection(BinaryDetection::quit(b'\0'))
            .memory_map(MmapChoice::never())
            .heap_limit(Some(HEAP_LIMIT));
        Searchers {
            plain: builder.bom_sniffing(false).build(),
            utf16: builder.bom_sniffing(true).passthru(true).build(),
        }
    }
}

/// Whether `file` starts with a UTF-16 byte order mark, little- or
/// big-endian.
fn is_utf16(file: &fs::File) -> bool {
    let mut bom = [0; 2];
    matches!(read_start(file, &mut bom), Ok(2)) && matches!(bom, [0xFF, 0xFE] | [0xFE, 0xFF])
}

/// Reads the start of `file` into `buf`, leaving its position where it was
/// (at the start, as it is opened).
#[cfg(unix)]
fn read_start(file: &fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, 0)
}

/// Windows' positional read moves the file's position: it is put back.
#[cfg(windows)]
fn read_start(file: &fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    use std::io::Seek as _;
    let read = std::os::windows::fs::FileExt::seek_read(file, buf, 0);
    (&*file).seek(std::io::SeekFrom::Start(0))?;
    read
}

/// What searching one walk entry came to.
enum Outcome {
    /// Stop the walk: cancelled, or the cap is reached.
    Quit,
    /// Nothing to deliver.
    Skip,
    Found(FileMatches),
    Error(PathBuf, String),
}

/// What the walker's threads share.
struct Shared<'a> {
    root: &'a Path,
    /// `root` canonicalized, which files are opened under.
    open_root: &'a Path,
    /// A file not to search, relative to `root`.
    excluded: Option<&'a Path>,
    filter: &'a PathFilter,
    /// Picks the lines to look at, and finds the ranges in them.
    matcher: LineSelector,
    cancel: &'a AtomicBool,
    max: usize,
    /// Matches in files whose search has ended (and wasn't abandoned).
    found: AtomicUsize,
    files_searched: AtomicUsize,
    binary_skipped: AtomicUsize,
    truncated: AtomicBool,
}

impl Shared<'_> {
    /// The listing walk, not entering what the filter excludes.
    fn walk_builder(&self) -> ignore::WalkBuilder {
        let root = self.root.to_path_buf();
        let filter = self.filter.clone();
        listing::walk_builder(self.root, move |entry| {
            if filter.exclude.is_none() || entry.depth() == 0 {
                return true;
            }
            let rel = entry.path().strip_prefix(&root).unwrap_or(entry.path());
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            !filter.excludes(rel, is_dir)
        })
    }

    fn search_entry(&self, searchers: &mut Searchers, entry: &DirEntry) -> Outcome {
        if self.cancel.load(Ordering::Relaxed) {
            return Outcome::Quit;
        }
        // Regular files only: no directories, symlinks (never followed, as
        // in the tree) or FIFOs (which would block).
        if !entry.file_type().is_some_and(|kind| kind.is_file()) {
            return Outcome::Skip;
        }
        let path = entry.path();
        let rel = path.strip_prefix(self.root).unwrap_or(path);
        if !self.filter.is_empty() {
            // A file given as the root is filtered by its name.
            let name = Path::new(self.root.file_name().unwrap_or_default());
            let filtered = if rel.as_os_str().is_empty() {
                name
            } else {
                rel
            };
            if !self.filter.includes(filtered)
                || (rel.as_os_str().is_empty() && self.filter.excludes(filtered, false))
            {
                return Outcome::Skip;
            }
        }
        if self.excluded == Some(rel) {
            self.files_searched.fetch_add(1, Ordering::Relaxed);
            return Outcome::Skip;
        }
        if self.found.load(Ordering::Relaxed) >= self.max {
            self.truncated.store(true, Ordering::Relaxed);
            return Outcome::Quit;
        }
        self.files_searched.fetch_add(1, Ordering::Relaxed);
        // Opened without following a symlink at any component below the
        // root, and without blocking on a FIFO, in case either replaced the
        // file or a directory above it since the walk saw it.
        let opened = match (rel.as_os_str().is_empty(), self.open_root.parent()) {
            // The root itself, a file: under its directory.
            (true, Some(dir)) => {
                let name = self.open_root.file_name().unwrap_or_default();
                listing::open_file_under(dir, Path::new(name))
            }
            _ => listing::open_file_under(self.open_root, rel),
        };
        let file = match opened {
            Ok(file) => file,
            Err(err) => return Outcome::Error(path.to_path_buf(), err.to_string()),
        };
        if !file.metadata().is_ok_and(|meta| meta.is_file()) {
            return Outcome::Skip;
        }
        let utf16 = is_utf16(&file);
        let mut sink = FileSink {
            shared: self,
            lines: Vec::new(),
            count: 0,
            binary: false,
            // Past the BOM, which the transcoder drops.
            source_offset: utf16.then_some(2),
        };
        let searcher = if utf16 {
            &mut searchers.utf16
        } else {
            &mut searchers.plain
        };
        if let Err(err) = searcher.search_file(&self.matcher, &file, &mut sink) {
            return Outcome::Error(path.to_path_buf(), err.to_string());
        }
        if sink.binary {
            self.binary_skipped.fetch_add(1, Ordering::Relaxed);
            return Outcome::Skip;
        }
        if sink.lines.is_empty() {
            return Outcome::Skip;
        }
        self.found.fetch_add(sink.count, Ordering::Relaxed);
        Outcome::Found(FileMatches {
            path: path.to_path_buf(),
            rel: rel.to_string_lossy().into_owned(),
            lines: sink.lines,
        })
    }
}

/// Collects one file's matching lines.
struct FileSink<'s, 'a> {
    shared: &'s Shared<'a>,
    lines: Vec<LineMatch>,
    /// Ranges in `lines`.
    count: usize,
    binary: bool,
    /// For a transcoded UTF-16 file, the offset in the file of the next
    /// line the searcher passes.
    source_offset: Option<u64>,
}

impl FileSink<'_, '_> {
    /// The offset in the file of `line`, a line from the searcher at
    /// `offset` in what it searched, and moves past it.
    fn line_offset(&mut self, line: &[u8], offset: u64) -> u64 {
        let Some(source) = &mut self.source_offset else {
            return offset;
        };
        let at = *source;
        let units = String::from_utf8_lossy(line).encode_utf16().count();
        *source += 2 * units as u64;
        at
    }
}

impl Sink for FileSink<'_, '_> {
    type Error = std::io::Error;

    fn matched(&mut self, _: &Searcher, mat: &SinkMatch<'_>) -> Result<bool, std::io::Error> {
        let shared = self.shared;
        let line_offset = self.line_offset(mat.bytes(), mat.absolute_byte_offset());
        if shared.cancel.load(Ordering::Relaxed) {
            return Ok(false);
        }
        let found = shared.found.load(Ordering::Relaxed) + self.count;
        if found >= shared.max {
            shared.truncated.store(true, Ordering::Relaxed);
            return Ok(false);
        }
        let line = strip_terminator(mat.bytes());
        let mut ranges = Vec::new();
        let mut cancelled = false;
        // Cancel and the cap are checked at each match, so a line of many
        // matches doesn't hold either up.
        let _ = shared.matcher.line.find_iter(line, |m| {
            #[cfg(test)]
            ON_MATCH.with_borrow_mut(|hook| hook.as_mut().map(|hook| hook()));
            if shared.cancel.load(Ordering::Relaxed) {
                cancelled = true;
                return false;
            }
            ranges.push(m.start()..m.end());
            let found = shared.found.load(Ordering::Relaxed) + self.count;
            found + ranges.len() < shared.max
        });
        if cancelled {
            return Ok(false);
        }
        let found = shared.found.load(Ordering::Relaxed) + self.count;
        let room = shared.max.saturating_sub(found);
        if ranges.len() > room {
            ranges.truncate(room);
        }
        // Stop once the cap is reached, rather than read on to the next
        // match: the rest of the file could still fail to read.
        let full = found + ranges.len() >= shared.max;
        if full {
            shared.truncated.store(true, Ordering::Relaxed);
        }
        if let Some(first) = ranges.first() {
            let (preview, preview_start) = preview(line, first);
            self.count += ranges.len();
            self.lines.push(LineMatch {
                line_number: mat.line_number().unwrap_or(0),
                line_offset,
                ranges,
                preview,
                preview_start,
            });
        }
        Ok(!full)
    }

    fn context(&mut self, _: &Searcher, context: &SinkContext<'_>) -> Result<bool, std::io::Error> {
        self.line_offset(context.bytes(), context.absolute_byte_offset());
        Ok(!self.shared.cancel.load(Ordering::Relaxed))
    }

    fn binary_data(&mut self, _: &Searcher, _: u64) -> Result<bool, std::io::Error> {
        self.binary = true;
        Ok(false)
    }
}

#[cfg(test)]
#[path = "search_filter_tests.rs"]
mod filter_tests;

#[cfg(test)]
thread_local! {
    /// Called at each match found on a matching line, before `cancel` and
    /// the cap are checked.
    pub(crate) static ON_MATCH: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

/// `line` without its `\n` or `\r\n`.
fn strip_terminator(line: &[u8]) -> &[u8] {
    match line.strip_suffix(b"\n") {
        Some(line) => line.strip_suffix(b"\r").unwrap_or(line),
        None => line,
    }
}

/// [`LineMatch::preview`] and [`LineMatch::preview_start`] for `line`, whose
/// first match is `first`.
///
/// The window is made of whole decoding units of the line (a char, or an
/// invalid sequence that lossy decoding replaces with U+FFFD), measured by
/// their decoded size, so it never splits a char and replacement characters
/// count at their size. It holds the units the match touches (or as many as
/// fit, from the first), then as much on each side as fits, the before side
/// taking up to half of what's left first. Its preview is
/// `String::from_utf8_lossy` of the window, and the work and memory it takes
/// are bounded by the preview's size, not the line's or the match's.
pub(crate) fn preview(line: &[u8], first: &Range<usize>) -> (String, usize) {
    // A decoded unit is never smaller than its bytes, so the units that can
    // fit before the match start within PREVIEW bytes of it.
    let mut pos = sync(line, first.start.saturating_sub(PREVIEW));
    let mut before = Vec::new();
    while pos < line.len() {
        let (len, out) = next_unit(line, pos);
        if pos + len > first.start {
            break;
        }
        before.push((len, out));
        pos += len;
    }
    // `pos` is now the start of the unit holding the match's first byte.
    let mut start = pos;
    let mut end = pos;
    let mut size = 0;
    // Takes the unit at `end` if it fits.
    let take = |end: &mut usize, size: &mut usize| {
        if *end >= line.len() {
            return false;
        }
        let (len, out) = next_unit(line, *end);
        if *size + out > PREVIEW {
            return false;
        }
        *end += len;
        *size += out;
        true
    };
    while end < first.end.max(start + 1) && take(&mut end, &mut size) {}
    if end >= first.end {
        let half = size + (PREVIEW - size) / 2;
        let mut before = before.into_iter().rev().peekable();
        while let Some(&(len, out)) = before.peek() {
            if size + out > half {
                break;
            }
            before.next();
            start -= len;
            size += out;
        }
        while take(&mut end, &mut size) {}
        for (len, out) in before {
            if size + out > PREVIEW {
                break;
            }
            start -= len;
            size += out;
        }
    }
    (
        String::from_utf8_lossy(&line[start..end]).into_owned(),
        start,
    )
}

/// A position at or before `ix` where lossy decoding of `line` from there
/// gives the same units as decoding the whole line: the nearest byte that
/// isn't a UTF-8 continuation byte, if one is within 3 bytes (a longer run
/// of continuation bytes can't belong to a char, so each is a unit of its
/// own), else `ix`.
fn sync(line: &[u8], ix: usize) -> usize {
    (ix.saturating_sub(3)..=ix)
        .rev()
        .find(|&at| line.get(at).is_none_or(|&byte| byte & 0xC0 != 0x80))
        .unwrap_or(ix)
}

/// The decoding unit of `line` at `pos`, where a unit starts: its length in
/// `line`, then its size when decoded lossily (as a char, or as U+FFFD for an
/// invalid sequence, which `String::from_utf8_lossy` replaces as one).
fn next_unit(line: &[u8], pos: usize) -> (usize, usize) {
    let bytes = &line[pos..(pos + 4).min(line.len())];
    match std::str::from_utf8(bytes) {
        Ok(text) => {
            let len = text.chars().next().map_or(0, char::len_utf8);
            (len, len)
        }
        Err(err) if err.valid_up_to() > 0 => {
            let text = std::str::from_utf8(&bytes[..err.valid_up_to()]).unwrap_or_default();
            let len = text.chars().next().map_or(0, char::len_utf8);
            (len, len)
        }
        // Cut short by the end of the line, as only it can cut a 4-byte
        // window: the rest of the line is one invalid sequence.
        Err(err) => (
            err.error_len().unwrap_or(bytes.len()),
            char::REPLACEMENT_CHARACTER.len_utf8(),
        ),
    }
}

/// Batches results for the sink, on the calling thread, and trims them to
/// the cap.
struct Collector<'a, F> {
    sink: F,
    cancel: &'a AtomicBool,
    max: usize,
    batch: Vec<FileMatches>,
    batch_matches: usize,
    /// Matches added, delivered or waiting in `batch`.
    taken: usize,
    summary: SearchSummary,
}

impl<F: FnMut(Vec<FileMatches>)> Collector<'_, F> {
    fn add(&mut self, mut file: FileMatches) {
        if self.cancel.load(Ordering::Relaxed) {
            return;
        }
        let total: usize = file.lines.iter().map(|line| line.ranges.len()).sum();
        let count = total.min(self.max - self.taken);
        if count < total {
            self.summary.truncated = true;
            let mut left = count;
            file.lines.retain_mut(|line| {
                line.ranges.truncate(left);
                left -= line.ranges.len();
                !line.ranges.is_empty()
            });
        }
        if count == 0 {
            return;
        }
        if !self.batch.is_empty() && self.batch_matches + count > BATCH_MATCHES {
            self.flush();
        }
        self.batch.push(file);
        self.batch_matches += count;
        self.taken += count;
        if self.batch.len() >= BATCH_FILES || self.batch_matches >= BATCH_MATCHES {
            self.flush();
        }
    }

    /// Hands the batch to the sink, unless cancelled.
    fn flush(&mut self) {
        if self.batch.is_empty() {
            return;
        }
        let batch = std::mem::take(&mut self.batch);
        let matches = std::mem::take(&mut self.batch_matches);
        if !self.cancel.load(Ordering::Relaxed) {
            self.summary.files_matched += batch.len();
            self.summary.matches += matches;
            (self.sink)(batch);
        }
    }
}
