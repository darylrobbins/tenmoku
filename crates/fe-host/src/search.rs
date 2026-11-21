//! [`SearchHost`]: a host's project search and file list, as fe-project's
//! search and finder make them (D § 5).
//!
//! The query, options, matches and summary types were fe-project's
//! (`search.rs`), which re-exports them where they were. The filter goes as
//! its globs, which the host compiles ([`SearchFilter`]).

use std::{
    fmt, io,
    ops::Range,
    path::{Path, PathBuf},
};

use crate::Stream;

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

/// Which paths under the root a search or file list takes: fe-project's
/// `PathFilter` as its globs, which the host compiles, and the file
/// `search_excluding` leaves out. The default takes every path.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchFilter {
    /// gitignore-style globs relative to the root, as fe-project's
    /// `PathFilter::new` takes them: with any, only files matching one
    /// (themselves or through a parent) are taken.
    pub include: Vec<String>,
    /// Globs whose files and directories are skipped. Exclude wins.
    pub exclude: Vec<String>,
    /// A file, relative to the root, whose text is searched another way,
    /// such as from memory: a search doesn't open it, so none of its
    /// matches is delivered or counts towards [`SearchOptions::max_matches`],
    /// though it's still counted in [`SearchSummary::files_searched`]; a
    /// file list leaves it out.
    pub excluded: Option<PathBuf>,
}

impl SearchFilter {
    /// The filter of `include` and `exclude` globs.
    pub fn globs(include: &[&str], exclude: &[&str]) -> SearchFilter {
        SearchFilter {
            include: include.iter().map(|glob| (*glob).to_owned()).collect(),
            exclude: exclude.iter().map(|glob| (*glob).to_owned()).collect(),
            excluded: None,
        }
    }

    /// Every path but `excluded`, relative to the root.
    pub fn excluding(excluded: impl Into<PathBuf>) -> SearchFilter {
        SearchFilter {
            excluded: Some(excluded.into()),
            ..SearchFilter::default()
        }
    }

    /// Whether it takes every path.
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty() && self.excluded.is_none()
    }
}

/// Why a search or file list didn't run: the end of its stream. fe-project's
/// `SearchError` and `GlobError` word the first two.
#[derive(Debug)]
pub enum SearchFailed {
    /// The pattern isn't a valid regex, or names a line break.
    InvalidPattern(String),
    /// A glob of the [`SearchFilter`] doesn't parse.
    InvalidGlob { glob: String, message: String },
    /// The host couldn't run it.
    Io(io::Error),
}

impl fmt::Display for SearchFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SearchFailed::InvalidPattern(message) => write!(f, "invalid pattern: {message}"),
            SearchFailed::InvalidGlob { glob, message } => {
                write!(f, "invalid glob {glob:?}: {message}")
            }
            SearchFailed::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for SearchFailed {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SearchFailed::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for SearchFailed {
    fn from(error: io::Error) -> Self {
        SearchFailed::Io(error)
    }
}

/// How a file list went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilesSummary {
    /// The walk ran to the end: it wasn't cancelled.
    pub complete: bool,
}

/// [`SearchHost::search`]'s stream: batches of files' matches, then the
/// summary.
pub type SearchStream = Stream<Vec<FileMatches>, Result<SearchSummary, SearchFailed>>;

/// [`SearchHost::files`]'s stream: batches of paths relative to the root,
/// then whether the walk ran to the end.
pub type FilesStream = Stream<Vec<PathBuf>, Result<FilesSummary, SearchFailed>>;

/// A host's project search and file list. Both run in the background,
/// streaming batches as they go, and stop soon after their stream is
/// dropped (D § 11: a new query cancels the old one). Their local impl is
/// fe-project's `LocalSearch`.
pub trait SearchHost: Send + Sync {
    /// Searches every file under `root` that the project tree shows and
    /// `filter` takes for `query`: fe-project's `search`, `search_filtered`
    /// and `search_excluding`, whose docs have the rules. Each item is one
    /// of their batches, in the order they hand them over; the stream ends
    /// with their summary, or their error.
    fn search(
        &self,
        root: &Path,
        query: &SearchQuery,
        options: &SearchOptions,
        filter: &SearchFilter,
    ) -> SearchStream;

    /// Every file under `root` that the project tree shows and `filter`
    /// takes, as paths relative to `root`: fe-project's
    /// `FinderIndex::walk`, Quick Open's list, in its batches (less what
    /// `filter` leaves out, and none left empty by it).
    fn files(&self, root: &Path, filter: &SearchFilter) -> FilesStream;
}
