//! Test coverage reports read into a per-file line model, found in the
//! project and reloaded when they change. Pure: no GPUI, no painting (the
//! gutter marks are card two's); the design calls are in
//! `docs/decisions/cee3td8pva-test-coverage-i-read-lcov-and-cobertura.md`.
//!
//! Two standard formats, told apart by content, never by extension:
//!
//! - **lcov** (`cargo llvm-cov --lcov`, c8, istanbul, nyc, `coverage lcov`):
//!   `SF:` opens a file's record and `end_of_record` closes it; `DA:<line>,
//!   <hits>` marks a line covered (hits > 0) or uncovered (0); `BRDA:<line>,
//!   <block>,<branch>,<taken|->` records a branch, and a covered line with a
//!   branch never taken is partial. Other records (`TN:`, `FN:`, `FNDA:`,
//!   `LF:`, `LH:`, anything unknown) are skipped, a CRLF is a line end, and
//!   an `SF:` seen again for a file merges into the first: hits add, and a
//!   branch taken in either is taken. The text is streamed, a line at a time.
//! - **Cobertura XML** (`coverage xml`, istanbul's `cobertura`, `cargo
//!   llvm-cov --cobertura`): each `<class filename=..>`'s `<lines><line
//!   number= hits= branch= condition-coverage=..>`; a `branch="true"` line
//!   with hits > 0 and condition coverage under 100% is partial. Relative
//!   `filename`s resolve through each `<sources><source>`, then the project
//!   root. No XML crate: a small scanner reads the few elements needed and
//!   refuses a DOCTYPE and any entity besides XML's five and character
//!   references (no external entities, no entity expansion).
//!
//! Anything else (JaCoCo XML, gcov text, Clover XML) is refused as an
//! unsupported report, naming its first line. Lines are 1-based; a line
//! with no `DA` (no `<line>`) isn't instrumented and has no mark.
//!
//! Paths are made absolute against the project root and the report's
//! folder. One inside the root is canonicalised, symlinks resolved, so it
//! matches an open file's canonical path; one outside is kept, as written
//! but normalised, for a file open from elsewhere, and never read from
//! disk. Where the filesystem is case-insensitive (macOS, Windows) lookups
//! fold case.
//!
//! [`find_report`] looks in a fixed list of places, never walking the tree;
//! [`CoverageSource`] follows the report found, one stat per check, and
//! parses it again on a background thread when its modification time or
//! length changes, keeping the report it had when the new one fails.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    fs::{self, File},
    hash::{DefaultHasher, Hash, Hasher},
    io::{self, BufRead, BufReader, Read},
    path::{Component, Path, PathBuf},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant, SystemTime},
};

use fe_project::FsChange;
use kiln_l10n::tr;

/// The most files one report may cover; past it the load fails.
pub const MAX_FILES: usize = 200_000;
/// The most line records (`DA`, `BRDA`, `<line>`) one report may hold.
pub const MAX_LINE_RECORDS: usize = 20_000_000;
/// The largest Cobertura report read: XML is read whole, lcov streamed.
pub const MAX_XML_BYTES: u64 = 64 << 20;
/// The longest lcov line, as a path or a record can be.
pub const MAX_TEXT_LINE: usize = 1 << 20;
/// The deepest nesting of XML elements: Cobertura's is six.
const MAX_XML_DEPTH: usize = 64;
/// The most paths a glob in `coverage.report` may match, folders on the
/// way included: past it, discovery fails rather than guess.
pub const MAX_GLOB_MATCHES: usize = 10_000;
/// How much of an unsupported report's first line its error shows.
const FIRST_LINE_SHOWN: usize = 80;

/// How often [`CoverageSource::poll`] stats the report without a live file
/// watcher, and looks again for a newer one.
pub const POLL_INTERVAL: Duration = Duration::from_secs(2);
pub const DISCOVER_INTERVAL: Duration = Duration::from_secs(10);

/// What a report says of a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineCover {
    /// Run at least once, every branch on it taken.
    Covered,
    /// Never run.
    Uncovered,
    /// Run, but a branch on it never taken.
    Partial,
}

/// A file's instrumented lines, 1-based.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileCoverage {
    pub lines: BTreeMap<u32, LineCover>,
}

impl FileCoverage {
    /// Line `line`'s mark (1-based), or `None` where it isn't instrumented.
    pub fn mark(&self, line: u32) -> Option<LineCover> {
        self.lines.get(&line).copied()
    }
}

/// A whole report, by file.
#[derive(Clone, Debug, Default)]
pub struct CoverageReport {
    /// By path: canonical when inside the project root.
    pub files: HashMap<PathBuf, FileCoverage>,
    /// The report file's modification time, when it was read from one.
    pub modified: Option<SystemTime>,
    /// Where the filesystem folds case: the paths of `files` by their
    /// folded spelling.
    folded: Option<HashMap<String, PathBuf>>,
}

/// An open file's coverage, and whether the report is older than the file.
#[derive(Clone, Copy, Debug)]
pub struct FileState<'a> {
    pub coverage: &'a FileCoverage,
    /// The report was written before the file last was: its lines may have
    /// moved.
    pub stale: bool,
}

impl CoverageReport {
    /// The coverage of the file at `path`, a canonical path for a file
    /// inside the project.
    pub fn file(&self, path: &Path) -> Option<&FileCoverage> {
        if let Some(file) = self.files.get(path) {
            return Some(file);
        }
        let folded = self.folded.as_ref()?;
        self.files.get(folded.get(&fold(path))?)
    }

    /// Line `line` (1-based) of the file at `path`.
    pub fn mark(&self, path: &Path, line: u32) -> Option<LineCover> {
        self.file(path)?.mark(line)
    }

    /// Whether the report is older than a file last modified at
    /// `file_modified`. A report newer than the file, or as old, isn't.
    pub fn stale_for(&self, file_modified: SystemTime) -> bool {
        self.modified.is_some_and(|report| report < file_modified)
    }

    /// The file at `path`'s coverage, stale when the report is older than
    /// `file_modified`.
    pub fn file_state(
        &self,
        path: &Path,
        file_modified: Option<SystemTime>,
    ) -> Option<FileState<'_>> {
        let coverage = self.file(path)?;
        let stale = file_modified.is_some_and(|modified| self.stale_for(modified));
        Some(FileState { coverage, stale })
    }
}

/// Why a report didn't load, or wasn't found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoverageError {
    /// The report couldn't be opened or read.
    Read { path: PathBuf, error: String },
    /// Nothing in it but whitespace.
    Empty,
    /// Neither lcov nor Cobertura: its first line, cut short.
    Unsupported { first_line: String },
    /// An XML report with a DOCTYPE, which could declare entities.
    Doctype,
    /// An XML report naming an entity other than XML's own.
    Entity { name: String },
    /// XML that isn't well-formed at `line` (1-based), or ends early.
    Malformed { line: usize },
    /// An lcov record at `line` (1-based) that doesn't parse, or is outside
    /// a file's record.
    LcovMalformed { line: usize },
    /// An lcov file's record not ended: a report still being written.
    Truncated,
    /// An XML report over `limit` bytes.
    TooLarge { limit: u64 },
    /// More than `limit` files.
    TooManyFiles { limit: usize },
    /// More than `limit` line records.
    TooManyLines { limit: usize },
    /// An lcov line, `line` (1-based), longer than `limit` bytes.
    LineTooLong { line: usize, limit: usize },
    /// The background load ended without an answer.
    LoadFailed,
    /// No report where one is looked for.
    NoReport,
    /// Only coverage.py's `.coverage` database, which isn't read.
    CoverageDatabase,
    /// `coverage.report` names no file.
    SettingNoMatch { setting: String },
    /// `coverage.report`'s glob matches more than `limit` paths.
    SettingTooBroad { setting: String, limit: usize },
    /// `coverage.report` names a place outside the project.
    SettingOutsideRoot { setting: String },
}

impl fmt::Display for CoverageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Read { path, error } => tr!(
                "workbench-coverage-read-failed",
                path = path.display().to_string(),
                error = error.as_str(),
            ),
            Self::Empty => tr!("workbench-coverage-empty"),
            Self::Unsupported { first_line } => {
                tr!("workbench-coverage-unsupported", line = first_line.as_str())
            }
            Self::Doctype => tr!("workbench-coverage-doctype"),
            Self::Entity { name } => tr!("workbench-coverage-entity", name = name.as_str()),
            Self::Malformed { line } => {
                tr!("workbench-coverage-malformed", line = line.to_string())
            }
            Self::LcovMalformed { line } => {
                tr!("workbench-coverage-lcov-malformed", line = line.to_string())
            }
            Self::Truncated => tr!("workbench-coverage-truncated"),
            Self::TooLarge { limit } => {
                tr!(
                    "workbench-coverage-too-large",
                    limit = (limit >> 20).to_string()
                )
            }
            Self::TooManyFiles { limit } => {
                tr!(
                    "workbench-coverage-too-many-files",
                    limit = limit.to_string()
                )
            }
            Self::TooManyLines { limit } => {
                tr!(
                    "workbench-coverage-too-many-lines",
                    limit = limit.to_string()
                )
            }
            Self::LineTooLong { line, limit } => tr!(
                "workbench-coverage-line-too-long",
                line = line.to_string(),
                limit = (limit >> 10).to_string(),
            ),
            Self::LoadFailed => tr!("workbench-coverage-load-failed"),
            Self::NoReport => tr!("workbench-coverage-none"),
            Self::CoverageDatabase => tr!("workbench-coverage-database"),
            Self::SettingNoMatch { setting } => {
                tr!(
                    "workbench-coverage-setting-no-match",
                    setting = setting.as_str()
                )
            }
            Self::SettingTooBroad { setting, limit } => tr!(
                "workbench-coverage-setting-too-broad",
                setting = setting.as_str(),
                limit = limit.to_string(),
            ),
            Self::SettingOutsideRoot { setting } => {
                tr!(
                    "workbench-coverage-setting-outside",
                    setting = setting.as_str()
                )
            }
        };
        f.write_str(&text)
    }
}

impl std::error::Error for CoverageError {}

/// The caps a load holds to: [`Limits::DEFAULT`] but in tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub files: usize,
    pub line_records: usize,
    pub xml_bytes: u64,
    pub text_line: usize,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        files: MAX_FILES,
        line_records: MAX_LINE_RECORDS,
        xml_bytes: MAX_XML_BYTES,
        text_line: MAX_TEXT_LINE,
    };
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Reads the report at `path` for the project at `root`.
pub fn load_report(path: &Path, root: &Path) -> Result<CoverageReport, CoverageError> {
    let read_error = |error: io::Error| CoverageError::Read {
        path: path.to_owned(),
        error: error.to_string(),
    };
    let file = File::open(path).map_err(read_error)?;
    let modified = file.metadata().and_then(|meta| meta.modified()).ok();
    let report_dir = path.parent().unwrap_or(root);
    let mut report = parse_report(file, report_dir, root, &Limits::DEFAULT)?;
    report.modified = modified;
    Ok(report)
}

/// Reads a report from `reader`: lcov or Cobertura, by its content. Paths
/// in it resolve against `root` and `report_dir`, the report's folder.
pub fn parse_report(
    reader: impl Read,
    report_dir: &Path,
    root: &Path,
    limits: &Limits,
) -> Result<CoverageReport, CoverageError> {
    let mut reader = BufReader::with_capacity(64 << 10, reader);
    let io_error = |error: io::Error| CoverageError::Read {
        path: report_dir.to_owned(),
        error: error.to_string(),
    };
    let first = skip_leading_space(&mut reader).map_err(io_error)?;
    let raw = match first {
        None => return Err(CoverageError::Empty),
        Some(b'<') => {
            let mut data = Vec::new();
            (reader.by_ref().take(limits.xml_bytes.saturating_add(1)))
                .read_to_end(&mut data)
                .map_err(io_error)?;
            if data.len() as u64 > limits.xml_bytes {
                return Err(CoverageError::TooLarge {
                    limit: limits.xml_bytes,
                });
            }
            let text = match std::str::from_utf8(&data) {
                Ok(text) => text,
                Err(error) => {
                    let valid = &data[..error.valid_up_to()];
                    let line = 1 + valid.iter().filter(|&&b| b == b'\n').count();
                    return Err(CoverageError::Malformed { line });
                }
            };
            parse_cobertura(text, limits)?
        }
        Some(_) => parse_lcov(&mut reader, limits).map_err(|error| match error {
            LcovError::Io(error) => io_error(error),
            LcovError::Coverage(error) => error,
        })?,
    };
    Ok(build(raw, report_dir, root))
}

/// Skips a UTF-8 byte order mark and leading whitespace; the first byte
/// after, left unread, or `None` at the end.
fn skip_leading_space(reader: &mut impl BufRead) -> io::Result<Option<u8>> {
    let available = reader.fill_buf()?;
    if available.starts_with(b"\xEF\xBB\xBF") {
        reader.consume(3);
    }
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(None);
        }
        match available.iter().position(|b| !b.is_ascii_whitespace()) {
            Some(at) => {
                let first = available[at];
                reader.consume(at);
                return Ok(Some(first));
            }
            None => {
                let all = available.len();
                reader.consume(all);
            }
        }
    }
}

/// A report as read, before its paths resolve: files by their spelling in
/// it.
#[derive(Default)]
struct RawReport {
    files: HashMap<Vec<u8>, RawFile>,
    /// Cobertura's `<source>`s, in order.
    sources: Vec<String>,
}

#[derive(Default)]
struct RawFile {
    lines: HashMap<u32, RawLine>,
    /// lcov's branches, by line and a hash of the block and branch, and
    /// whether each was taken.
    branches: HashMap<(u32, u64), bool>,
}

#[derive(Clone, Copy, Default)]
struct RawLine {
    hits: u64,
    /// Cobertura's condition coverage.
    conditions: Conditions,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum Conditions {
    #[default]
    Unknown,
    Missed,
    Full,
}

impl Conditions {
    /// Two records of one line: one with every condition met wins.
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Unknown, other) | (other, Self::Unknown) => other,
            (Self::Full, _) | (_, Self::Full) => Self::Full,
            (Self::Missed, Self::Missed) => Self::Missed,
        }
    }
}

impl RawLine {
    fn merge(&mut self, other: RawLine) {
        self.hits = self.hits.saturating_add(other.hits);
        self.conditions = self.conditions.merge(other.conditions);
    }
}

impl RawFile {
    fn merge(&mut self, other: RawFile) {
        for (line, raw) in other.lines {
            self.lines.entry(line).or_default().merge(raw);
        }
        for (key, taken) in other.branches {
            *self.branches.entry(key).or_insert(false) |= taken;
        }
    }

    fn into_coverage(self) -> FileCoverage {
        let missed_branch: HashSet<u32> = (self.branches.iter())
            .filter(|(_, taken)| !**taken)
            .map(|((line, _), _)| *line)
            .collect();
        let lines = (self.lines.into_iter())
            .map(|(line, raw)| {
                let mark = if raw.hits == 0 {
                    LineCover::Uncovered
                } else if missed_branch.contains(&line) || raw.conditions == Conditions::Missed {
                    LineCover::Partial
                } else {
                    LineCover::Covered
                };
                (line, mark)
            })
            .collect();
        FileCoverage { lines }
    }
}

/// Counts records and files against the caps.
struct Counter<'a> {
    limits: &'a Limits,
    records: usize,
}

impl Counter<'_> {
    fn record(&mut self) -> Result<(), CoverageError> {
        self.records += 1;
        if self.records > self.limits.line_records {
            return Err(CoverageError::TooManyLines {
                limit: self.limits.line_records,
            });
        }
        Ok(())
    }

    /// The entry for `name`, a file not seen before counted.
    fn file<'r>(
        &self,
        files: &'r mut HashMap<Vec<u8>, RawFile>,
        name: &[u8],
    ) -> Result<&'r mut RawFile, CoverageError> {
        if !files.contains_key(name) {
            if files.len() >= self.limits.files {
                return Err(CoverageError::TooManyFiles {
                    limit: self.limits.files,
                });
            }
            files.insert(name.to_owned(), RawFile::default());
        }
        Ok(files.get_mut(name).expect("inserted above"))
    }
}

enum LcovError {
    Io(io::Error),
    Coverage(CoverageError),
}

impl From<CoverageError> for LcovError {
    fn from(error: CoverageError) -> Self {
        Self::Coverage(error)
    }
}

/// lcov's tracefile, streamed: the first line must open it (`TN:` or
/// `SF:`).
fn parse_lcov(reader: &mut impl BufRead, limits: &Limits) -> Result<RawReport, LcovError> {
    let mut raw = RawReport::default();
    let mut counter = Counter { limits, records: 0 };
    let mut buf = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    let mut number = 0;
    while next_line(reader, &mut buf, limits.text_line, number + 1)? {
        number += 1;
        let mut line = buf.as_slice();
        while let [rest @ .., b'\n' | b'\r'] = line {
            line = rest;
        }
        if number == 1 && !(line.starts_with(b"TN:") || line.starts_with(b"SF:")) {
            return Err(unsupported(line).into());
        }
        let malformed = || CoverageError::LcovMalformed { line: number };
        if let Some(path) = line.strip_prefix(b"SF:") {
            // A file's record opened inside another's: the first never
            // ended.
            if current.is_some() || trim_bytes(path).is_empty() {
                return Err(malformed().into());
            }
            counter.file(&mut raw.files, path)?;
            current = Some(path.to_owned());
        } else if line == b"end_of_record" {
            current = None;
        } else if let Some(record) = line.strip_prefix(b"DA:") {
            // A record outside a file's, or one that doesn't parse, is a
            // report written halfway or broken: refused, not skipped.
            let name = current.as_ref().ok_or_else(malformed)?;
            let mut fields = record.split(|&b| b == b',');
            let line = fields.next().and_then(line_index).ok_or_else(malformed)?;
            let hits = fields.next().and_then(hit_count).ok_or_else(malformed)?;
            counter.record()?;
            let Some(line) = line else { continue };
            let file = counter.file(&mut raw.files, name)?;
            (file.lines.entry(line).or_default()).merge(RawLine {
                hits,
                ..RawLine::default()
            });
        } else if let Some(record) = line.strip_prefix(b"BRDA:") {
            let name = current.as_ref().ok_or_else(malformed)?;
            let mut fields = record.splitn(4, |&b| b == b',');
            let line = fields.next().and_then(line_index).ok_or_else(malformed)?;
            let (Some(block), Some(branch), Some(taken)) =
                (fields.next(), fields.next(), fields.next())
            else {
                return Err(malformed().into());
            };
            if trim_bytes(block).is_empty() || trim_bytes(branch).is_empty() {
                return Err(malformed().into());
            }
            let taken = match trim_bytes(taken) {
                b"-" => false,
                taken => hit_count(taken).ok_or_else(malformed)? > 0,
            };
            counter.record()?;
            let Some(line) = line else { continue };
            let mut hasher = DefaultHasher::new();
            (block, branch).hash(&mut hasher);
            let file = counter.file(&mut raw.files, name)?;
            *file
                .branches
                .entry((line, hasher.finish()))
                .or_insert(false) |= taken;
        }
    }
    if current.is_some() {
        // A file's record never ended: the report is still being written.
        return Err(LcovError::Coverage(CoverageError::Truncated));
    }
    Ok(raw)
}

/// Reads one line, its end included, into `buf`: false at the end.
fn next_line(
    reader: &mut impl BufRead,
    buf: &mut Vec<u8>,
    limit: usize,
    number: usize,
) -> Result<bool, LcovError> {
    buf.clear();
    loop {
        let available = reader.fill_buf().map_err(LcovError::Io)?;
        if available.is_empty() {
            return Ok(!buf.is_empty());
        }
        let (take, done) = match available.iter().position(|&b| b == b'\n') {
            Some(at) => (at + 1, true),
            None => (available.len(), false),
        };
        // Room for the line end, CRLF.
        if buf.len() + take > limit.saturating_add(2) {
            return Err(CoverageError::LineTooLong {
                line: number,
                limit,
            }
            .into());
        }
        buf.extend_from_slice(&available[..take]);
        reader.consume(take);
        if done {
            return Ok(true);
        }
    }
}

/// A 1-based line number.
fn line_number(text: &[u8]) -> Option<u32> {
    let line: u32 = std::str::from_utf8(text).ok()?.trim().parse().ok()?;
    (line > 0).then_some(line)
}

/// A line number as lcov writes it: `Some` 1-based line, `None` for
/// line 0 (some tools' placeholder, no line to mark), or an error (outer
/// `None`) when it isn't a number.
fn line_index(text: &[u8]) -> Option<Option<u32>> {
    let line: u32 = std::str::from_utf8(text).ok()?.trim().parse().ok()?;
    Some((line > 0).then_some(line))
}

/// A hit count: a whole number, or a decimal as some tools write; one
/// below zero counts none.
fn hit_count(text: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(text).ok()?.trim();
    if let Ok(hits) = text.parse::<u64>() {
        return Some(hits);
    }
    let hits: f64 = text.parse().ok()?;
    hits.is_finite().then(|| hits.max(0.0) as u64)
}

fn unsupported(first_line: &[u8]) -> CoverageError {
    let text = String::from_utf8_lossy(first_line);
    let text = text.trim();
    let mut shown: String = text.chars().take(FIRST_LINE_SHOWN).collect();
    if shown.len() < text.len() {
        shown.push('…');
    }
    CoverageError::Unsupported { first_line: shown }
}

/// A piece of XML.
enum Token<'a> {
    Start {
        name: &'a str,
        attrs: &'a str,
        empty: bool,
    },
    End {
        name: &'a str,
    },
    Text(&'a str),
    /// CDATA: text taken as it is, not decoded.
    CData(&'a str),
}

/// The few parts of XML a Cobertura report uses: elements, attributes,
/// text, comments, CDATA and processing instructions. A DOCTYPE or any
/// other declaration is refused.
struct Scanner<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Scanner<'a> {
    fn malformed(&self, at: usize) -> CoverageError {
        CoverageError::Malformed {
            line: line_at(self.text, at),
        }
    }

    /// The next token and where it starts, `None` at the end.
    fn next(&mut self) -> Result<Option<(usize, Token<'a>)>, CoverageError> {
        loop {
            let start = self.at;
            let rest = &self.text[start..];
            if rest.is_empty() {
                return Ok(None);
            }
            if !rest.starts_with('<') {
                let end = rest.find('<').unwrap_or(rest.len());
                self.at += end;
                return Ok(Some((start, Token::Text(&rest[..end]))));
            }
            if rest.starts_with("<?") {
                self.at += 2 + self.find(start + 2, "?>")? + 2;
                continue;
            }
            if rest.starts_with("<!--") {
                self.at += 4 + self.find(start + 4, "-->")? + 3;
                continue;
            }
            if let Some(cdata) = rest.strip_prefix("<![CDATA[") {
                let end = self.find(start + 9, "]]>")?;
                self.at += 9 + end + 3;
                return Ok(Some((start, Token::CData(&cdata[..end]))));
            }
            if rest.starts_with("<!") {
                return Err(CoverageError::Doctype);
            }
            let end = tag_end(rest).ok_or_else(|| self.malformed(start))?;
            self.at += end + 1;
            let inner = &rest[1..end];
            if let Some(name) = inner.strip_prefix('/') {
                return Ok(Some((start, Token::End { name: name.trim() })));
            }
            let (inner, empty) = match inner.strip_suffix('/') {
                Some(inner) => (inner, true),
                None => (inner, false),
            };
            let name_end = inner
                .find(|c: char| c.is_ascii_whitespace())
                .unwrap_or(inner.len());
            let name = &inner[..name_end];
            if name.is_empty() {
                return Err(self.malformed(start));
            }
            let attrs = &inner[name_end..];
            return Ok(Some((start, Token::Start { name, attrs, empty })));
        }
    }

    /// Where `needle` is, past `from`, relative to it.
    fn find(&self, from: usize, needle: &str) -> Result<usize, CoverageError> {
        self.text[from..]
            .find(needle)
            .ok_or_else(|| self.malformed(from))
    }
}

/// The `>` that ends the tag `tag` starts, outside quoted values.
fn tag_end(tag: &str) -> Option<usize> {
    let mut quote = None;
    for (at, byte) in tag.bytes().enumerate().skip(1) {
        match (quote, byte) {
            (None, b'"' | b'\'') => quote = Some(byte),
            (Some(open), _) if byte == open => quote = None,
            (None, b'>') => return Some(at),
            (None, b'<') => return None,
            _ => {}
        }
    }
    None
}

/// The 1-based line `at` is on.
fn line_at(text: &str, at: usize) -> usize {
    1 + text.as_bytes()[..at.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// The text of the line `at` is on.
fn line_text(text: &str, at: usize) -> &str {
    let start = text[..at].rfind('\n').map_or(0, |at| at + 1);
    let end = text[at..].find('\n').map_or(text.len(), |end| at + end);
    &text[start..end]
}

/// A tag's attributes, values undecoded: an error when they aren't
/// `name="value"` or `name='value'`.
fn attributes(mut attrs: &str) -> Result<Vec<(&str, &str)>, ()> {
    let mut out = Vec::new();
    loop {
        attrs = attrs.trim_start();
        if attrs.is_empty() {
            return Ok(out);
        }
        let eq = attrs.find('=').ok_or(())?;
        let name = attrs[..eq].trim();
        if name.is_empty() || name.contains(|c: char| c.is_ascii_whitespace()) {
            return Err(());
        }
        let value = attrs[eq + 1..].trim_start();
        let quote = value
            .chars()
            .next()
            .filter(|c| matches!(c, '"' | '\''))
            .ok_or(())?;
        let end = value[1..].find(quote).ok_or(())?;
        out.push((name, &value[1..1 + end]));
        attrs = &value[1 + end + 1..];
    }
}

/// Text or an attribute's value with its references replaced: XML's five
/// entities and character references. Any other entity is refused.
fn decode(text: &str) -> Result<std::borrow::Cow<'_, str>, CoverageError> {
    if !text.contains('&') {
        return Ok(text.into());
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let end = (after
            .find(|c: char| !(c.is_alphanumeric() || matches!(c, '#' | '_' | '-' | '.'))))
        .unwrap_or(after.len());
        let name = &after[..end];
        if !after[end..].starts_with(';') {
            // A bare `&`: not well-formed, refused as the entity it starts.
            return Err(CoverageError::Entity {
                name: name.to_owned(),
            });
        }
        let decoded = match name {
            "lt" => Some('<'),
            "gt" => Some('>'),
            "amp" => Some('&'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => match name.strip_prefix('#') {
                Some(hex) if hex.starts_with(['x', 'X']) => u32::from_str_radix(&hex[1..], 16)
                    .ok()
                    .and_then(char::from_u32),
                Some(decimal) => decimal.parse().ok().and_then(char::from_u32),
                None => None,
            },
        };
        let Some(decoded) = decoded else {
            return Err(CoverageError::Entity {
                name: name.to_owned(),
            });
        };
        out.push(decoded);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out.into())
}

/// A Cobertura report.
fn parse_cobertura(text: &str, limits: &Limits) -> Result<RawReport, CoverageError> {
    let mut scanner = Scanner { text, at: 0 };
    let mut raw = RawReport::default();
    let mut counter = Counter { limits, records: 0 };
    let mut stack: Vec<&str> = Vec::new();
    let mut root_seen = false;
    let mut source: Option<String> = None;
    let mut class: Option<Vec<u8>> = None;
    loop {
        let token = match scanner.next() {
            Ok(Some(token)) => token,
            Ok(None) => break,
            // A DOCTYPE before the root names its element: another format's
            // is that format, unsupported, before it's a DOCTYPE.
            Err(CoverageError::Doctype) if !root_seen => {
                let at = scanner.at;
                let declared = text[at..].strip_prefix("<!DOCTYPE").map(|rest| {
                    let rest = rest.trim_start();
                    let end = rest
                        .find(|c: char| c.is_ascii_whitespace() || matches!(c, '[' | '>'))
                        .unwrap_or(rest.len());
                    &rest[..end]
                });
                return Err(match declared {
                    Some(name) if name != "coverage" => unsupported(line_text(text, at).as_bytes()),
                    _ => CoverageError::Doctype,
                });
            }
            Err(error) => return Err(error),
        };
        let (at, token) = token;
        match token {
            Token::Text(chunk) => text_chunk(&mut source, root_seen, &decode(chunk)?, (at, text))?,
            Token::CData(chunk) => text_chunk(&mut source, root_seen, chunk, (at, text))?,
            Token::Start { name, attrs, empty } => {
                let attrs = attributes(attrs).map_err(|()| scanner.malformed(at))?;
                let mut values = HashMap::new();
                for (attr, value) in attrs {
                    values.insert(attr, decode(value)?);
                }
                if !root_seen {
                    root_seen = true;
                    if name != "coverage" || values.contains_key("clover") {
                        return Err(unsupported(line_text(text, at).as_bytes()));
                    }
                } else if stack.is_empty() {
                    // Past the root's end.
                    return Err(scanner.malformed(at));
                }
                let parent = stack.last().copied();
                match name {
                    "project" if parent == Some("coverage") => {
                        // Clover's layout under a `<coverage>` root.
                        return Err(unsupported(line_text(text, at).as_bytes()));
                    }
                    "source" if parent == Some("sources") && !empty => {
                        source = Some(String::new());
                    }
                    "class" if !empty => {
                        class = match values.get("filename") {
                            Some(filename) => {
                                let filename = filename.as_bytes();
                                counter.file(&mut raw.files, filename)?;
                                Some(filename.to_owned())
                            }
                            None => None,
                        };
                    }
                    // Only the class's own lines: a `<method>`'s are copies
                    // of some of them, and could disagree on conditions.
                    "line"
                        if parent == Some("lines")
                            && stack.len() >= 2
                            && stack[stack.len() - 2] == "class" =>
                    {
                        if let Some(file) = &class
                            && let Some((line, raw_line)) = cobertura_line(&values)
                        {
                            counter.record()?;
                            let file = counter.file(&mut raw.files, file)?;
                            file.lines.entry(line).or_default().merge(raw_line);
                        }
                    }
                    _ => {}
                }
                if !empty {
                    if stack.len() >= MAX_XML_DEPTH {
                        return Err(scanner.malformed(at));
                    }
                    stack.push(name);
                }
            }
            Token::End { name } => {
                if stack.pop() != Some(name) {
                    return Err(scanner.malformed(at));
                }
                match name {
                    "source" => {
                        if let Some(source) = source.take() {
                            let source = source.trim();
                            if !source.is_empty() {
                                raw.sources.push(source.to_owned());
                            }
                        }
                    }
                    "class" => class = None,
                    _ => {}
                }
            }
        }
    }
    if !root_seen {
        return Err(CoverageError::Empty);
    }
    if !stack.is_empty() {
        return Err(scanner.malformed(text.len()));
    }
    Ok(raw)
}

/// Text between tags: a `<source>`'s, collected; before the root, only
/// whitespace.
fn text_chunk(
    source: &mut Option<String>,
    root_seen: bool,
    chunk: &str,
    at: (usize, &str),
) -> Result<(), CoverageError> {
    if let Some(source) = source {
        source.push_str(chunk);
    } else if !root_seen && !chunk.trim().is_empty() {
        let (at, text) = at;
        return Err(unsupported(line_text(text, at).as_bytes()));
    }
    Ok(())
}

/// A `<line>`'s number and record.
fn cobertura_line(values: &HashMap<&str, std::borrow::Cow<'_, str>>) -> Option<(u32, RawLine)> {
    let line = line_number(values.get("number")?.as_bytes())?;
    let hits = hit_count(values.get("hits")?.as_bytes())?;
    let branch = values
        .get("branch")
        .is_some_and(|branch| branch.trim() == "true");
    let conditions = match values.get("condition-coverage") {
        Some(coverage) if branch => match condition_coverage(coverage) {
            Some(true) => Conditions::Full,
            Some(false) => Conditions::Missed,
            None => Conditions::Unknown,
        },
        _ => Conditions::Unknown,
    };
    Some((line, RawLine { hits, conditions }))
}

/// Whether `50% (1/2)` is every condition: by the count in brackets, or
/// else the percentage.
fn condition_coverage(text: &str) -> Option<bool> {
    if let Some(open) = text.find('(') {
        let inner = text[open + 1..].split(')').next()?;
        let (met, all) = inner.split_once('/')?;
        let met: u64 = met.trim().parse().ok()?;
        let all: u64 = all.trim().parse().ok()?;
        return Some(met >= all);
    }
    let percent: f64 = text.split('%').next()?.trim().parse().ok()?;
    Some(percent >= 100.0)
}

/// Turns spellings into paths, made absolute and, inside the project,
/// canonical.
struct Resolver {
    root: PathBuf,
    canonical_root: PathBuf,
    /// Canonical directories, by their spelling.
    dirs: HashMap<PathBuf, PathBuf>,
}

impl Resolver {
    fn new(root: &Path) -> Self {
        let root = normalize(root);
        let canonical_root = dunce::canonicalize(&root).unwrap_or_else(|_| root.clone());
        Self {
            root,
            canonical_root,
            dirs: HashMap::new(),
        }
    }

    fn inside(&self, path: &Path) -> bool {
        path.starts_with(&self.root) || path.starts_with(&self.canonical_root)
    }

    /// `raw` as a path: an absolute one as it is; a relative one against
    /// each of `bases` in turn, the first that exists inside the project,
    /// or else against the first.
    fn resolve(&mut self, raw: &[u8], bases: &[PathBuf]) -> PathBuf {
        let raw = trim_bytes(raw);
        if is_foreign_absolute(raw) {
            return path_from_bytes(raw);
        }
        let path = path_from_bytes(raw);
        if path.is_absolute() {
            return self.canonical(normalize(&path));
        }
        let candidates: Vec<PathBuf> = bases
            .iter()
            .map(|base| normalize(&base.join(&path)))
            .collect();
        let found = (candidates.iter())
            .find(|candidate| self.inside(candidate) && fs::symlink_metadata(candidate).is_ok());
        match found.or(candidates.first()) {
            Some(candidate) => self.canonical(candidate.clone()),
            None => normalize(&self.root.join(&path)),
        }
    }

    /// `path` with its symlinks resolved when it's inside the project; as
    /// it is outside, never read.
    fn canonical(&mut self, path: PathBuf) -> PathBuf {
        if !self.inside(&path) {
            // Spelled through a symlinked ancestor of the root (`/var` on
            // macOS): inside once resolved. Otherwise kept as spelled.
            if path.is_absolute()
                && let Ok(canonical) = dunce::canonicalize(&path)
                && canonical.starts_with(&self.canonical_root)
            {
                return canonical;
            }
            return path;
        }
        if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink())
            && let Ok(canonical) = dunce::canonicalize(&path)
        {
            return canonical;
        }
        let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
            return path;
        };
        let name = name.to_owned();
        let (root, canonical_root) = (&self.root, &self.canonical_root);
        let dir = self.dirs.entry(dir.to_owned()).or_insert_with(|| {
            dunce::canonicalize(dir).unwrap_or_else(|_| match dir.strip_prefix(root) {
                Ok(rest) => canonical_root.join(rest),
                Err(_) => dir.to_owned(),
            })
        });
        dir.join(name)
    }
}

/// The report with its paths resolved and its records merged by path.
fn build(raw: RawReport, report_dir: &Path, root: &Path) -> CoverageReport {
    let mut resolver = Resolver::new(root);
    let root = resolver.root.clone();
    let report_dir = normalize(&root.join(report_dir));
    let mut bases: Vec<PathBuf> = (raw.sources.iter())
        .map(|source| {
            let source = path_from_bytes(source.as_bytes());
            if is_foreign_absolute(source.as_os_str().as_encoded_bytes()) {
                source
            } else {
                normalize(&root.join(source))
            }
        })
        .collect();
    bases.push(root.clone());
    bases.push(report_dir);
    let mut unique = HashSet::new();
    bases.retain(|base| unique.insert(base.clone()));
    let mut merged: HashMap<PathBuf, RawFile> = HashMap::new();
    for (name, file) in raw.files {
        let path = resolver.resolve(&name, &bases);
        merged.entry(path).or_default().merge(file);
    }
    let files: HashMap<PathBuf, FileCoverage> = (merged.into_iter())
        .map(|(path, file)| (path, file.into_coverage()))
        .collect();
    let folded = case_insensitive(&resolver.canonical_root).then(|| {
        (files.keys())
            .map(|path| (fold(path), path.clone()))
            .collect()
    });
    CoverageReport {
        files,
        modified: None,
        folded,
    }
}

fn trim_bytes(mut bytes: &[u8]) -> &[u8] {
    while let [first, rest @ ..] = bytes
        && first.is_ascii_whitespace()
    {
        bytes = rest;
    }
    while let [rest @ .., last] = bytes
        && last.is_ascii_whitespace()
    {
        bytes = rest;
    }
    bytes
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    use std::os::unix::ffi::OsStrExt as _;
    PathBuf::from(std::ffi::OsStr::from_bytes(bytes))
}

#[cfg(not(unix))]
fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(bytes).into_owned())
}

/// Whether `raw` is a Windows path that's absolute (`C:\x`, `C:/x`,
/// `\\server\share`), on a system where std doesn't know it is: such a
/// path is another machine's, kept as it is and never joined to a base.
pub(crate) fn is_foreign_absolute(raw: &[u8]) -> bool {
    if cfg!(windows) {
        return false;
    }
    match raw {
        [drive, b':', b'\\' | b'/', ..] => drive.is_ascii_alphabetic(),
        [b'\\', b'\\', ..] => true,
        _ => false,
    }
}

/// `path` with `.` and `..` taken out, as written: nothing read.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                } else if !out.has_root() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// `path` spelled the way a case-folding filesystem compares it.
fn fold(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// Whether the filesystem holding `root` folds case: on macOS, asked by
/// looking `root` up spelled in the other case.
#[cfg(target_os = "macos")]
fn case_insensitive(root: &Path) -> bool {
    use std::os::unix::fs::MetadataExt as _;
    let spelled = root.to_string_lossy();
    let flipped: String = (spelled.chars())
        .map(|c| {
            if c.is_ascii_lowercase() {
                c.to_ascii_uppercase()
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    if flipped == spelled {
        return false;
    }
    match (fs::metadata(root), fs::metadata(&flipped)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}

/// Windows' filesystems fold case (NTFS's per-folder case sensitivity
/// aside).
#[cfg(windows)]
fn case_insensitive(_root: &Path) -> bool {
    true
}

/// Linux's filesystems don't fold case.
#[cfg(not(any(target_os = "macos", windows)))]
fn case_insensitive(_root: &Path) -> bool {
    false
}

/// The places a report is looked for, by folder in the order searched,
/// and each folder's report names in order: the first folder holding any
/// gives its newest.
const SEARCH: &[(&str, &[&str])] = &[
    ("", &["lcov.info", "coverage.xml"]),
    (
        "coverage",
        &["lcov.info", "cobertura-coverage.xml", "coverage.xml"],
    ),
    ("target/llvm-cov", &["lcov.info"]),
    ("target/llvm-cov-target", &["lcov.info"]),
    ("target/coverage", &["lcov.info"]),
];

/// coverage.py's database, which isn't a report.
const COVERAGE_DB: &str = ".coverage";

/// The report for the project at `root`: `coverage.report`'s (`setting`)
/// when set, else the newest in the first folder searched that has one.
pub fn find_report(root: &Path, setting: Option<&Path>) -> Option<PathBuf> {
    discover(root, setting).ok()
}

/// [`find_report`], saying why when there's no report.
pub fn discover(root: &Path, setting: Option<&Path>) -> Result<PathBuf, CoverageError> {
    discover_limited(root, setting, MAX_GLOB_MATCHES)
}

/// [`discover`], a glob matching at most `glob_limit` paths.
pub(crate) fn discover_limited(
    root: &Path,
    setting: Option<&Path>,
    glob_limit: usize,
) -> Result<PathBuf, CoverageError> {
    let root = normalize(root);
    let canonical_root = dunce::canonicalize(&root).unwrap_or_else(|_| root.clone());
    if let Some(setting) = setting {
        return from_setting(&root, &canonical_root, setting, glob_limit);
    }
    for (folder, names) in SEARCH {
        let dir = if folder.is_empty() {
            root.clone()
        } else {
            root.join(folder)
        };
        let found = (names.iter())
            .filter_map(|name| report_file(&dir.join(name), &canonical_root))
            .reduce(|newest, other| if other.1 > newest.1 { other } else { newest });
        if let Some((path, _)) = found {
            return Ok(path);
        }
    }
    if report_file(&root.join(COVERAGE_DB), &canonical_root).is_some() {
        return Err(CoverageError::CoverageDatabase);
    }
    Err(CoverageError::NoReport)
}

/// `path` and its modification time when it's a file inside the project,
/// symlinks resolved.
fn report_file(path: &Path, canonical_root: &Path) -> Option<(PathBuf, SystemTime)> {
    let canonical = dunce::canonicalize(path).ok()?;
    if !canonical.starts_with(canonical_root) {
        return None;
    }
    let meta = fs::metadata(&canonical).ok()?;
    if !meta.is_file() {
        return None;
    }
    Some((
        path.to_owned(),
        meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    ))
}

fn has_glob(text: &str) -> bool {
    text.contains(['*', '?', '['])
}

/// `coverage.report`: a path, relative to the root, or a glob whose
/// wildcards match within one folder each (`**` is `*`), the newest match
/// winning. Only folders the pattern names are listed.
fn from_setting(
    root: &Path,
    canonical_root: &Path,
    setting: &Path,
    glob_limit: usize,
) -> Result<PathBuf, CoverageError> {
    let shown = setting.display().to_string();
    let joined = normalize(&root.join(setting));
    let base = [root, canonical_root]
        .into_iter()
        .find(|base| joined.starts_with(base));
    let Some(base) = base else {
        return Err(CoverageError::SettingOutsideRoot { setting: shown });
    };
    let rest = joined
        .strip_prefix(base)
        .unwrap_or(Path::new(""))
        .to_owned();
    let no_match = || CoverageError::SettingNoMatch {
        setting: shown.clone(),
    };
    if !has_glob(&rest.to_string_lossy()) {
        return report_file(&joined, canonical_root)
            .map(|(path, _)| path)
            .ok_or_else(no_match);
    }
    let mut paths = vec![base.to_owned()];
    for component in rest.components() {
        let part = component.as_os_str().to_string_lossy();
        if !has_glob(&part) {
            paths.iter_mut().for_each(|path| path.push(component));
            continue;
        }
        let pattern: Vec<char> = part.replace("**", "*").chars().collect();
        let mut next = Vec::new();
        for dir in &paths {
            // A folder linked from outside the project isn't listed.
            let inside = dunce::canonicalize(dir).is_ok_and(|dir| dir.starts_with(canonical_root));
            let Some(entries) = inside.then(|| fs::read_dir(dir).ok()).flatten() else {
                continue;
            };
            // Every entry is looked at; only matches are kept, and too
            // many fail the search rather than drop some silently.
            for entry in entries.flatten() {
                let name: Vec<char> = entry.file_name().to_string_lossy().chars().collect();
                if glob_match(&pattern, &name) {
                    if next.len() >= glob_limit {
                        return Err(CoverageError::SettingTooBroad {
                            setting: shown,
                            limit: glob_limit,
                        });
                    }
                    next.push(dir.join(entry.file_name()));
                }
            }
        }
        next.sort();
        paths = next;
    }
    (paths.iter())
        .filter_map(|path| report_file(path, canonical_root))
        .reduce(|newest, other| if other.1 > newest.1 { other } else { newest })
        .map(|(path, _)| path)
        .ok_or_else(no_match)
}

/// Whether `name` matches `pattern`: `*` any run, `?` any one character,
/// `[abc]`, `[a-z]` and `[!a]` (or `[^a]`) a class; anything else itself.
pub(crate) fn glob_match(pattern: &[char], name: &[char]) -> bool {
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        let step = match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
                continue;
            }
            Some('?') => Some(p + 1),
            Some('[') => match class(pattern, p, name[n]) {
                Some((true, end)) => Some(end),
                Some((false, _)) => None,
                None => (name[n] == '[').then_some(p + 1),
            },
            Some(&c) => (c == name[n]).then_some(p + 1),
            None => None,
        };
        match (step, star) {
            (Some(next), _) => {
                p = next;
                n += 1;
            }
            (None, Some((star_p, star_n))) => {
                p = star_p + 1;
                n = star_n + 1;
                star = Some((star_p, star_n + 1));
            }
            (None, None) => return false,
        }
    }
    pattern[p.min(pattern.len())..].iter().all(|&c| c == '*')
}

/// The class at `pattern[start]` (`[`): whether `c` is in it, and where it
/// ends; `None` without a closing `]`.
fn class(pattern: &[char], start: usize, c: char) -> Option<(bool, usize)> {
    let mut at = start + 1;
    let negated = matches!(pattern.get(at), Some('!' | '^'));
    if negated {
        at += 1;
    }
    let mut matched = false;
    let mut first = true;
    loop {
        let &lo = pattern.get(at)?;
        if lo == ']' && !first {
            return Some((matched != negated, at + 1));
        }
        first = false;
        if pattern.get(at + 1) == Some(&'-') && pattern.get(at + 2).is_some_and(|&hi| hi != ']') {
            let hi = pattern[at + 2];
            matched |= lo <= c && c <= hi;
            at += 3;
        } else {
            matched |= lo == c;
            at += 1;
        }
    }
}

/// [`CoverageSource::watch_places`]: folders with the pattern of what
/// under each could be a report, and the reports' file names.
#[derive(Default)]
struct WatchPlaces {
    places: Vec<(PathBuf, Vec<String>)>,
    names: Vec<Vec<char>>,
    roots: [PathBuf; 2],
}

impl WatchPlaces {
    /// Whether a change at `path` could add, remove or reorder a report: a
    /// path in a place searched or a folder on the way to one (a folder of
    /// reports moved in whole), or, anywhere in the project, a file named
    /// as a report is, which may sit in a folder linked from a searched
    /// one.
    fn could_be_report(&self, path: &Path) -> bool {
        let in_place = (self.places.iter()).any(|(base, pattern)| {
            let Ok(relative) = path.strip_prefix(base) else {
                return false;
            };
            let relative: Vec<_> = relative.components().collect();
            relative.len() <= pattern.len()
                && (relative.iter().zip(pattern)).all(|(part, glob)| {
                    let glob: Vec<char> = glob.chars().collect();
                    let part: Vec<char> = part.as_os_str().to_string_lossy().chars().collect();
                    glob_match(&glob, &part)
                })
        });
        if in_place {
            return true;
        }
        let inside = self.roots.iter().any(|root| path.starts_with(root));
        let name: Option<Vec<char>> =
            (path.file_name()).map(|name| name.to_string_lossy().chars().collect());
        inside && name.is_some_and(|name| self.names.iter().any(|glob| glob_match(glob, &name)))
    }
}

/// A report file's modification time and length, compared to tell it
/// changed.
type Stamp = (SystemTime, u64);

struct Loaded {
    result: Result<CoverageReport, CoverageError>,
    #[cfg(test)]
    thread: thread::ThreadId,
}

/// The project's report, followed as it changes.
///
/// [`CoverageSource::poll`] is the one entry: with no live file watcher it
/// stats the report every [`POLL_INTERVAL`] and looks for a newer one every
/// [`DISCOVER_INTERVAL`]; with one ([`CoverageSource::set_watching`]) it
/// does nothing until [`CoverageSource::fs_changed`] hears of the report or
/// a file that could be one, as `disk_sync.rs` waits on the watcher's
/// batches instead of polling. A changed `(mtime, len)` parses the report
/// again on a background thread; a failed parse keeps the report there was
/// and holds the error for the UI.
pub struct CoverageSource {
    root: PathBuf,
    /// `root` with its symlinks resolved, as watchers may name paths.
    canonical_root: PathBuf,
    setting: Option<PathBuf>,
    report: Option<Arc<CoverageReport>>,
    error: Option<CoverageError>,
    /// The report file followed, as found, and canonical.
    path: Option<(PathBuf, PathBuf)>,
    /// The followed file's stamp as last loaded, or being loaded.
    stamp: Option<Stamp>,
    loading: Option<mpsc::Receiver<Loaded>>,
    watching: bool,
    /// A check is owed: the watcher named the report, or it just started.
    dirty: bool,
    /// A look for a newer report is owed.
    rediscover: bool,
    next_check: Option<Instant>,
    next_discover: Option<Instant>,
    #[cfg(test)]
    pub(crate) test: SourceTest,
}

/// What the tests count.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct SourceTest {
    /// Stats of the followed report.
    pub(crate) stats: usize,
    /// Looks for a report.
    pub(crate) discoveries: usize,
    /// Loads started.
    pub(crate) loads: usize,
    /// The thread the last load ran on.
    pub(crate) loaded_on: Option<thread::ThreadId>,
}

impl CoverageSource {
    /// Follows the report of the project at `root`, `coverage.report`
    /// being `setting`. Only the root is resolved until the first poll.
    pub fn new(root: impl Into<PathBuf>, setting: Option<PathBuf>) -> Self {
        let root = normalize(&root.into());
        let canonical_root = dunce::canonicalize(&root).unwrap_or_else(|_| root.clone());
        Self {
            root,
            canonical_root,
            setting,
            report: None,
            error: None,
            path: None,
            stamp: None,
            loading: None,
            watching: false,
            dirty: true,
            rediscover: true,
            next_check: None,
            next_discover: None,
            #[cfg(test)]
            test: SourceTest::default(),
        }
    }

    /// The report loaded last, if any loaded.
    pub fn report(&self) -> Option<&Arc<CoverageReport>> {
        self.report.as_ref()
    }

    /// Why the last load, or the look for a report, failed; cleared by a
    /// load that succeeds.
    pub fn error(&self) -> Option<&CoverageError> {
        self.error.as_ref()
    }

    /// The report file followed.
    pub fn report_path(&self) -> Option<&Path> {
        self.path.as_ref().map(|(path, _)| path.as_path())
    }

    /// Whether a load is running.
    pub fn is_loading(&self) -> bool {
        self.loading.is_some()
    }

    /// The file at `path`'s coverage, stale when the report is older than
    /// `file_modified`.
    pub fn file_state(
        &self,
        path: &Path,
        file_modified: Option<SystemTime>,
    ) -> Option<FileState<'_>> {
        self.report.as_ref()?.file_state(path, file_modified)
    }

    /// `coverage.report` changed: the report is looked for again.
    pub fn set_setting(&mut self, setting: Option<PathBuf>) {
        if self.setting == setting {
            return;
        }
        self.setting = setting;
        // A load still running is the old setting's report: dropped, its
        // answer never landed.
        self.loading = None;
        self.path = None;
        self.stamp = None;
        self.report = None;
        self.error = None;
        self.dirty = true;
        self.rediscover = true;
    }

    /// Whether a file watcher covering the project is live: while it is,
    /// polls only check what [`Self::fs_changed`] heard of.
    pub fn set_watching(&mut self, live: bool) {
        if self.watching && !live {
            self.dirty = true;
            self.rediscover = true;
        }
        self.watching = live;
    }

    /// A watcher batch: the report changing owes a check, and a file that
    /// could be a report a look for a newer one.
    pub fn fs_changed(&mut self, changes: &[FsChange]) {
        let mut places = None;
        for change in changes {
            match change {
                FsChange::Rescan => {
                    self.dirty = true;
                    self.rediscover = true;
                }
                FsChange::Path(path) => {
                    let followed = (self.path.as_ref())
                        .is_some_and(|(found, canonical)| path == found || path == canonical);
                    // The followed report changing can change which is
                    // newest, so it's looked for again too.
                    let places = places.get_or_insert_with(|| self.watch_places());
                    if followed || places.could_be_report(path) {
                        self.dirty = true;
                        self.rediscover = true;
                    }
                }
            }
        }
    }

    /// Where a report could appear, spelled as the search spells it and
    /// as a watcher may name it, through the root's and the searched
    /// folders' symlinks.
    fn watch_places(&self) -> WatchPlaces {
        let (root, canonical_root) = (&self.root, &self.canonical_root);
        let split = |path: &Path| -> Vec<String> {
            (path.components())
                .map(|part| part.as_os_str().to_string_lossy().replace("**", "*"))
                .collect()
        };
        let patterns: Vec<Vec<String>> = match &self.setting {
            None => (SEARCH.iter())
                .flat_map(|(folder, names)| {
                    names.iter().map(move |name| Path::new(folder).join(name))
                })
                .chain([PathBuf::from(COVERAGE_DB)])
                .map(|path| split(&path))
                .collect(),
            Some(setting) => {
                let joined = normalize(&root.join(setting));
                ([root, canonical_root].into_iter())
                    .find_map(|base| joined.strip_prefix(base).ok())
                    .map(split)
                    .into_iter()
                    .collect()
            }
        };
        let mut canonical_dirs: HashMap<PathBuf, Option<PathBuf>> = HashMap::new();
        let mut places = WatchPlaces::default();
        for pattern in patterns {
            places.places.push((root.clone(), pattern.clone()));
            places
                .places
                .push((canonical_root.clone(), pattern.clone()));
            // Each folder on the way, up to the first wildcard, as its
            // symlinks resolve.
            let mut dir = root.clone();
            for (at, part) in pattern
                .iter()
                .enumerate()
                .take(pattern.len().saturating_sub(1))
            {
                if has_glob(part) {
                    break;
                }
                dir.push(part);
                let canonical = (canonical_dirs.entry(dir.clone()))
                    .or_insert_with(|| dunce::canonicalize(&dir).ok())
                    .clone();
                if let Some(canonical) = canonical
                    && canonical != canonical_root.join(dir.strip_prefix(root).unwrap_or(&dir))
                {
                    places.places.push((canonical, pattern[at + 1..].to_vec()));
                }
            }
            if let Some(name) = pattern.last() {
                places.names.push(name.chars().collect());
            }
        }
        places.roots = [root.clone(), canonical_root.clone()];
        places
    }

    /// Lands a finished load, and checks the report when a check is due
    /// at `now`. True when the report or the error changed.
    pub fn poll(&mut self, now: Instant) -> bool {
        let mut changed = match self.loading.as_ref().map(mpsc::Receiver::try_recv) {
            Some(Ok(loaded)) => self.landed(Some(loaded)),
            Some(Err(mpsc::TryRecvError::Disconnected)) => self.landed(None),
            Some(Err(mpsc::TryRecvError::Empty)) => return false,
            None => false,
        };
        if !self.watching {
            if self.next_check.is_none_or(|at| now >= at) {
                self.dirty = true;
            }
            if self.next_discover.is_none_or(|at| now >= at) {
                self.rediscover = true;
            }
        }
        if !self.dirty && !self.rediscover {
            return changed;
        }
        if self.rediscover || self.path.is_none() {
            self.rediscover = false;
            self.next_discover = Some(now + DISCOVER_INTERVAL);
            changed |= self.discover();
        }
        self.dirty = false;
        self.next_check = Some(now + POLL_INTERVAL);
        let Some((path, _)) = self.path.clone() else {
            return changed;
        };
        #[cfg(test)]
        {
            self.test.stats += 1;
        }
        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => {
                // Gone: what it said goes with it, and another is looked for.
                self.path = None;
                self.stamp = None;
                self.rediscover = true;
                changed |= self.report.take().is_some();
                return changed;
            }
        };
        let stamp = (
            meta.modified().unwrap_or(SystemTime::UNIX_EPOCH),
            meta.len(),
        );
        if self.stamp == Some(stamp) {
            return changed;
        }
        self.stamp = Some(stamp);
        self.load(path);
        changed
    }

    /// Looks for the report; true when the error changed.
    fn discover(&mut self) -> bool {
        #[cfg(test)]
        {
            self.test.discoveries += 1;
        }
        match discover(&self.root, self.setting.as_deref()) {
            Ok(path) => {
                if self
                    .path
                    .as_ref()
                    .is_none_or(|(followed, _)| *followed != path)
                {
                    let canonical = dunce::canonicalize(&path).unwrap_or_else(|_| path.clone());
                    self.path = Some((path, canonical));
                    self.stamp = None;
                }
                false
            }
            Err(error) => {
                let changed = self.error.as_ref() != Some(&error) || self.report.is_some();
                self.path = None;
                self.stamp = None;
                self.report = None;
                self.error = Some(error);
                changed
            }
        }
    }

    fn load(&mut self, path: PathBuf) {
        #[cfg(test)]
        {
            self.test.loads += 1;
        }
        let (sender, receiver) = mpsc::channel();
        let root = self.root.clone();
        let spawned = thread::Builder::new()
            .name("coverage-report".into())
            .spawn(move || {
                let result = load_report(&path, &root);
                sender
                    .send(Loaded {
                        result,
                        #[cfg(test)]
                        thread: thread::current().id(),
                    })
                    .ok();
            });
        match spawned {
            Ok(_) => self.loading = Some(receiver),
            Err(_) => self.error = Some(CoverageError::LoadFailed),
        }
    }

    /// A load's answer, or `None` when it ended without one.
    fn landed(&mut self, loaded: Option<Loaded>) -> bool {
        self.loading = None;
        let Some(loaded) = loaded else {
            self.error = Some(CoverageError::LoadFailed);
            return true;
        };
        #[cfg(test)]
        {
            self.test.loaded_on = Some(loaded.thread);
        }
        match loaded.result {
            Ok(report) => {
                self.report = Some(Arc::new(report));
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
        true
    }

    /// Waits for the running load, if any, and lands it.
    #[cfg(test)]
    pub(crate) fn wait_for_load(&mut self) -> bool {
        let Some(receiver) = &self.loading else {
            return false;
        };
        let loaded = receiver.recv_timeout(Duration::from_secs(30)).ok();
        self.landed(loaded)
    }
}
