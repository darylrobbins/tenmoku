//! Channel 3's bodies: `SearchHost` (§ 5), over fe-project's search and
//! the finder's file list.

use crate::PathBytes;
use serde::{Deserialize, Serialize};
use std::ops::Range;

/// A `Request` body on channel 3.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchOp {
    /// Searches the files under `root` that `filter` lets through. Replies
    /// [`SearchData::Matches`] batches, then one [`SearchData::Summary`].
    /// `excluded`, relative to `root`, is a file searched another way (from
    /// a buffer): it isn't opened, as in fe-project's `search_excluding`.
    Search {
        root: PathBytes,
        query: SearchQuery,
        options: SearchOptions,
        filter: PathFilter,
        excluded: Option<PathBytes>,
    },
    /// Lists the files under `root` that `filter` lets through, for Quick
    /// Open. Replies [`SearchData::Files`] batches.
    Files { root: PathBytes, filter: PathFilter },
}

/// A `Data` body on channel 3.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchData {
    Matches(Vec<FileMatches>),
    Summary(SearchSummary),
    /// Paths relative to the root.
    Files(Vec<PathBytes>),
}

/// fe-project's `SearchQuery`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
    pub whole_word: bool,
}

/// fe-project's `SearchOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchOptions {
    pub max_matches: u64,
    /// Walker threads on the host; 0 is the walker's default.
    pub threads: u32,
}

/// fe-project's `PathFilter`, as the globs it is built from.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathFilter {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

/// fe-project's `FileMatches`: a file's matching lines, in line order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMatches {
    pub path: PathBytes,
    /// Relative to the root, `/`-separated, lossy where a name isn't UTF-8.
    pub rel: String,
    pub lines: Vec<LineMatch>,
}

/// fe-project's `LineMatch`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineMatch {
    /// 1-based.
    pub line_number: u64,
    pub line_offset: u64,
    /// Byte ranges in the line.
    pub ranges: Vec<Range<u64>>,
    pub preview: String,
    pub preview_start: u64,
}

/// fe-project's `SearchSummary`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchSummary {
    pub files_searched: u64,
    pub files_matched: u64,
    pub matches: u64,
    pub binary_skipped: u64,
    pub errors: Vec<(PathBytes, String)>,
    pub truncated: bool,
    pub cancelled: bool,
}
