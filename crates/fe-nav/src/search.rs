//! [`SearchNavigation`]: go-to-definition and workspace symbols by name,
//! from a [`SymbolIndex`], ready before any language server is.

use std::{
    borrow::Cow,
    cell::Cell,
    collections::HashSet,
    fmt,
    ops::Range,
    path::{Path, PathBuf},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
    },
    thread,
    time::{Duration, Instant},
};

use fe_buffer::{Bias, Buffer, Text};
use fe_project::{SearchOptions, SearchQuery, search};
use fe_syntax::{Outline, OutlineJob, OutlineKind, Syntax, language_by_name, symbols_job};

use crate::{
    Definition, Definitions, FileReferences, FileSymbols, Location, NavError, NavRequest,
    NavigationProvider, Precision, Readiness, ReferenceOptions, ReferencesSummary, Skipped,
    SymbolIndex, Tier, WorkspaceSymbol,
    extract::{extract_with, is_symbol, symbols_language},
    references,
};

/// The most definitions in an answer.
const MAX_ITEMS: usize = 50;

/// The most files the fallback extracts.
const FALLBACK_FILES: usize = 64;

/// The most matches the fallback's search finds.
const FALLBACK_MATCHES: usize = 2_000;

/// How long a request may work before it answers with what it has: its
/// own text's parse and definitions, and the fallback's extraction, all
/// stop this long after it started, to answer within 100 ms.
const BUDGET: Duration = Duration::from_millis(80);

/// How often a request checks its cancel flag and its deadline while
/// waiting for a background thread.
const POLL: Duration = Duration::from_millis(5);

/// The most fallback threads running at once. A request that finds them
/// all busy (say, walks blocked on an ignore file that is a FIFO, a known
/// limit of [`fe_project::search`]) answers from the index alone.
const FALLBACK_THREADS: usize = 4;

/// The most threads finding the request text's definitions at once. A
/// request that finds them all busy answers without same-file definitions.
const SYMBOLS_THREADS: usize = 2;

/// In a buffer too large to parse, how much of its start is read for
/// imports.
const IMPORT_SCAN_BYTES: usize = 1 << 20;

/// The longest an unclosed import statement is followed for.
const MAX_IMPORT_LINES: usize = 200;

/// How many lines or definitions a request works through between looks at
/// its cancel flag.
const CANCEL_EVERY: usize = 1024;

/// Reads one file's definitions, giving up once its third argument says
/// so: [`extract_with`], except in tests.
type Extract =
    dyn Fn(&Path, &Path, &dyn Fn() -> bool) -> Result<FileSymbols, Skipped> + Send + Sync;

/// Runs a definitions query: [`OutlineJob::run`], except in tests.
type RunSymbols = dyn Fn(OutlineJob) -> Outline + Send + Sync;

/// A step of a request that a test can act at, as by cancelling it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Go-to-definition is about to read the request text's imports.
    Imports,
    /// Go-to-definition is about to rank what it found.
    Rank,
    /// Go-to-definition has ranked what it found, and is about to answer.
    Ranked,
    /// Workspace symbols is about to match the query.
    Fuzzy,
}

/// Navigation by name: the definitions named like the word at the caret,
/// ranked by how close they are to the request, from a [`SymbolIndex`]
/// shared with the build that fills it.
///
/// It's [`Readiness::Ready`] for every language, and degrades rather than
/// waits: before the index is complete, go-to-definition searches the
/// project's text for the word (capped in files and time) and extracts the
/// definitions of the files that have it. Each request has a budget, 80 ms
/// by default; work it hasn't finished by then is left out of the answer.
///
/// Find references is a word search of the project, filtered by syntax
/// (see [`ReferenceOptions`]); it has no deadline, and streams instead.
pub struct SearchNavigation {
    root: PathBuf,
    index: Arc<RwLock<SymbolIndex>>,
    budget: Duration,
    reference_options: ReferenceOptions,
    extract: Arc<Extract>,
    run_symbols: Arc<RunSymbols>,
    fallback_slots: Arc<Slots>,
    symbols_slots: Arc<Slots>,
    searches: AtomicUsize,
    files_extracted: Arc<AtomicUsize>,
    #[cfg(test)]
    on_phase: Arc<dyn Fn(Phase) + Send + Sync>,
}

/// What a [`SearchNavigation`]'s fallback has done, since it was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Project text searches started, one per request answered before the
    /// index was complete with time and a thread left for one.
    pub searches: usize,
    /// Files those searches led it to extract (or try to).
    pub files_extracted: usize,
}

impl SearchNavigation {
    pub const NAME: &'static str = "search";

    /// Answers for the project at `root`, from `index`, which a build may
    /// be extending.
    pub fn new(root: impl Into<PathBuf>, index: Arc<RwLock<SymbolIndex>>) -> Self {
        Self {
            root: root.into(),
            index,
            budget: BUDGET,
            reference_options: ReferenceOptions::default(),
            extract: Arc::new(|root, rel, give_up| {
                extract_with(root, rel, |syntax| {
                    syntax.parse_until(Duration::MAX, give_up);
                })
            }),
            run_symbols: Arc::new(OutlineJob::run),
            fallback_slots: Slots::new(FALLBACK_THREADS),
            symbols_slots: Slots::new(SYMBOLS_THREADS),
            searches: AtomicUsize::new(0),
            files_extracted: Arc::new(AtomicUsize::new(0)),
            #[cfg(test)]
            on_phase: Arc::new(|_| {}),
        }
    }

    /// How long a request may work before it answers with what it has:
    /// 80 ms unless set otherwise, as a test does to count files without
    /// racing the clock.
    pub fn with_budget(mut self, budget: Duration) -> Self {
        self.budget = budget;
        self
    }

    /// How find references works: 1,000 files verified per request unless
    /// set otherwise.
    pub fn with_reference_options(mut self, options: ReferenceOptions) -> Self {
        self.reference_options = options;
        self
    }

    /// What the fallback has done. Files are extracted on a thread of the
    /// fallback's own, which a request stops waiting for at its deadline,
    /// so the count can still grow by the extraction in flight then.
    pub fn stats(&self) -> SearchStats {
        SearchStats {
            searches: self.searches.load(Ordering::Relaxed),
            files_extracted: self.files_extracted.load(Ordering::Relaxed),
        }
    }

    /// Tells a test that a request is at `phase`.
    fn reached(&self, _phase: Phase) {
        #[cfg(test)]
        (self.on_phase)(_phase);
    }

    fn index(&self) -> std::sync::RwLockReadGuard<'_, SymbolIndex> {
        self.index.read().unwrap_or_else(|err| err.into_inner())
    }

    /// The definitions in the request's own text, found by `job` on a
    /// thread of their own, which this waits for only until `deadline` or
    /// a cancel. `None` if they didn't come in time or no thread was free.
    fn same_file_symbols(
        &self,
        job: OutlineJob,
        deadline: Instant,
        cancel: &AtomicBool,
    ) -> Result<Option<Outline>, NavError> {
        let Some(slot) = self.symbols_slots.take() else {
            return Ok(None);
        };
        let run = self.run_symbols.clone();
        let (tx, rx) = mpsc::channel();
        let spawned = thread::Builder::new()
            .name("fe-nav symbols".to_owned())
            .spawn(move || {
                let _slot = slot;
                let _ = tx.send(run(job));
            });
        if spawned.is_err() {
            return Ok(None);
        }
        let mut outline = None;
        wait(&rx, deadline, cancel, |found| outline = Some(found))?;
        Ok(outline)
    }

    /// Adds to `found` the definitions of `word` in files that contain it
    /// as a whole word, which an incomplete index may not have yet: the
    /// project's text is searched, and the first [`FALLBACK_FILES`] files
    /// found whose language has symbols are extracted, until `deadline`. A
    /// definition already in `found`, by file and name range, isn't added
    /// again.
    ///
    /// The search and the extraction run on a [`Fallback`] thread, which
    /// this waits for only until the deadline or a cancel. Then it's told
    /// to stop, and left to finish on its own, so a slow extraction or a
    /// blocked walk doesn't hold up the answer. At most
    /// [`FALLBACK_THREADS`] run at once; with none free, there's no
    /// fallback.
    fn fallback(
        &self,
        word: &str,
        req: &NavRequest,
        deadline: Instant,
        cancel: &AtomicBool,
        found: &mut Vec<Definition>,
    ) -> Result<(), NavError> {
        if Instant::now() >= deadline {
            return Ok(());
        }
        let Ok(root) = std::fs::canonicalize(&self.root) else {
            return Ok(());
        };
        let Some(slot) = self.fallback_slots.take() else {
            return Ok(());
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let fallback = Fallback {
            root,
            word: word.to_owned(),
            skip: req.rel.clone(),
            deadline,
            stop: stop.clone(),
            extract: self.extract.clone(),
            extracted: self.files_extracted.clone(),
            tx,
            _slot: slot,
        };
        let spawned = thread::Builder::new()
            .name("fe-nav fallback".to_owned())
            .spawn(move || fallback.run());
        if spawned.is_err() {
            return Ok(());
        }
        self.searches.fetch_add(1, Ordering::Relaxed);
        let mut seen: HashSet<(PathBuf, Range<usize>)> = (found.iter())
            .map(|def| (def.location.rel.clone(), def.location.range.clone()))
            .collect();
        let mut merge = |defs: Vec<Definition>| {
            for def in defs {
                if seen.insert((def.location.rel.clone(), def.location.range.clone())) {
                    found.push(def);
                }
            }
        };
        let waited = wait(&rx, deadline, cancel, &mut merge);
        stop.store(true, Ordering::Relaxed);
        if waited.is_ok() {
            // Files extracted by the deadline, not yet taken.
            while let Ok(defs) = rx.try_recv() {
                merge(defs);
            }
        }
        waited
    }

    #[cfg(test)]
    fn with_extract(
        mut self,
        extract: impl Fn(&Path, &Path, &dyn Fn() -> bool) -> Result<FileSymbols, Skipped>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        self.extract = Arc::new(extract);
        self
    }

    #[cfg(test)]
    fn with_on_phase(mut self, on_phase: impl Fn(Phase) + Send + Sync + 'static) -> Self {
        self.on_phase = Arc::new(on_phase);
        self
    }

    #[cfg(test)]
    fn with_run_symbols(
        mut self,
        run: impl Fn(OutlineJob) -> Outline + Send + Sync + 'static,
    ) -> Self {
        self.run_symbols = Arc::new(run);
        self
    }
}

impl fmt::Debug for SearchNavigation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SearchNavigation")
            .field("root", &self.root)
            .field("budget", &self.budget)
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

/// Hands each value `rx` receives to `take` until its sender is gone,
/// `deadline` passes or `cancel` is set (then `Err(Cancelled)`).
fn wait<T>(
    rx: &Receiver<T>,
    deadline: Instant,
    cancel: &AtomicBool,
    mut take: impl FnMut(T),
) -> Result<(), NavError> {
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(NavError::Cancelled);
        }
        let now = Instant::now();
        if now >= deadline {
            return Ok(());
        }
        match rx.recv_timeout((deadline - now).min(POLL)) {
            Ok(value) => take(value),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

/// A fixed number of background threads that may run at once.
#[derive(Debug)]
struct Slots {
    busy: AtomicUsize,
    limit: usize,
}

impl Slots {
    fn new(limit: usize) -> Arc<Slots> {
        Arc::new(Slots {
            busy: AtomicUsize::new(0),
            limit,
        })
    }

    /// One of the slots, held until the [`Slot`] is dropped; `None` if
    /// they're all taken.
    fn take(self: &Arc<Self>) -> Option<Slot> {
        self.busy
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |busy| {
                (busy < self.limit).then_some(busy + 1)
            })
            .ok()?;
        Some(Slot(self.clone()))
    }
}

struct Slot(Arc<Slots>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.busy.fetch_sub(1, Ordering::AcqRel);
    }
}

/// A fallback's search and extraction, on a thread of its own, sending each
/// extracted file's definitions of the word as it goes.
///
/// It stops at the first of: the search's end, [`FALLBACK_FILES`]
/// extractions attempted, the deadline, and `stop` (set by the request once
/// it stops waiting). Each is checked before every file, the search checks
/// `stop` before each file and at each match, and an extraction's parse
/// gives up on either, so once the request has gone it ends soon, unless
/// the walk itself is blocked. It holds one of the [`FALLBACK_THREADS`]
/// slots until it ends.
struct Fallback {
    /// Canonical.
    root: PathBuf,
    word: String,
    /// The request's own file.
    skip: PathBuf,
    deadline: Instant,
    stop: Arc<AtomicBool>,
    extract: Arc<Extract>,
    extracted: Arc<AtomicUsize>,
    tx: Sender<Vec<Definition>>,
    _slot: Slot,
}

impl Fallback {
    fn run(self) {
        let query = SearchQuery {
            pattern: self.word.clone(),
            regex: false,
            case_sensitive: true,
            whole_word: true,
        };
        let options = SearchOptions {
            max_matches: FALLBACK_MATCHES,
            threads: 0,
        };
        let give_up = || self.stop.load(Ordering::Relaxed) || Instant::now() >= self.deadline;
        let mut files = 0;
        // The pattern is a literal, so it can't be invalid.
        let _ = search(&self.root, &query, &options, self.stop.clone(), |batch| {
            for file in batch {
                if self.stop.load(Ordering::Relaxed) {
                    return;
                }
                if files == FALLBACK_FILES || Instant::now() >= self.deadline {
                    self.stop.store(true, Ordering::Relaxed);
                    return;
                }
                // `file.rel` is lossy; the path keeps the name's bytes.
                let Ok(rel) = file.path.strip_prefix(&self.root) else {
                    continue;
                };
                // The request's own file is answered from its live text. A
                // file the index has is extracted too, and its symbols
                // deduplicated with the index's.
                if rel == self.skip || symbols_language(rel).is_none() {
                    continue;
                }
                files += 1;
                self.extracted.fetch_add(1, Ordering::Relaxed);
                if let Ok(extracted) = (self.extract)(&self.root, rel, &give_up) {
                    let defs: Vec<Definition> = (extracted.symbols.into_iter())
                        .filter(|symbol| symbol.name == self.word)
                        .map(|symbol| Definition {
                            location: Location {
                                rel: rel.to_path_buf(),
                                range: symbol.name_range,
                                row: symbol.row,
                            },
                            name: symbol.name,
                            kind: symbol.kind,
                            container: symbol.container,
                            tier: Tier::Project,
                        })
                        .collect();
                    if !defs.is_empty() && self.tx.send(defs).is_err() {
                        self.stop.store(true, Ordering::Relaxed);
                        return;
                    }
                }
                if files == FALLBACK_FILES {
                    self.stop.store(true, Ordering::Relaxed);
                    return;
                }
            }
        });
    }
}

impl NavigationProvider for SearchNavigation {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn precision(&self) -> Precision {
        Precision::Search
    }

    fn readiness(&self, _language: Option<&str>) -> Readiness {
        Readiness::Ready
    }

    fn definitions(&self, req: &NavRequest, cancel: &AtomicBool) -> Result<Definitions, NavError> {
        let started = Instant::now();
        let deadline = started + self.budget;
        let check = || {
            if cancel.load(Ordering::Relaxed) {
                Err(NavError::Cancelled)
            } else {
                Ok(())
            }
        };
        check()?;
        let Some((word_range, word)) = word_at(&req.text, req.offset, req.language) else {
            return Ok(Definitions::default());
        };
        // Read up front, so every answer says whether the index was complete.
        let complete = self.index().is_complete();

        // The request's own text, parsed once, when its language allows and
        // the parse ends by the deadline.
        let language = (req.language)
            .and_then(language_by_name)
            .filter(|language| req.text.len() <= language.parse_limit());
        let source = language.map(|_| req.text.to_string());
        let syntax = match (language, &source) {
            (Some(language), Some(source)) if Instant::now() < deadline => {
                let mut syntax = Syntax::new(language, Buffer::new(source).current());
                let parsed = syntax.parse_until(deadline - started, || {
                    cancel.load(Ordering::Relaxed) || Instant::now() >= deadline
                });
                check()?;
                parsed.then_some(syntax)
            }
            _ => None,
        };
        let mut hint = None;
        let mut found: Vec<Definition> = Vec::new();
        if let (Some(syntax), Some(source)) = (&syntax, &source) {
            let spans = syntax.highlights(word_range.clone());
            if in_comment_or_string(&spans, &word_range) {
                check()?;
                return Ok(Definitions {
                    word: Some(word),
                    items: Vec::new(),
                    unambiguous: false,
                    partial: !complete,
                });
            }
            let called = source[word_range.end..].starts_with('(');
            hint = (spans.iter())
                .find(|span| span.range.contains(&word_range.start))
                .and_then(|span| KindHint::of(span.highlight.name(), called));
            let outline = match symbols_job(syntax) {
                Ok(job) => self.same_file_symbols(job, deadline, cancel)?,
                Err(_) => None,
            };
            if let Some(outline) = outline {
                let items = &outline.items;
                for item in items {
                    if item.name != word || !is_symbol(item.kind) {
                        continue;
                    }
                    let definition = Definition {
                        location: Location {
                            rel: req.rel.clone(),
                            range: item.name_range.clone(),
                            row: item.row,
                        },
                        name: item.name.clone(),
                        kind: item.kind,
                        container: item
                            .parent
                            .map(|parent| items[parent as usize].name.clone()),
                        tier: Tier::SameFile,
                    };
                    let on_name =
                        item.name_range.start <= req.offset && req.offset <= item.name_range.end;
                    if on_name {
                        check()?;
                        return Ok(Definitions {
                            word: Some(word),
                            items: vec![definition],
                            unambiguous: true,
                            partial: !complete,
                        });
                    }
                    found.push(definition);
                }
            }
        }
        check()?;

        {
            let index = self.index();
            for (step, symbol) in index.definitions(&word).into_iter().enumerate() {
                check_every(step, cancel)?;
                if symbol.rel == req.rel {
                    continue;
                }
                found.push(Definition {
                    location: Location {
                        rel: symbol.rel.to_path_buf(),
                        range: symbol.name_range,
                        row: symbol.row,
                    },
                    name: symbol.name.to_owned(),
                    kind: symbol.kind,
                    container: symbol.container.map(str::to_owned),
                    tier: Tier::Project,
                });
            }
        }
        // A complete index is authoritative.
        if !complete {
            self.fallback(&word, req, deadline, cancel, &mut found)?;
        }
        check()?;

        // Imports are read whether or not the text could be parsed; from the
        // start of a buffer too large to parse.
        let import_source: Option<Cow<'_, str>> = match (&source, req.language) {
            (_, None) => None,
            (Some(source), _) => Some(Cow::Borrowed(source)),
            (None, Some(_)) => {
                let end = (req.text).clip_offset(IMPORT_SCAN_BYTES, Bias::Left);
                Some(Cow::Owned(req.text.slice_to_string(0..end)))
            }
        };
        self.reached(Phase::Imports);
        let imports = match (req.language, &import_source) {
            (Some(language), Some(source)) => import_lines(language, source, cancel)?,
            _ => Vec::new(),
        };
        let poll = Poll::new(cancel);
        for definition in found.iter_mut() {
            poll.check(Work::Definition)?;
            if definition.tier != Tier::Project {
                continue;
            }
            let Some(module) = module_name(&definition.location.rel) else {
                continue;
            };
            for line in &imports {
                poll.check(Work::Compare)?;
                if has_word(line, module) {
                    definition.tier = Tier::Imported;
                    break;
                }
            }
        }
        self.reached(Phase::Rank);
        check()?;
        rank(&mut found, &req.rel, hint);
        found.truncate(MAX_ITEMS);
        let unambiguous = match &found[..] {
            [_] => true,
            [first, second, ..] => first.tier < second.tier,
            [] => false,
        };
        self.reached(Phase::Ranked);
        check()?;
        Ok(Definitions {
            word: Some(word),
            items: found,
            unambiguous,
            partial: !complete,
        })
    }

    fn references(
        &self,
        req: &NavRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<ReferencesSummary, NavError> {
        references::find(&self.root, &self.reference_options, req, cancel, sink)
    }

    fn workspace_symbols(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(NavError::Cancelled);
        }
        self.reached(Phase::Fuzzy);
        let index = self.index();
        // The match itself can't be cancelled: `SymbolIndex::fuzzy` takes
        // no flag. Its answer is dropped if a cancel came meanwhile.
        let symbols: Vec<WorkspaceSymbol> = (index.fuzzy(query, limit).into_iter())
            .map(|found| WorkspaceSymbol {
                location: Location {
                    rel: found.symbol.rel.to_path_buf(),
                    range: found.symbol.name_range,
                    row: found.symbol.row,
                },
                name: found.symbol.name.to_owned(),
                kind: found.symbol.kind,
                container: found.symbol.container.map(str::to_owned),
                indices: found.indices,
            })
            .collect();
        if cancel.load(Ordering::Relaxed) {
            return Err(NavError::Cancelled);
        }
        Ok(symbols)
    }
}

/// The word at `offset` in `text`, and its bytes: the longest run of
/// identifier chars touching it, so a caret just after a word counts.
pub(crate) fn word_at(
    text: &Text,
    offset: usize,
    language: Option<&str>,
) -> Option<(Range<usize>, String)> {
    let dollar = matches!(language, Some("javascript" | "typescript" | "tsx"));
    // The search's word chars, so the word is one it finds whole.
    let is_word = |ch: char| fe_project::is_word_char(ch) || (dollar && ch == '$');
    let offset = text.clip_offset(offset, Bias::Left);
    let row = text.offset_to_point(offset).row;
    let row_start = text.row_start(row);
    let line = text.row_to_string(row);
    let column = offset - row_start;
    let start = (line[..column].char_indices().rev())
        .take_while(|&(_, ch)| is_word(ch))
        .last()
        .map_or(column, |(at, _)| at);
    let end = (line[column..].char_indices())
        .find(|&(_, ch)| !is_word(ch))
        .map_or(line.len(), |(at, _)| column + at);
    (start < end).then(|| {
        (
            row_start + start..row_start + end,
            line[start..end].to_owned(),
        )
    })
}

/// Whether every byte of `word` is under a comment or string highlight.
pub(crate) fn in_comment_or_string(
    spans: &[fe_syntax::HighlightSpan],
    word: &Range<usize>,
) -> bool {
    let mut covered: Vec<Range<usize>> = (spans.iter())
        .filter(|span| {
            let name = span.highlight.name();
            name.starts_with("comment") || name.starts_with("string")
        })
        .map(|span| span.range.start.max(word.start)..span.range.end.min(word.end))
        .filter(|range| !range.is_empty())
        .collect();
    covered.sort_by_key(|range| range.start);
    let mut reached = word.start;
    for range in covered {
        if range.start > reached {
            return false;
        }
        reached = reached.max(range.end);
    }
    reached >= word.end
}

/// What the highlight on the word says it names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KindHint {
    Callable,
    Type,
    Module,
    Property,
}

impl KindHint {
    /// The hint of a word with this highlight. A `type` directly followed
    /// by `(` (`called`) is a call: TypeScript's query paints every
    /// capitalised identifier as a type, the `Item` of `Item()` included,
    /// over JavaScript's `function` and `constructor`.
    fn of(highlight: &str, called: bool) -> Option<KindHint> {
        match highlight {
            "constructor" => Some(KindHint::Callable),
            "type" if called => Some(KindHint::Callable),
            "module" => Some(KindHint::Module),
            "property" => Some(KindHint::Property),
            name if name.starts_with("function") => Some(KindHint::Callable),
            name if name.starts_with("type") => Some(KindHint::Type),
            _ => None,
        }
    }

    fn matches(self, kind: OutlineKind) -> bool {
        use OutlineKind::*;
        match self {
            KindHint::Callable => matches!(kind, Function | Method | Macro),
            KindHint::Type => matches!(kind, Struct | Class | Enum | Interface | Trait | Type),
            KindHint::Module => matches!(kind, Module),
            KindHint::Property => matches!(kind, Field | Method),
        }
    }
}

/// Sorts `found` best first: by tier, then whether the kind matches the
/// hint, then whether it's in the request file's directory, then by how
/// many leading directories it shares with it, then by path and start.
fn rank(found: &mut [Definition], request: &Path, hint: Option<KindHint>) {
    let dir = request.parent().unwrap_or(Path::new(""));
    found.sort_by_cached_key(|def| {
        let rel = &def.location.rel;
        let parent = rel.parent().unwrap_or(Path::new(""));
        let shared = (parent.components())
            .zip(dir.components())
            .take_while(|(a, b)| a == b)
            .count();
        (
            def.tier,
            !hint.is_some_and(|hint| hint.matches(def.kind)),
            parent != dir,
            std::cmp::Reverse(shared),
            rel.clone(),
            def.location.range.start,
        )
    });
}

/// The name a file is imported by: its stem, or its directory's name for a
/// file that stands for its directory (`mod.rs`, `index.ts`, `__init__.py`).
fn module_name(rel: &Path) -> Option<&str> {
    let stem = rel.file_stem()?.to_str()?;
    if matches!(stem, "mod" | "lib" | "main" | "index" | "__init__") {
        rel.parent()?.file_name()?.to_str()
    } else {
        Some(stem)
    }
}

/// Whether `word` occurs in `line` with no identifier char on either side.
fn has_word(line: &str, word: &str) -> bool {
    let is_ident = |ch: char| ch.is_alphanumeric() || ch == '_';
    line.match_indices(word).any(|(at, _)| {
        !line[..at].chars().next_back().is_some_and(is_ident)
            && !line[at + word.len()..].chars().next().is_some_and(is_ident)
    })
}

/// The lines of `source`'s import statements, for a language that has
/// them. A statement not complete on its first line (`use a::{`,
/// `import (`) runs on to the line that completes it. For JavaScript and
/// TypeScript, the module paths of its imports and re-exports instead.
/// `Err(Cancelled)` once `cancel` is set, which it looks at every
/// [`CANCEL_EVERY`] lines.
fn import_lines<'a>(
    language: &str,
    source: &'a str,
    cancel: &AtomicBool,
) -> Result<Vec<&'a str>, NavError> {
    let lines: Vec<&str> = source.lines().collect();
    if matches!(language, "javascript" | "typescript" | "tsx") {
        return js_imports(&lines, language != "typescript", cancel);
    }
    let mut found = Vec::new();
    let mut ix = 0;
    for step in 0.. {
        if ix >= lines.len() {
            break;
        }
        check_every(step, cancel)?;
        let Some(statement) = import_statement(language, lines[ix].trim_start()) else {
            ix += 1;
            continue;
        };
        let rest = &lines[ix..lines.len().min(ix + MAX_IMPORT_LINES)];
        let taken = &rest[..(statement.len)(rest).clamp(1, rest.len())];
        if (statement.keep)(taken) {
            found.extend_from_slice(taken);
        }
        ix += taken.len();
    }
    Ok(found)
}

/// `Err(Cancelled)` if `cancel` is set and this is a `step` to look at it
/// on: every [`CANCEL_EVERY`]th, the first included.
fn check_every(step: usize, cancel: &AtomicBool) -> Result<(), NavError> {
    if step.is_multiple_of(CANCEL_EVERY) && cancel.load(Ordering::Relaxed) {
        Err(NavError::Cancelled)
    } else {
        Ok(())
    }
}

/// How an import statement of some form runs.
struct Statement {
    /// How many of these lines, from its first, it takes.
    len: fn(&[&str]) -> usize,
    /// Whether the whole statement is an import after all.
    keep: fn(&[&str]) -> bool,
}

impl Statement {
    const LINE: Statement = Statement {
        len: |_| 1,
        keep: |_| true,
    };
    /// To the line with its `;`, as `use a::{` in Rust.
    const TO_SEMICOLON: Statement = Statement {
        len: |lines| until(lines, |line| line.contains(';')),
        keep: |_| true,
    };
    /// To the `)` closing a list its first line opens, as `import (` in Go.
    const PAREN_LIST: Statement = Statement {
        len: |lines| {
            if lines[0].contains('(') && !lines[0].contains(')') {
                until(lines, |line| line.contains(')'))
            } else {
                1
            }
        },
        keep: |_| true,
    };
}

/// How many of `lines` up to and including the first that `last` accepts;
/// all of them if none does.
fn until(lines: &[&str], last: fn(&str) -> bool) -> usize {
    lines
        .iter()
        .position(|line| last(line))
        .map_or(lines.len(), |ix| ix + 1)
}

/// Whether `line`, without its indent, starts an import statement in
/// `language`, and if so, how the statement runs.
fn import_statement(language: &str, line: &str) -> Option<Statement> {
    match language {
        "rust" => {
            let line = strip_visibility(line);
            if keyword(line, "use").is_some() {
                Some(Statement::TO_SEMICOLON)
            } else if (keyword(line, "mod").is_some() && line.contains(';'))
                || keyword(line, "extern")
                    .and_then(|rest| keyword(rest, "crate"))
                    .is_some()
            {
                Some(Statement::LINE)
            } else {
                None
            }
        }
        "python" => {
            if keyword(line, "import").is_some() {
                Some(Statement::LINE)
            } else if keyword(line, "from").is_some() {
                Some(Statement::PAREN_LIST)
            } else {
                None
            }
        }
        "go" => keyword(line, "import").map(|_| Statement::PAREN_LIST),
        "java" => keyword(line, "import").map(|_| Statement::LINE),
        "c" => {
            let directive = line.strip_prefix('#')?.trim_start();
            keyword(directive, "include").map(|_| Statement::LINE)
        }
        _ => None,
    }
}

/// The module paths of the JavaScript or TypeScript imports and re-exports
/// in these lines: `import … from 'm'`, `import 'm'`,
/// `export { … } from 'm'`, `export * as n from 'm'`, each starting a line
/// or following a `;`, a `}` or another such statement, followed across
/// lines and comments (at most [`MAX_IMPORT_LINES`] each); and the `'m'` of
/// each `require('m')` outside them, comments included, as a plain search
/// for `require(` finds it. What is in a string, a regular expression, a
/// template or a multiline comment isn't read as a statement, wherever the
/// scan restarts.
///
/// The lexer is best effort: it can't follow every JS/TS/TSX context.
/// Known misreads are parenthesized arrow return types, templates in JSX
/// type arguments, generic arrows across comments longer than its
/// lookahead, ternaries nested past its counters in a case, and spaced
/// conditionals read as optional methods. T168 replaces it with
/// tree-sitter queries.
fn js_imports<'a>(
    lines: &[&'a str],
    jsx: bool,
    cancel: &AtomicBool,
) -> Result<Vec<&'a str>, NavError> {
    let poll = Poll::new(cancel);
    let states = line_states(lines, jsx, &poll);
    poll.finish()?;
    let mut found = Vec::new();
    // Where the scan is: a line, and a byte in it after a statement, with
    // the lexical state there.
    let (mut ix, mut start) = (0, 0);
    let mut restart = None;
    while ix < lines.len() {
        poll.check(Work::Line)?;
        let window = &lines[ix..lines.len().min(ix + MAX_IMPORT_LINES)];
        let line = &window[0][start..];
        let lex = restart.take().unwrap_or(states[ix]);
        // The next statement on the line: where it starts, its module, and
        // where the scan goes on after that.
        let mut statement = None;
        // A line is searched for the keywords once, not again after each
        // statement, so a long line of them takes linear time.
        if start > 0 || line.contains("import") || line.contains("export") {
            let mut tokens = Tokens::at(window, start, lex, &poll);
            let mut statement_start = true;
            loop {
                let before = tokens.clone();
                let Some((token, 0)) = tokens.next() else {
                    break;
                };
                // A line terminator inside the line starts one too.
                statement_start |= tokens.breaks != before.breaks;
                if statement_start
                    && !before.lex.overflowed
                    && let Token::Word(keyword @ ("import" | "export")) = token
                {
                    let mut after = before.clone();
                    if let Some(module) = module_of(&mut after) {
                        // Where the keyword starts, after any comment
                        // before it, which the `require(` search reads.
                        let at = tokens.position().1 - keyword.len() - start;
                        statement = Some((at, module, after.position(), after.lex));
                        break;
                    }
                }
                statement_start = matches!(token, Token::Punct(';' | '}'));
            }
            poll.finish()?;
        }
        let mut require = &line[..statement.map_or(line.len(), |(at, ..)| at)];
        while let Some(at) = require.find("require(") {
            poll.check(Work::Require)?;
            require = &require[at + "require(".len()..];
            // Only a string is read, after any comment, and only so far,
            // so that a line of `require(` takes linear time.
            let arg = require.trim_start();
            if !(arg.starts_with(['\'', '"', '`']) || arg.starts_with("/*")) {
                continue;
            }
            let arg = &require[..require.floor_char_boundary(MAX_MODULE_SCAN)];
            if let Some((Token::Str(module), _)) =
                Tokens::at(&[arg], 0, Lex::start(false), &poll).next()
            {
                found.push(module);
            }
        }
        poll.finish()?;
        (ix, start) = match statement {
            Some((_, module, (line, after), lex)) => {
                found.push(module);
                restart = Some(lex);
                (ix + line, after)
            }
            None => (ix + 1, 0),
        };
    }
    Ok(found)
}

/// Work an import step does, for [`Poll`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Work {
    /// A line, or a statement on one, begun.
    Line,
    /// A token, or a comment or line skipped, read.
    Token,
    /// A name, string or comma of a named list read ahead.
    List,
    /// A `require(` looked at.
    Require,
    /// A definition looked at for an import of it.
    Definition,
    /// A definition's module compared with an import.
    Compare,
}

/// Counts the work an import step does, and looks at a cancel flag every
/// [`CANCEL_EVERY`] units of it, the first included. Once it has seen the
/// flag set it keeps saying to stop.
struct Poll<'c> {
    cancel: &'c AtomicBool,
    done: Cell<usize>,
    stopped: Cell<bool>,
}

impl<'c> Poll<'c> {
    fn new(cancel: &'c AtomicBool) -> Self {
        Poll {
            cancel,
            done: Cell::new(0),
            stopped: Cell::new(false),
        }
    }

    /// Counts a unit of `work`; whether to stop.
    fn tick(&self, _work: Work) -> bool {
        if self.stopped.get() {
            return true;
        }
        #[cfg(test)]
        progress(_work);
        let done = self.done.get();
        self.done.set(done + 1);
        if done.is_multiple_of(CANCEL_EVERY) && self.cancel.load(Ordering::Relaxed) {
            self.stopped.set(true);
        }
        self.stopped.get()
    }

    /// [`Poll::tick`], as an error to propagate.
    fn check(&self, work: Work) -> Result<(), NavError> {
        if self.tick(work) {
            Err(NavError::Cancelled)
        } else {
            Ok(())
        }
    }

    /// `Err(Cancelled)` if it has said to stop, which a [`Tokens`] does by
    /// ending.
    fn finish(&self) -> Result<(), NavError> {
        if self.stopped.get() {
            Err(NavError::Cancelled)
        } else {
            Ok(())
        }
    }
}

/// What a test does as import steps work, on its own thread.
#[cfg(test)]
use std::cell::RefCell;

#[cfg(test)]
type Progress = Box<dyn FnMut(Work)>;

#[cfg(test)]
thread_local! {
    static PROGRESS: RefCell<Option<Progress>> = const { RefCell::new(None) };
}

#[cfg(test)]
fn progress(work: Work) {
    PROGRESS.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook(work);
        }
    });
}

/// The module path of the import or re-export statement at `tokens`,
/// which it leaves after the path.
fn module_of<'a>(tokens: &mut Tokens<'a, '_>) -> Option<&'a str> {
    match next_token(tokens)? {
        Token::Word("import") => {
            let mut ahead = tokens.clone();
            match next_token(&mut ahead)? {
                Token::Str(module) => {
                    *tokens = ahead;
                    return Some(module);
                }
                // `import(…)` and `import.meta` aren't statements.
                Token::Word(_) | Token::Punct('{' | '*') => import_clause(tokens)?,
                _ => return None,
            }
        }
        Token::Word("export") => {
            let mut ahead = tokens.clone();
            if next_token(&mut ahead)? == Token::Word("type") {
                *tokens = ahead;
            }
            names_or_namespace(tokens)?;
        }
        _ => return None,
    }
    if next_token(tokens)? != Token::Word("from") {
        return None;
    }
    match next_token(tokens)? {
        Token::Str(module) => Some(module),
        _ => None,
    }
}

/// Takes an import's bindings: modifiers (`type`, `typeof`, `defer`, …),
/// then a default binding, `,` and named or namespace ones, or either
/// alone. A binding may be named `from`: a word is the default binding
/// when a `,`, or `from` and the module, follows it, and a modifier when
/// another binding does. At most [`MAX_MODIFIERS`] modifiers.
fn import_clause(tokens: &mut Tokens<'_, '_>) -> Option<()> {
    for _ in 0..=MAX_MODIFIERS {
        let mut ahead = tokens.clone();
        let Token::Word(_) = next_token(&mut ahead)? else {
            return names_or_namespace(tokens);
        };
        let mut after = ahead.clone();
        match next_token(&mut after)? {
            Token::Punct(',') => {
                *tokens = after;
                return names_or_namespace(tokens);
            }
            Token::Word("from") if matches!(next_token(&mut after), Some(Token::Str(_))) => {
                *tokens = ahead;
                return Some(());
            }
            Token::Word(_) | Token::Punct('{' | '*') => *tokens = ahead,
            _ => return None,
        }
    }
    None
}

/// The most modifiers an import's bindings follow, as `type` in
/// `import type from from 'm'`; so a run of words that isn't an import is
/// given up early.
const MAX_MODIFIERS: usize = 2;

/// Takes `{ … }`, or `*` and, if there is one, `as` and a name (which may
/// be `from`).
fn names_or_namespace(tokens: &mut Tokens<'_, '_>) -> Option<()> {
    match next_token(tokens)? {
        Token::Punct('{') => {
            // Names, `as`, `type`, strings and commas to the `}`; anything
            // else (an unclosed brace's `;`, say) isn't a list of names.
            loop {
                if tokens.poll.tick(Work::List) {
                    return None;
                }
                match next_token(tokens)? {
                    Token::Punct('}') => return Some(()),
                    Token::Word(_) | Token::Str(_) | Token::Punct(',') => {}
                    _ => return None,
                }
            }
        }
        Token::Punct('*') => {
            let mut ahead = tokens.clone();
            if next_token(&mut ahead)? == Token::Word("as") {
                match next_token(&mut ahead)? {
                    Token::Word(_) | Token::Str(_) => *tokens = ahead,
                    _ => return None,
                }
            }
            Some(())
        }
        _ => None,
    }
}

fn next_token<'a>(tokens: &mut Tokens<'a, '_>) -> Option<Token<'a>> {
    tokens.next().map(|(token, _)| token)
}

/// A JavaScript token, roughly: enough to follow an import statement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token<'a> {
    /// An identifier, whole: Unicode escapes (`a`) and combining marks
    /// included.
    Word(&'a str),
    /// A string's contents.
    Str(&'a str),
    /// A regular expression literal, flags included.
    Regex,
    /// A numeric literal.
    Number,
    Punct(char),
}

/// Where a scan is in the text's lexical structure, which a scan restarted
/// part-way must be given rather than guess: whether it's in a comment, a
/// string, a template or JSX, and enough of the expression around to tell
/// a regular expression from a division.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Lex {
    mode: Mode,
    /// Whether a `<` where an operand goes may start a JSX element: in
    /// JavaScript and TSX, not TypeScript, whose `<T>x` is an assertion.
    jsx: bool,
    /// Whether a `/` here starts a regular expression, not a division.
    regex_ok: bool,
    /// Whether a `{` here opens a block, not an object.
    block_ok: bool,
    /// Whether a `(` here opens the condition of an `if`, `while`, ….
    condition: bool,
    /// Whether a statement starts here.
    stmt: bool,
    /// Whether a `.` came just before.
    property: bool,
    /// Whether a `)` came just before: a `:` after it starts a return type.
    paren: bool,
    /// Whether a `>` closing type arguments came just before, which may
    /// end a type that a body follows.
    type_end: bool,
    /// How many `<` of type arguments haven't met their `>`. Only a `<`
    /// in a type (a header's, or a return type) is counted: elsewhere it's
    /// a comparison.
    generics: u8,
    /// Whether a TypeScript `as`, `satisfies` or `is` came just before: a
    /// type follows, and a `const` is a value.
    type_op: bool,
    /// Whether a `void` came just before, which may end a type at a line
    /// break.
    void_word: bool,
    /// Whether a `void` came just before in a type (after a `:`, or in a
    /// return type), which a body may follow.
    void_type: bool,
    /// Whether a `:` came just before.
    colon: bool,
    /// Whether what came just before ends its statement at a line break
    /// (a `return`, a `break` and its label, …): a statement follows.
    restricted: bool,
    /// Whether a `break` or `continue` came just before, which a label may
    /// follow.
    jump: bool,
    /// Whether a line break came, where statements are, after what ends a
    /// statement there (a value, a `return`), so that a declaration may
    /// start.
    line_break: bool,
    /// Whether a name here, at a statement's start, is a label if a `:`
    /// follows.
    label: bool,
    /// Whether a `function` or `class` came just before, which a line
    /// break doesn't end.
    heading: bool,
    /// Whether a decorator's `@` or `.` came just before: a name follows.
    decorator: bool,
    /// Whether a decorator's name came just before: a `.` or `(` goes on
    /// with it, and a statement follows it.
    decorated: bool,
    /// Whether a regular expression was looked for on this line and not
    /// found: a `/` after is a division. (It's not looked for again, which
    /// on a line of them would take quadratic time.)
    no_regex: bool,
    /// What is waiting for its end, innermost last: each its [`Scope`] in
    /// the low 3 bits, the depth of brackets it is at in the next 7, and
    /// for a [`Scope::Question`] how many more conditionals' `?` there wait
    /// with it in the top 6. A class's `extends`, a function's parameters
    /// and a case's expression hold others. The first
    /// [`pending`](Lex::pending); past [`MAX_SCOPES`] the outermost is
    /// forgotten.
    scopes: [u16; MAX_SCOPES],
    pending: u8,
    /// The kinds of the brackets open, 3 bits each, the innermost lowest,
    /// for the first [`MAX_OPEN`]: the low 2 bits in `open`, the high one
    /// in `open_high`.
    open: u128,
    open_high: u64,
    depth: u32,
    /// Whether a bracket was opened past [`MAX_OPEN`], so what is open is
    /// no longer known. Statements aren't looked for after.
    overflowed: bool,
}

/// Something begun that waits for its end at the depth it was begun.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    /// A declaration (`function`, `class`, or TypeScript's `interface`,
    /// …), a statement: its body is a block.
    Declaration = 0,
    /// A `function` or `class` expression: after its body is a value.
    Expression = 1,
    /// A decorator's arguments: a statement follows their `)`.
    Decorator = 2,
    /// A `case`, whose `:` a statement follows.
    Case = 3,
    /// A conditional's `?`, waiting for its `:`.
    Question = 4,
    /// A return type, after a `)` and a `:`, that a body may follow.
    ReturnType = 5,
}

const SCOPES: [Scope; 6] = [
    Scope::Declaration,
    Scope::Expression,
    Scope::Decorator,
    Scope::Case,
    Scope::Question,
    Scope::ReturnType,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Code,
    /// In a `/* */` comment that a line began.
    Block,
    /// In a string with this quote, continued over a line end by a `\`.
    Quote(char),
    /// In the text of a template literal; its `${}` substitutions are code.
    Template,
    /// In a JSX tag, among its name and attributes.
    JsxTag,
    /// In a JSX attribute's string with this quote.
    JsxQuote(char),
    /// In a JSX element's children, text and `{}` expressions.
    JsxText,
    /// In a JSX closing tag.
    JsxClose,
    /// In a `/* */` comment in a JSX tag.
    JsxComment,
    /// In a JSX tag's type arguments, this many `<` deep, and in a `/* */`
    /// comment there or not.
    JsxTypeArgs(u8, bool),
}

/// A bracket's kind: anything not below, `{` of an object, `(` or `[`.
const OTHER: u8 = 0;
/// A bracket's kind: a block; a statement follows it.
const BLOCK: u8 = 1;
/// A bracket's kind: a `${` in a template.
const SUBSTITUTION: u8 = 2;
/// A bracket's kind: the body of a function or class expression, whose
/// inside is statements and after it a value.
const BODY: u8 = 3;
/// A bracket's kind: the condition of an `if`, `while`, `for`, …; a
/// statement follows it.
const COND: u8 = 4;
/// A bracket's kind: a `{}` among JSX children.
const JSX_CHILD: u8 = 5;
/// A bracket's kind: a `{}` among a JSX tag's attributes.
const JSX_ATTR: u8 = 6;
/// A bracket's kind: a JSX element, from its `<` to its closing tag.
const ELEMENT: u8 = 7;
/// The most brackets whose kind is kept.
const MAX_OPEN: u32 = 64;
/// The most bytes after a `require(` read for its module's string.
const MAX_MODULE_SCAN: usize = 4096;
/// The most scopes kept waiting at once.
const MAX_SCOPES: usize = 16;
/// The most conditionals' `?` counted in a scope beyond its first.
const MORE_QUESTIONS_MAX: u16 = 63;
/// The bits of a scope's depth, after its kind. (A depth past
/// [`MAX_OPEN`] has stopped the scan for statements.)
const DEPTH_MASK: u32 = 0x7f;

/// The depth of brackets a scope's entry was begun at.
fn scope_depth(entry: u16) -> u32 {
    u32::from(entry >> 3) & DEPTH_MASK
}

/// Whether `ch` ends a line in JavaScript. (`\n` ends the lines a scan is
/// given; these may be inside one.)
fn is_line_terminator(ch: char) -> bool {
    matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

impl Lex {
    /// The start of a text, JSX in it or not.
    const fn start(jsx: bool) -> Lex {
        Lex {
            mode: Mode::Code,
            jsx,
            regex_ok: true,
            block_ok: true,
            condition: false,
            stmt: true,
            property: false,
            paren: false,
            type_end: false,
            generics: 0,
            type_op: false,
            void_word: false,
            void_type: false,
            colon: false,
            restricted: false,
            jump: false,
            line_break: false,
            label: false,
            heading: false,
            decorator: false,
            decorated: false,
            no_regex: false,
            scopes: [0; MAX_SCOPES],
            pending: 0,
            open: 0,
            open_high: 0,
            depth: 0,
            overflowed: false,
        }
    }

    /// After something with a value (a name, a string, `)`, …).
    fn value(&mut self) {
        self.regex_ok = false;
        self.block_ok = false;
        self.condition = false;
        self.stmt = false;
        self.property = false;
        self.paren = false;
        self.type_end = false;
        self.type_op = false;
        self.void_word = false;
        self.void_type = false;
        self.colon = false;
        self.restricted = false;
        self.jump = false;
        self.line_break = false;
        self.label = false;
        self.heading = false;
        self.decorator = false;
        self.decorated = false;
    }

    /// After something that an operand follows.
    fn operator(&mut self) {
        self.value();
        self.regex_ok = true;
    }

    /// A line ends here. Where statements are, after a value it ends the
    /// statement too, if what follows can't go on with it: a semicolon is
    /// inserted. (Only the declarations that may follow are looked at, and
    /// nothing goes on with a value by them.)
    fn line_end(&mut self) {
        self.no_regex = false;
        if !matches!(self.mode, Mode::Code | Mode::Block) || self.heading || !self.in_statements() {
            return;
        }
        if self.restricted {
            // A statement ended, and another starts.
            self.regex_ok = true;
            self.block_ok = true;
            self.stmt = true;
            self.restricted = false;
            self.jump = false;
        } else if !self.regex_ok || self.void_word {
            self.line_break = true;
        }
    }

    /// The kind of the innermost bracket open, if any.
    fn innermost(&self) -> Option<u8> {
        if self.depth == 0 {
            None
        } else if self.depth > MAX_OPEN {
            Some(OTHER)
        } else {
            Some((((self.open_high & 1) << 2) as u8) | ((self.open & 3) as u8))
        }
    }

    /// Whether statements, not an expression, are what this depth holds:
    /// at the top, or in a block or a body.
    fn in_statements(&self) -> bool {
        matches!(self.innermost(), None | Some(BLOCK | BODY))
    }

    /// A scope begun at this depth.
    fn begin(&mut self, scope: Scope) {
        self.end_scopes(self.depth + 1);
        if self.overflowed {
            return;
        }
        // Conditionals nested are counted in one scope.
        if scope == Scope::Question && self.scope() == Some(Scope::Question) {
            let top = &mut self.scopes[usize::from(self.pending) - 1];
            if *top >> 10 < MORE_QUESTIONS_MAX {
                *top += 1 << 10;
            }
            return;
        }
        if usize::from(self.pending) == MAX_SCOPES {
            self.scopes.copy_within(1.., 0);
            self.pending -= 1;
        }
        let depth = u16::try_from(self.depth.min(DEPTH_MASK)).unwrap_or(0) << 3;
        self.scopes[usize::from(self.pending)] = depth | scope as u16;
        self.pending += 1;
    }

    /// The innermost scope, if it was begun at this depth.
    fn scope(&self) -> Option<Scope> {
        let top = *self.scopes[..usize::from(self.pending)].last()?;
        if scope_depth(top) != self.depth {
            return None;
        }
        SCOPES.get(usize::from(top & 7)).copied()
    }

    /// Ends the innermost scope if it's `scope`, begun at this depth (one
    /// of the conditionals it counts, for a [`Scope::Question`]).
    fn end(&mut self, scope: Scope) -> bool {
        let ended = self.scope() == Some(scope);
        if ended {
            let top = &mut self.scopes[usize::from(self.pending) - 1];
            if *top >> 10 > 0 {
                *top -= 1 << 10;
            } else {
                self.pending -= 1;
            }
        }
        ended
    }

    /// Ends the scopes begun at `depth` or deeper.
    fn end_scopes(&mut self, depth: u32) {
        while let Some(&top) = self.scopes[..usize::from(self.pending)].last()
            && scope_depth(top) >= depth
        {
            self.pending -= 1;
        }
    }

    /// Whether a type is read here: a header's, or a return type.
    fn in_type(&self) -> bool {
        matches!(
            self.scope(),
            Some(Scope::Declaration | Scope::Expression | Scope::ReturnType)
        )
    }

    /// The header begun at this depth that a `{` here is the body of, if
    /// any, past a return type; neither waits any longer.
    fn take_header(&mut self) -> Option<Scope> {
        self.end(Scope::ReturnType);
        let header = self
            .scope()
            .filter(|&scope| matches!(scope, Scope::Declaration | Scope::Expression))?;
        self.pending -= 1;
        Some(header)
    }

    /// After the `:` of a label or a case: a statement starts.
    fn statement_colon(&mut self) {
        self.operator();
        self.block_ok = true;
        self.stmt = true;
    }

    fn push(&mut self, kind: u8) {
        if self.depth < MAX_OPEN {
            self.open = (self.open << 2) | u128::from(kind & 3);
            self.open_high = (self.open_high << 1) | u64::from(kind >> 2);
        } else {
            self.overflowed = true;
        }
        self.depth += 1;
    }

    fn pop(&mut self) -> u8 {
        let Some(kind) = self.innermost() else {
            return OTHER;
        };
        if self.depth <= MAX_OPEN {
            self.open >>= 2;
            self.open_high >>= 1;
        }
        self.depth -= 1;
        // What waits inside a bracket is over with it.
        self.end_scopes(self.depth + 1);
        kind
    }

    /// A JSX element ended: what follows is its parent's children, or a
    /// value.
    fn element_end(&mut self) {
        self.pop();
        if self.innermost() == Some(ELEMENT) {
            self.mode = Mode::JsxText;
        } else {
            self.mode = Mode::Code;
            self.value();
        }
    }

    /// The state after the word `word`, with `rest` after it on its line.
    fn word(&mut self, word: &str, rest: &str) {
        let lex = *self;
        self.value();
        if !lex.property {
            // A statement starts here, or would after the line break.
            let stmt = lex.stmt || lex.line_break;
            // What waits at this depth that a statement begins after is
            // over, but for a class's heritage going on over a line break.
            if stmt && !matches!(word, "extends" | "implements") {
                self.end_scopes(lex.depth);
            }
            // `of` is a name but after a `for`'s binding, and `const`
            // after an `as` a type. In a type, its operators take one.
            self.regex_ok = (REGEX_AFTER.contains(&word)
                && (word != "of" || (!lex.regex_ok && !lex.line_break))
                && !(word == "const" && lex.type_op))
                || (matches!(word, "as" | "satisfies" | "is") && !lex.regex_ok && !lex.line_break)
                || (lex.in_type()
                    && matches!(word, "keyof" | "readonly" | "unique" | "infer" | "asserts"));
            self.type_op = self.regex_ok && matches!(word, "as" | "satisfies" | "is");
            self.void_word = word == "void";
            self.void_type =
                word == "void" && (lex.colon || lex.scope() == Some(Scope::ReturnType));
            // A label follows a `break` or `continue` on its line.
            self.restricted =
                matches!(word, "return" | "yield" | "break" | "continue" | "debugger")
                    || (lex.jump && !lex.line_break);
            self.jump = matches!(word, "break" | "continue");
            self.label = stmt;
            self.block_ok = matches!(word, "else" | "do" | "try" | "finally" | "catch");
            self.stmt = matches!(word, "else" | "do" | "try" | "finally")
                // Modifiers of a statement don't end its start.
                || (stmt
                    && matches!(
                        word,
                        "export" | "default" | "async" | "declare" | "abstract" | "const"
                    ));
            self.condition = matches!(word, "if" | "while" | "for" | "with" | "switch" | "catch")
                || (word == "await" && lex.condition);
            if word == "case" && stmt {
                self.begin(Scope::Case);
            }
            if matches!(word, "function" | "class") {
                self.begin(if stmt {
                    Scope::Declaration
                } else {
                    Scope::Expression
                });
                self.heading = true;
            }
            // TypeScript's declarations, whose bodies are blocks. (Its
            // reserved words always begin one.)
            if stmt && matches!(word, "interface" | "enum") {
                self.begin(Scope::Declaration);
                self.heading = true;
            }
            if stmt
                && matches!(word, "namespace" | "module" | "global")
                && ts_declaration_follows(word, rest)
            {
                self.begin(Scope::Declaration);
            }
        }
        // A decorator's name, or a part of it; a statement follows.
        if lex.decorator {
            self.decorated = true;
            self.stmt = true;
        }
    }

    /// The state after the token `ch`, a punctuator, at the start of
    /// `rest`; and how many bytes of `rest` that token is.
    fn punct(&mut self, ch: char, rest: &str) -> usize {
        match ch {
            '(' => {
                if self.decorator || self.decorated {
                    self.begin(Scope::Decorator);
                }
                let kind = if self.condition { COND } else { OTHER };
                self.push(kind);
                self.operator();
            }
            ')' => {
                let kind = self.pop();
                self.value();
                // A statement follows a condition, or a decorator; a block
                // follows a `)`, or a return type.
                let decorator = self.end(Scope::Decorator);
                self.regex_ok = kind == COND;
                self.stmt = kind == COND || decorator;
                self.block_ok = true;
                self.paren = true;
            }
            '[' => {
                self.push(OTHER);
                self.operator();
            }
            ']' => {
                self.pop();
                self.value();
            }
            '{' => {
                // A body follows a header's name, parameters, heritage or
                // type; after an operator a `{` is an object or a type.
                let after_value = !self.regex_ok || self.type_end || self.void_type;
                let header = if after_value {
                    self.take_header()
                } else {
                    None
                };
                let kind = match header {
                    Some(Scope::Expression) => BODY,
                    Some(_) => BLOCK,
                    // A value can't go on with a `{`: after one it's a body
                    // (a method's, after its return type, or a static
                    // block), or a block after a line break.
                    None if self.block_ok || after_value => BLOCK,
                    None => OTHER,
                };
                if header.is_some() {
                    self.generics = 0;
                }
                self.push(kind);
                self.operator();
                // Statements inside a block or a body.
                self.block_ok = kind == BLOCK || kind == BODY;
                self.stmt = self.block_ok;
            }
            '}' => {
                let kind = self.pop();
                match kind {
                    SUBSTITUTION => self.mode = Mode::Template,
                    JSX_CHILD => self.mode = Mode::JsxText,
                    JSX_ATTR => self.mode = Mode::JsxTag,
                    _ => {
                        self.value();
                        // A statement follows a block, a value the rest.
                        self.regex_ok = kind == BLOCK;
                        self.block_ok = kind == BLOCK;
                        self.stmt = kind == BLOCK;
                    }
                }
            }
            // A list's item ends with its return type. (Type arguments
            // hold commas.)
            ',' => {
                if self.generics == 0 {
                    self.end(Scope::ReturnType);
                }
                self.operator();
            }
            ';' => {
                self.operator();
                // (A `for` header's `;` separates expressions.)
                if self.innermost() != Some(COND) {
                    self.block_ok = true;
                    self.stmt = true;
                }
                self.generics = 0;
                self.end_scopes(self.depth);
            }
            '.' if rest.starts_with("...") => {
                self.operator();
                return 3;
            }
            '.' => {
                let decorated = self.decorated;
                self.operator();
                self.property = true;
                self.decorator = decorated;
            }
            // A private name is a property's.
            '#' => {
                self.operator();
                self.property = true;
            }
            // TypeScript's non-null `!` after a value keeps it one. (After
            // a line break, it's a `!` starting a statement.)
            '!' if !self.regex_ok && !self.line_break && !rest[1..].starts_with('=') => return 1,
            '@' => {
                let stmt = self.stmt || self.line_break;
                self.operator();
                self.decorator = stmt;
            }
            '?' if rest[1..].starts_with('?') => {
                self.operator();
                return 2;
            }
            '?' if rest[1..].starts_with('.')
                && !rest[2..].starts_with(|next: char| next.is_ascii_digit()) =>
            {
                self.operator();
                self.property = true;
                return 2;
            }
            '?' => {
                // A conditional's, not the mark of TypeScript's optional
                // method (`m?()`). (Other optional marks are ended with
                // their bracket or `;`, or taken with their `:`, which is
                // all the same here.)
                if !(self.label && inline_trivia_end(&rest[1..]).starts_with(['(', '<'])) {
                    self.begin(Scope::Question);
                }
                self.operator();
            }
            ':' if self.label => self.statement_colon(),
            // (A conditional's `:` isn't a type's.)
            ':' if self.end(Scope::Question) => self.operator(),
            ':' if self.end(Scope::Case) => self.statement_colon(),
            ':' => {
                let paren = self.paren;
                self.operator();
                self.colon = true;
                if paren {
                    self.begin(Scope::ReturnType);
                }
            }
            // `++` and `--` don't change what's expected next.
            '+' | '-' if rest[1..].starts_with(ch) => return 2,
            '=' if rest[1..].starts_with('>') => {
                // An arrow's body follows its return type, unless the
                // arrow is a function type's in a return type.
                if !self.paren {
                    self.end(Scope::ReturnType);
                }
                self.operator();
                self.block_ok = true;
                return 2;
            }
            // A shift or a comparison. (A `>=` may close type arguments.)
            // (In a type, `<<` opens type arguments and a function type's
            // type parameters.)
            '<' if rest[1..].starts_with(['<', '=']) => {
                if self.in_type() && rest[1..].starts_with('<') {
                    self.generics = self.generics.saturating_add(2);
                }
                self.operator();
                return 2;
            }
            '<' => {
                if self.in_type() {
                    self.generics = self.generics.saturating_add(1);
                }
                self.operator();
            }
            '>' => {
                let generic = self.generics > 0;
                self.generics = self.generics.saturating_sub(1);
                self.operator();
                self.type_end = generic;
            }
            _ => self.operator(),
        }
        ch.len_utf8()
    }
}

/// Words after which a `/` starts a regular expression.
const REGEX_AFTER: &[&str] = &[
    "return",
    "typeof",
    "instanceof",
    "in",
    "of",
    "new",
    "delete",
    "void",
    "throw",
    "case",
    "do",
    "else",
    "yield",
    "await",
    "extends",
    "default",
    "const",
    "let",
    "var",
];

/// One primitive move of [`Tokens`].
enum Step<'a> {
    /// Something skipped, or a line's end passed over.
    Skipped,
    /// A new line was begun.
    Line,
    Token(Token<'a>, usize),
    End,
}

/// The tokens of some lines, with the index of the line each is on,
/// skipping comments, template text and the insides of regular
/// expressions. A string runs to its closing quote, over line ends that a
/// `\` escapes. Each step counts as work on `poll`; once that says to stop,
/// there are no more tokens.
#[derive(Clone)]
struct Tokens<'a, 'l> {
    lines: &'l [&'a str],
    line: usize,
    rest: &'a str,
    lex: Lex,
    poll: &'l Poll<'l>,
    /// How many line terminators have been passed inside lines.
    breaks: usize,
}

impl<'a, 'l> Tokens<'a, 'l> {
    /// The tokens from byte `start` of the first of `lines`, which is in
    /// `lex`.
    fn at(lines: &'l [&'a str], start: usize, lex: Lex, poll: &'l Poll<'l>) -> Self {
        Tokens {
            lines,
            line: 0,
            rest: lines.first().map_or("", |line| &line[start..]),
            lex,
            poll,
            breaks: 0,
        }
    }

    /// Where the next token is looked for from: a line, and a byte in it.
    fn position(&self) -> (usize, usize) {
        let line = self.lines.get(self.line).map_or(0, |line| line.len());
        (self.line, line - self.rest.len())
    }

    fn step(&mut self) -> Step<'a> {
        if self.poll.tick(Work::Token) {
            return Step::End;
        }
        match self.lex.mode {
            Mode::Code => {}
            Mode::Block => match self.rest.find("*/") {
                Some(end) => {
                    if self.rest[..end].contains(is_line_terminator) {
                        self.line_end();
                    }
                    self.rest = &self.rest[end + 2..];
                    self.lex.mode = Mode::Code;
                }
                None => self.rest = "",
            },
            // (An empty rest is the end of the line, not a string's start.)
            Mode::Quote(quote) if !self.rest.is_empty() => match string_end(self.rest, quote) {
                Ending::Closed(end) => {
                    self.rest = &self.rest[end + 1..];
                    self.lex.mode = Mode::Code;
                    self.lex.value();
                }
                Ending::Continued => self.rest = "",
                // Not closed, and not continued: cut off at the line end.
                Ending::Open => {
                    self.rest = "";
                    self.lex.mode = Mode::Code;
                    self.lex.value();
                }
            },
            Mode::Quote(_) => {}
            Mode::Template => match template_text(self.rest) {
                TemplateEnd::Closed(end) => {
                    self.rest = &self.rest[end + 1..];
                    self.lex.mode = Mode::Code;
                    self.lex.value();
                }
                TemplateEnd::Substitution(end) => {
                    self.rest = &self.rest[end + 2..];
                    self.lex.mode = Mode::Code;
                    self.lex.push(SUBSTITUTION);
                    self.lex.operator();
                }
                TemplateEnd::Open => self.rest = "",
            },
            Mode::JsxTag => {
                self.rest = self.rest.trim_start();
                if !self.rest.is_empty() {
                    self.jsx_tag();
                }
            }
            Mode::JsxQuote(quote) => match self.rest.find(quote) {
                Some(end) => {
                    self.rest = &self.rest[end + 1..];
                    self.lex.mode = Mode::JsxTag;
                }
                None => self.rest = "",
            },
            Mode::JsxText => match self.rest.find(['{', '<']) {
                Some(at) => {
                    let after = &self.rest[at + 1..];
                    if self.rest[at..].starts_with('{') {
                        self.lex.push(JSX_CHILD);
                        self.lex.mode = Mode::Code;
                        self.lex.operator();
                    } else if after.trim_start().starts_with('/') {
                        self.lex.mode = Mode::JsxClose;
                    } else {
                        self.lex.push(ELEMENT);
                        self.lex.mode = Mode::JsxTag;
                    }
                    self.rest = after;
                }
                None => self.rest = "",
            },
            Mode::JsxTypeArgs(open, comment) => self.jsx_type_args(open, comment),
            Mode::JsxComment => match self.rest.find("*/") {
                Some(end) => {
                    self.rest = &self.rest[end + 2..];
                    self.lex.mode = Mode::JsxTag;
                }
                None => self.rest = "",
            },
            Mode::JsxClose => match self.rest.find('>') {
                Some(end) => {
                    self.rest = &self.rest[end + 1..];
                    self.lex.element_end();
                }
                None => self.rest = "",
            },
        }
        let code = self.lex.mode == Mode::Code;
        if code {
            let trimmed = self.rest.trim_start();
            if self.rest[..self.rest.len() - trimmed.len()].contains(is_line_terminator) {
                self.line_end();
            }
            self.rest = trimmed;
            // A comment runs to the line's end, which a line terminator
            // inside it may be.
            if let Some(comment) = self.rest.strip_prefix("//") {
                self.rest = comment
                    .find(is_line_terminator)
                    .map_or("", |end| &comment[end..]);
                if !self.rest.is_empty() {
                    return Step::Skipped;
                }
            }
        }
        if self.rest.is_empty() {
            self.lex.line_end();
            self.line += 1;
            return match self.lines.get(self.line) {
                Some(line) => {
                    self.rest = line;
                    Step::Line
                }
                None => Step::End,
            };
        }
        if !code {
            return Step::Skipped;
        }
        if let Some(rest) = self.rest.strip_prefix("/*") {
            self.rest = rest;
            self.lex.mode = Mode::Block;
            return Step::Skipped;
        }
        let ch = self.rest.chars().next().unwrap_or(' ');
        let line = self.line;
        if matches!(ch, '\'' | '"') {
            let body = &self.rest[1..];
            return match string_end(body, ch) {
                Ending::Closed(end) => {
                    self.rest = &body[end + 1..];
                    self.lex.value();
                    Step::Token(Token::Str(&body[..end]), line)
                }
                Ending::Continued => {
                    self.rest = "";
                    self.lex.mode = Mode::Quote(ch);
                    Step::Skipped
                }
                Ending::Open => {
                    self.rest = "";
                    self.lex.value();
                    Step::Token(Token::Str(body), line)
                }
            };
        }
        if ch == '`' {
            let body = &self.rest[1..];
            if let TemplateEnd::Closed(end) = template_text(body) {
                self.rest = &body[end + 1..];
                self.lex.value();
                return Step::Token(Token::Str(&body[..end]), line);
            }
            self.rest = body;
            self.lex.mode = Mode::Template;
            return Step::Skipped;
        }
        if ch == '<' && self.lex.jsx && self.lex.regex_ok && self.jsx_starts() {
            self.rest = &self.rest[1..];
            self.lex.push(ELEMENT);
            self.lex.mode = Mode::JsxTag;
            return Step::Skipped;
        }
        if ch == '/' && self.lex.regex_ok && !self.lex.no_regex {
            if let Some(end) = regex_end(self.rest) {
                self.rest = &self.rest[end..];
                self.lex.value();
                return Step::Token(Token::Regex, line);
            }
            self.lex.no_regex = true;
        }
        if ch.is_ascii_digit()
            || (ch == '.' && self.rest[1..].starts_with(|next: char| next.is_ascii_digit()))
        {
            self.rest = &self.rest[number_end(self.rest)..];
            self.lex.value();
            return Step::Token(Token::Number, line);
        }
        let end = ident_end(self.rest);
        if end > 0 {
            let (word, rest) = self.rest.split_at(end);
            self.rest = rest;
            self.lex.word(word, rest);
            return Step::Token(Token::Word(word), line);
        }
        let len = self.lex.punct(ch, self.rest);
        self.rest = &self.rest[len..];
        Step::Token(Token::Punct(ch), line)
    }

    /// Whether the `<` the rest starts with, where an operand goes, begins
    /// a JSX element's tag: a name or a fragment's `>` follows, but not
    /// TypeScript's type parameters of an arrow function (`<T,>`,
    /// `<T = U>`, `<T extends U>`, `<const T,>`). What follows is looked
    /// at past comments and line ends, for a few lines.
    fn jsx_starts(&self) -> bool {
        let end = self.lines.len().min(self.line + 1 + JSX_LOOKAHEAD);
        let mut after = Lookahead {
            lines: self.lines.get(self.line + 1..end).unwrap_or(&[]),
            rest: &self.rest[1..],
        };
        if after.next().starts_with('>') {
            return true;
        }
        let name = after.word();
        if name.is_empty() {
            return false;
        }
        // (`const` before a type parameter.)
        if name == "const" && after.word().is_empty() {
            return true;
        }
        let next = after.next();
        if next.starts_with(',') || (next.starts_with('=') && !next.starts_with("=>")) {
            return false;
        }
        // A JSX attribute may be named `extends`: a type follows the
        // keyword.
        after.word() != "extends" || after.next().starts_with(['=', '>', '/'])
    }

    /// A line ends inside the line being read.
    fn line_end(&mut self) {
        self.lex.line_end();
        self.breaks += 1;
    }

    /// A JSX tag's type arguments, `open` `<` deep at the start of the
    /// rest and in a comment or not, to their end or the line's: strings,
    /// comments and `=>` in them aside.
    fn jsx_type_args(&mut self, mut open: u8, comment: bool) {
        let rest = self.rest;
        let mut at = 0;
        if comment {
            match rest.find("*/") {
                Some(end) => at = end + 2,
                None => {
                    self.rest = "";
                    return;
                }
            }
        }
        while let Some(ch) = rest[at..].chars().next() {
            let next = at + ch.len_utf8();
            match ch {
                '=' if rest[next..].starts_with('>') => at = next + 1,
                '\'' | '"' | '`' => match string_end(&rest[next..], ch) {
                    Ending::Closed(end) => at = next + end + 1,
                    Ending::Continued | Ending::Open => break,
                },
                '/' if rest[next..].starts_with('/') => {
                    match rest[next..].find(is_line_terminator) {
                        Some(end) => at = next + end,
                        None => break,
                    }
                }
                '/' if rest[next..].starts_with('*') => match rest[next + 1..].find("*/") {
                    Some(end) => at = next + 1 + end + 2,
                    None => {
                        self.rest = "";
                        self.lex.mode = Mode::JsxTypeArgs(open, true);
                        return;
                    }
                },
                '<' => {
                    open = open.saturating_add(1);
                    at = next;
                }
                '>' => {
                    open = open.saturating_sub(1);
                    at = next;
                    if open == 0 {
                        self.rest = &rest[at..];
                        self.lex.mode = Mode::JsxTag;
                        return;
                    }
                }
                _ => at = next,
            }
        }
        self.rest = "";
        self.lex.mode = Mode::JsxTypeArgs(open, false);
    }

    /// One part of a JSX tag, at the start of the rest, which isn't empty.
    fn jsx_tag(&mut self) {
        let rest = self.rest;
        if let Some(after) = rest.strip_prefix("/>") {
            self.rest = after;
            self.lex.element_end();
        } else if let Some(after) = rest.strip_prefix('>') {
            self.rest = after;
            self.lex.mode = Mode::JsxText;
        } else if let Some(after) = rest.strip_prefix('{') {
            self.rest = after;
            self.lex.push(JSX_ATTR);
            self.lex.mode = Mode::Code;
            self.lex.operator();
        } else if let Some(comment) = rest.strip_prefix("//") {
            // To the line's end, which a line terminator inside it may be.
            self.rest = comment
                .find(is_line_terminator)
                .map_or("", |end| &comment[end..]);
        } else if let Some(after) = rest.strip_prefix("/*") {
            self.rest = after;
            self.lex.mode = Mode::JsxComment;
        } else if rest.starts_with('<') {
            // Type arguments, after the tag's name.
            self.lex.mode = Mode::JsxTypeArgs(0, false);
        } else if let Some(quote) = rest.chars().next().filter(|ch| matches!(ch, '"' | '\'')) {
            self.rest = &rest[1..];
            self.lex.mode = Mode::JsxQuote(quote);
        } else {
            // A name, an `=`, or a `/` that isn't a `/>`.
            let end = rest
                .char_indices()
                .skip(1)
                .find(|&(_, ch)| {
                    ch.is_whitespace() || matches!(ch, '/' | '<' | '>' | '{' | '"' | '\'')
                })
                .map_or(rest.len(), |(at, _)| at);
            self.rest = &rest[end..];
        }
    }
}

/// `rest` past the spaces and `/* */` comments at its start that don't end
/// the line.
fn inline_trivia_end(mut rest: &str) -> &str {
    loop {
        rest = rest.trim_start_matches(|ch: char| ch.is_whitespace() && !is_line_terminator(ch));
        let Some(comment) = rest.strip_prefix("/*") else {
            return rest;
        };
        match comment.find("*/") {
            Some(end) if !comment[..end].contains(is_line_terminator) => {
                rest = &comment[end + 2..];
            }
            _ => return rest,
        }
    }
}

/// The most lines looked at past the one a `<` is on, to tell a JSX
/// element from type parameters.
const JSX_LOOKAHEAD: usize = 4;

/// Text looked ahead at, past comments and line ends.
struct Lookahead<'a, 'l> {
    lines: &'l [&'a str],
    rest: &'a str,
}

impl<'a> Lookahead<'a, '_> {
    /// The rest from what is next, past spaces, comments and line ends;
    /// empty if nothing is, in the lines given.
    fn next(&mut self) -> &'a str {
        loop {
            self.rest = self.rest.trim_start();
            if let Some(comment) = self.rest.strip_prefix("/*") {
                self.rest = comment;
                while !self.rest.contains("*/") {
                    let Some((line, lines)) = self.lines.split_first() else {
                        self.rest = "";
                        return "";
                    };
                    (self.rest, self.lines) = (line, lines);
                }
                let end = self.rest.find("*/").unwrap_or(0);
                self.rest = &self.rest[end + 2..];
                continue;
            }
            if let Some(comment) = self.rest.strip_prefix("//") {
                self.rest = comment
                    .find(is_line_terminator)
                    .map_or("", |end| &comment[end..]);
                if !self.rest.is_empty() {
                    continue;
                }
            }
            if !self.rest.is_empty() {
                return self.rest;
            }
            let Some((line, lines)) = self.lines.split_first() else {
                return "";
            };
            (self.rest, self.lines) = (line, lines);
        }
    }

    /// The name that is next, if any, past it.
    fn word(&mut self) -> &'a str {
        let rest = self.next();
        let end = ident_end(rest);
        self.rest = &rest[end..];
        &rest[..end]
    }
}

/// Whether TypeScript's declaration keyword `word` begins a declaration,
/// with `rest` after it: its name, or a module's string, or a global's
/// body, follows on its line, comments aside. (Otherwise the keyword is a
/// name.)
fn ts_declaration_follows(word: &str, rest: &str) -> bool {
    let rest = inline_trivia_end(rest);
    match word {
        "global" => rest.starts_with('{'),
        "module" if rest.starts_with(['\'', '"']) => true,
        _ => ident_end(rest) > 0,
    }
}

impl<'a> Iterator for Tokens<'a, '_> {
    type Item = (Token<'a>, usize);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.step() {
                Step::Token(token, line) => return Some((token, line)),
                Step::Skipped | Step::Line => {}
                Step::End => return None,
            }
        }
    }
}

/// The lexical state each of `lines` begins in. A scan skips the lines that
/// have no `import` or `export`, and restarts after each statement, so it
/// can't learn this as it goes. Stops early if `poll` says to.
fn line_states(lines: &[&str], jsx: bool, poll: &Poll<'_>) -> Vec<Lex> {
    let mut states = vec![Lex::start(jsx)];
    let mut tokens = Tokens::at(lines, 0, Lex::start(jsx), poll);
    loop {
        match tokens.step() {
            Step::Line => states.push(tokens.lex),
            Step::Skipped | Step::Token(..) => {}
            Step::End => return states,
        }
    }
}

/// How a string ends, on the line it's on.
enum Ending {
    /// At a closing quote, at this byte of what was searched.
    Closed(usize),
    /// A `\` ends the line: the string goes on over the line end.
    Continued,
    /// It's not closed.
    Open,
}

/// How the string that `body` is the inside of, or the rest of, ends.
fn string_end(body: &str, quote: char) -> Ending {
    let mut chars = body.char_indices();
    while let Some((at, ch)) = chars.next() {
        if ch == '\\' {
            if chars.next().is_none() {
                return Ending::Continued;
            }
        } else if ch == quote {
            return Ending::Closed(at);
        }
    }
    Ending::Open
}

/// What ends a run of template text.
enum TemplateEnd {
    /// A closing backtick, at this byte.
    Closed(usize),
    /// A `${`, at this byte.
    Substitution(usize),
    /// Neither, on this line: the text goes on.
    Open,
}

/// How the template text in `body` ends.
fn template_text(body: &str) -> TemplateEnd {
    let mut chars = body.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            '`' => return TemplateEnd::Closed(at),
            '$' if chars.peek().is_some_and(|&(_, next)| next == '{') => {
                return TemplateEnd::Substitution(at);
            }
            _ => {}
        }
    }
    TemplateEnd::Open
}

/// Where the regular expression literal that `rest` starts with ends,
/// flags included, if it's closed on this line.
fn regex_end(rest: &str) -> Option<usize> {
    let mut in_class = false;
    let mut chars = rest.char_indices().skip(1);
    while let Some((at, ch)) = chars.next() {
        match ch {
            '\\' => {
                chars.next();
            }
            ch if is_line_terminator(ch) => return None,
            '[' => in_class = true,
            ']' => in_class = false,
            '/' if !in_class => {
                let flags = rest[at + 1..]
                    .find(|ch| !is_word_char(ch))
                    .unwrap_or(rest.len() - at - 1);
                return Some(at + 1 + flags);
            }
            _ => {}
        }
    }
    None
}

/// The length of the numeric literal `rest` starts with: digits, letters
/// (`0x1f`, `10n`, `1e5`), `_` separators and `.`, and the sign of an
/// exponent.
fn number_end(rest: &str) -> usize {
    let bytes = rest.as_bytes();
    let radix = bytes.len() > 1
        && bytes[0] == b'0'
        && matches!(bytes[1], b'x' | b'X' | b'b' | b'B' | b'o' | b'O');
    let mut end = 0;
    while let Some(&byte) = bytes.get(end) {
        if !(byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.')) {
            break;
        }
        end += 1;
        if !radix && matches!(byte, b'e' | b'E') && matches!(bytes.get(end), Some(b'+' | b'-')) {
            end += 1;
        }
    }
    end
}

/// The length of the identifier `rest` starts with, by ECMAScript's rules:
/// `$`, `_` and ID_Start chars, then ID_Continue chars (combining marks,
/// digits, joiners), any of them written as a Unicode escape (`a`,
/// `\u{61}`) of such a char. 0 if it doesn't start with one.
fn ident_end(rest: &str) -> usize {
    let mut end = 0;
    while let Some(first) = rest[end..].chars().next() {
        let (len, ch) = if first == '\\' {
            match unicode_escape(&rest[end..]) {
                Some(escaped) => escaped,
                None => break,
            }
        } else {
            (first.len_utf8(), first)
        };
        let fits = if end == 0 {
            is_id_start(ch)
        } else {
            is_id_continue(ch)
        };
        if !fits {
            break;
        }
        end += len;
    }
    end
}

/// The length and char of the `\uXXXX` or `\u{X…}` that `rest` starts
/// with; not a surrogate or past U+10FFFF.
fn unicode_escape(rest: &str) -> Option<(usize, char)> {
    let body = rest.strip_prefix("\\u")?;
    let (digits, len) = match body.strip_prefix('{') {
        Some(braced) => {
            // Only hex digits, so a line of `\u{` takes linear time.
            let close = braced
                .find(|ch: char| !ch.is_ascii_hexdigit())
                .filter(|&at| braced[at..].starts_with('}'))?;
            (&braced[..close], 2 + 1 + close + 1)
        }
        None => (body.get(..4)?, 2 + 4),
    };
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let ch = char::from_u32(u32::from_str_radix(digits, 16).ok()?)?;
    Some((len, ch))
}

fn is_id_start(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_alphabetic() || matches!(ch, '$' | '_')
    } else {
        in_ranges(ID_START, ch)
    }
}

fn is_id_continue(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_alphanumeric() || matches!(ch, '$' | '_')
    } else {
        in_ranges(ID_CONTINUE, ch)
    }
}

fn in_ranges(ranges: &[(u32, u32)], ch: char) -> bool {
    let ch = ch as u32;
    ranges
        .binary_search_by(|&(first, last)| {
            if ch < first {
                std::cmp::Ordering::Greater
            } else if ch > last {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

// Non-ASCII chars with Unicode's ID_Start and ID_Continue properties (15.1),
// as ECMAScript uses them for identifiers; ID_Continue with the joiners
// ZWNJ and ZWJ.
#[rustfmt::skip]
const ID_START: &[(u32, u32)] = &[
    (0xAA, 0xAA), (0xB5, 0xB5), (0xBA, 0xBA), (0xC0, 0xD6), (0xD8, 0xF6), (0xF8, 0x2C1),
    (0x2C6, 0x2D1), (0x2E0, 0x2E4), (0x2EC, 0x2EC), (0x2EE, 0x2EE), (0x370, 0x374),
    (0x376, 0x377), (0x37A, 0x37D), (0x37F, 0x37F), (0x386, 0x386), (0x388, 0x38A),
    (0x38C, 0x38C), (0x38E, 0x3A1), (0x3A3, 0x3F5), (0x3F7, 0x481), (0x48A, 0x52F),
    (0x531, 0x556), (0x559, 0x559), (0x560, 0x588), (0x5D0, 0x5EA), (0x5EF, 0x5F2),
    (0x620, 0x64A), (0x66E, 0x66F), (0x671, 0x6D3), (0x6D5, 0x6D5), (0x6E5, 0x6E6),
    (0x6EE, 0x6EF), (0x6FA, 0x6FC), (0x6FF, 0x6FF), (0x710, 0x710), (0x712, 0x72F),
    (0x74D, 0x7A5), (0x7B1, 0x7B1), (0x7CA, 0x7EA), (0x7F4, 0x7F5), (0x7FA, 0x7FA),
    (0x800, 0x815), (0x81A, 0x81A), (0x824, 0x824), (0x828, 0x828), (0x840, 0x858),
    (0x860, 0x86A), (0x870, 0x887), (0x889, 0x88E), (0x8A0, 0x8C9), (0x904, 0x939),
    (0x93D, 0x93D), (0x950, 0x950), (0x958, 0x961), (0x971, 0x980), (0x985, 0x98C),
    (0x98F, 0x990), (0x993, 0x9A8), (0x9AA, 0x9B0), (0x9B2, 0x9B2), (0x9B6, 0x9B9),
    (0x9BD, 0x9BD), (0x9CE, 0x9CE), (0x9DC, 0x9DD), (0x9DF, 0x9E1), (0x9F0, 0x9F1),
    (0x9FC, 0x9FC), (0xA05, 0xA0A), (0xA0F, 0xA10), (0xA13, 0xA28), (0xA2A, 0xA30),
    (0xA32, 0xA33), (0xA35, 0xA36), (0xA38, 0xA39), (0xA59, 0xA5C), (0xA5E, 0xA5E),
    (0xA72, 0xA74), (0xA85, 0xA8D), (0xA8F, 0xA91), (0xA93, 0xAA8), (0xAAA, 0xAB0),
    (0xAB2, 0xAB3), (0xAB5, 0xAB9), (0xABD, 0xABD), (0xAD0, 0xAD0), (0xAE0, 0xAE1),
    (0xAF9, 0xAF9), (0xB05, 0xB0C), (0xB0F, 0xB10), (0xB13, 0xB28), (0xB2A, 0xB30),
    (0xB32, 0xB33), (0xB35, 0xB39), (0xB3D, 0xB3D), (0xB5C, 0xB5D), (0xB5F, 0xB61),
    (0xB71, 0xB71), (0xB83, 0xB83), (0xB85, 0xB8A), (0xB8E, 0xB90), (0xB92, 0xB95),
    (0xB99, 0xB9A), (0xB9C, 0xB9C), (0xB9E, 0xB9F), (0xBA3, 0xBA4), (0xBA8, 0xBAA),
    (0xBAE, 0xBB9), (0xBD0, 0xBD0), (0xC05, 0xC0C), (0xC0E, 0xC10), (0xC12, 0xC28),
    (0xC2A, 0xC39), (0xC3D, 0xC3D), (0xC58, 0xC5A), (0xC5D, 0xC5D), (0xC60, 0xC61),
    (0xC80, 0xC80), (0xC85, 0xC8C), (0xC8E, 0xC90), (0xC92, 0xCA8), (0xCAA, 0xCB3),
    (0xCB5, 0xCB9), (0xCBD, 0xCBD), (0xCDD, 0xCDE), (0xCE0, 0xCE1), (0xCF1, 0xCF2),
    (0xD04, 0xD0C), (0xD0E, 0xD10), (0xD12, 0xD3A), (0xD3D, 0xD3D), (0xD4E, 0xD4E),
    (0xD54, 0xD56), (0xD5F, 0xD61), (0xD7A, 0xD7F), (0xD85, 0xD96), (0xD9A, 0xDB1),
    (0xDB3, 0xDBB), (0xDBD, 0xDBD), (0xDC0, 0xDC6), (0xE01, 0xE30), (0xE32, 0xE33),
    (0xE40, 0xE46), (0xE81, 0xE82), (0xE84, 0xE84), (0xE86, 0xE8A), (0xE8C, 0xEA3),
    (0xEA5, 0xEA5), (0xEA7, 0xEB0), (0xEB2, 0xEB3), (0xEBD, 0xEBD), (0xEC0, 0xEC4),
    (0xEC6, 0xEC6), (0xEDC, 0xEDF), (0xF00, 0xF00), (0xF40, 0xF47), (0xF49, 0xF6C),
    (0xF88, 0xF8C), (0x1000, 0x102A), (0x103F, 0x103F), (0x1050, 0x1055), (0x105A, 0x105D),
    (0x1061, 0x1061), (0x1065, 0x1066), (0x106E, 0x1070), (0x1075, 0x1081), (0x108E, 0x108E),
    (0x10A0, 0x10C5), (0x10C7, 0x10C7), (0x10CD, 0x10CD), (0x10D0, 0x10FA), (0x10FC, 0x1248),
    (0x124A, 0x124D), (0x1250, 0x1256), (0x1258, 0x1258), (0x125A, 0x125D), (0x1260, 0x1288),
    (0x128A, 0x128D), (0x1290, 0x12B0), (0x12B2, 0x12B5), (0x12B8, 0x12BE), (0x12C0, 0x12C0),
    (0x12C2, 0x12C5), (0x12C8, 0x12D6), (0x12D8, 0x1310), (0x1312, 0x1315), (0x1318, 0x135A),
    (0x1380, 0x138F), (0x13A0, 0x13F5), (0x13F8, 0x13FD), (0x1401, 0x166C), (0x166F, 0x167F),
    (0x1681, 0x169A), (0x16A0, 0x16EA), (0x16EE, 0x16F8), (0x1700, 0x1711), (0x171F, 0x1731),
    (0x1740, 0x1751), (0x1760, 0x176C), (0x176E, 0x1770), (0x1780, 0x17B3), (0x17D7, 0x17D7),
    (0x17DC, 0x17DC), (0x1820, 0x1878), (0x1880, 0x18A8), (0x18AA, 0x18AA), (0x18B0, 0x18F5),
    (0x1900, 0x191E), (0x1950, 0x196D), (0x1970, 0x1974), (0x1980, 0x19AB), (0x19B0, 0x19C9),
    (0x1A00, 0x1A16), (0x1A20, 0x1A54), (0x1AA7, 0x1AA7), (0x1B05, 0x1B33), (0x1B45, 0x1B4C),
    (0x1B83, 0x1BA0), (0x1BAE, 0x1BAF), (0x1BBA, 0x1BE5), (0x1C00, 0x1C23), (0x1C4D, 0x1C4F),
    (0x1C5A, 0x1C7D), (0x1C80, 0x1C88), (0x1C90, 0x1CBA), (0x1CBD, 0x1CBF), (0x1CE9, 0x1CEC),
    (0x1CEE, 0x1CF3), (0x1CF5, 0x1CF6), (0x1CFA, 0x1CFA), (0x1D00, 0x1DBF), (0x1E00, 0x1F15),
    (0x1F18, 0x1F1D), (0x1F20, 0x1F45), (0x1F48, 0x1F4D), (0x1F50, 0x1F57), (0x1F59, 0x1F59),
    (0x1F5B, 0x1F5B), (0x1F5D, 0x1F5D), (0x1F5F, 0x1F7D), (0x1F80, 0x1FB4), (0x1FB6, 0x1FBC),
    (0x1FBE, 0x1FBE), (0x1FC2, 0x1FC4), (0x1FC6, 0x1FCC), (0x1FD0, 0x1FD3), (0x1FD6, 0x1FDB),
    (0x1FE0, 0x1FEC), (0x1FF2, 0x1FF4), (0x1FF6, 0x1FFC), (0x2071, 0x2071), (0x207F, 0x207F),
    (0x2090, 0x209C), (0x2102, 0x2102), (0x2107, 0x2107), (0x210A, 0x2113), (0x2115, 0x2115),
    (0x2118, 0x211D), (0x2124, 0x2124), (0x2126, 0x2126), (0x2128, 0x2128), (0x212A, 0x2139),
    (0x213C, 0x213F), (0x2145, 0x2149), (0x214E, 0x214E), (0x2160, 0x2188), (0x2C00, 0x2CE4),
    (0x2CEB, 0x2CEE), (0x2CF2, 0x2CF3), (0x2D00, 0x2D25), (0x2D27, 0x2D27), (0x2D2D, 0x2D2D),
    (0x2D30, 0x2D67), (0x2D6F, 0x2D6F), (0x2D80, 0x2D96), (0x2DA0, 0x2DA6), (0x2DA8, 0x2DAE),
    (0x2DB0, 0x2DB6), (0x2DB8, 0x2DBE), (0x2DC0, 0x2DC6), (0x2DC8, 0x2DCE), (0x2DD0, 0x2DD6),
    (0x2DD8, 0x2DDE), (0x3005, 0x3007), (0x3021, 0x3029), (0x3031, 0x3035), (0x3038, 0x303C),
    (0x3041, 0x3096), (0x309B, 0x309F), (0x30A1, 0x30FA), (0x30FC, 0x30FF), (0x3105, 0x312F),
    (0x3131, 0x318E), (0x31A0, 0x31BF), (0x31F0, 0x31FF), (0x3400, 0x4DBF), (0x4E00, 0xA48C),
    (0xA4D0, 0xA4FD), (0xA500, 0xA60C), (0xA610, 0xA61F), (0xA62A, 0xA62B), (0xA640, 0xA66E),
    (0xA67F, 0xA69D), (0xA6A0, 0xA6EF), (0xA717, 0xA71F), (0xA722, 0xA788), (0xA78B, 0xA7CA),
    (0xA7D0, 0xA7D1), (0xA7D3, 0xA7D3), (0xA7D5, 0xA7D9), (0xA7F2, 0xA801), (0xA803, 0xA805),
    (0xA807, 0xA80A), (0xA80C, 0xA822), (0xA840, 0xA873), (0xA882, 0xA8B3), (0xA8F2, 0xA8F7),
    (0xA8FB, 0xA8FB), (0xA8FD, 0xA8FE), (0xA90A, 0xA925), (0xA930, 0xA946), (0xA960, 0xA97C),
    (0xA984, 0xA9B2), (0xA9CF, 0xA9CF), (0xA9E0, 0xA9E4), (0xA9E6, 0xA9EF), (0xA9FA, 0xA9FE),
    (0xAA00, 0xAA28), (0xAA40, 0xAA42), (0xAA44, 0xAA4B), (0xAA60, 0xAA76), (0xAA7A, 0xAA7A),
    (0xAA7E, 0xAAAF), (0xAAB1, 0xAAB1), (0xAAB5, 0xAAB6), (0xAAB9, 0xAABD), (0xAAC0, 0xAAC0),
    (0xAAC2, 0xAAC2), (0xAADB, 0xAADD), (0xAAE0, 0xAAEA), (0xAAF2, 0xAAF4), (0xAB01, 0xAB06),
    (0xAB09, 0xAB0E), (0xAB11, 0xAB16), (0xAB20, 0xAB26), (0xAB28, 0xAB2E), (0xAB30, 0xAB5A),
    (0xAB5C, 0xAB69), (0xAB70, 0xABE2), (0xAC00, 0xD7A3), (0xD7B0, 0xD7C6), (0xD7CB, 0xD7FB),
    (0xF900, 0xFA6D), (0xFA70, 0xFAD9), (0xFB00, 0xFB06), (0xFB13, 0xFB17), (0xFB1D, 0xFB1D),
    (0xFB1F, 0xFB28), (0xFB2A, 0xFB36), (0xFB38, 0xFB3C), (0xFB3E, 0xFB3E), (0xFB40, 0xFB41),
    (0xFB43, 0xFB44), (0xFB46, 0xFBB1), (0xFBD3, 0xFD3D), (0xFD50, 0xFD8F), (0xFD92, 0xFDC7),
    (0xFDF0, 0xFDFB), (0xFE70, 0xFE74), (0xFE76, 0xFEFC), (0xFF21, 0xFF3A), (0xFF41, 0xFF5A),
    (0xFF66, 0xFFBE), (0xFFC2, 0xFFC7), (0xFFCA, 0xFFCF), (0xFFD2, 0xFFD7), (0xFFDA, 0xFFDC),
    (0x10000, 0x1000B), (0x1000D, 0x10026), (0x10028, 0x1003A), (0x1003C, 0x1003D),
    (0x1003F, 0x1004D), (0x10050, 0x1005D), (0x10080, 0x100FA), (0x10140, 0x10174),
    (0x10280, 0x1029C), (0x102A0, 0x102D0), (0x10300, 0x1031F), (0x1032D, 0x1034A),
    (0x10350, 0x10375), (0x10380, 0x1039D), (0x103A0, 0x103C3), (0x103C8, 0x103CF),
    (0x103D1, 0x103D5), (0x10400, 0x1049D), (0x104B0, 0x104D3), (0x104D8, 0x104FB),
    (0x10500, 0x10527), (0x10530, 0x10563), (0x10570, 0x1057A), (0x1057C, 0x1058A),
    (0x1058C, 0x10592), (0x10594, 0x10595), (0x10597, 0x105A1), (0x105A3, 0x105B1),
    (0x105B3, 0x105B9), (0x105BB, 0x105BC), (0x10600, 0x10736), (0x10740, 0x10755),
    (0x10760, 0x10767), (0x10780, 0x10785), (0x10787, 0x107B0), (0x107B2, 0x107BA),
    (0x10800, 0x10805), (0x10808, 0x10808), (0x1080A, 0x10835), (0x10837, 0x10838),
    (0x1083C, 0x1083C), (0x1083F, 0x10855), (0x10860, 0x10876), (0x10880, 0x1089E),
    (0x108E0, 0x108F2), (0x108F4, 0x108F5), (0x10900, 0x10915), (0x10920, 0x10939),
    (0x10980, 0x109B7), (0x109BE, 0x109BF), (0x10A00, 0x10A00), (0x10A10, 0x10A13),
    (0x10A15, 0x10A17), (0x10A19, 0x10A35), (0x10A60, 0x10A7C), (0x10A80, 0x10A9C),
    (0x10AC0, 0x10AC7), (0x10AC9, 0x10AE4), (0x10B00, 0x10B35), (0x10B40, 0x10B55),
    (0x10B60, 0x10B72), (0x10B80, 0x10B91), (0x10C00, 0x10C48), (0x10C80, 0x10CB2),
    (0x10CC0, 0x10CF2), (0x10D00, 0x10D23), (0x10E80, 0x10EA9), (0x10EB0, 0x10EB1),
    (0x10F00, 0x10F1C), (0x10F27, 0x10F27), (0x10F30, 0x10F45), (0x10F70, 0x10F81),
    (0x10FB0, 0x10FC4), (0x10FE0, 0x10FF6), (0x11003, 0x11037), (0x11071, 0x11072),
    (0x11075, 0x11075), (0x11083, 0x110AF), (0x110D0, 0x110E8), (0x11103, 0x11126),
    (0x11144, 0x11144), (0x11147, 0x11147), (0x11150, 0x11172), (0x11176, 0x11176),
    (0x11183, 0x111B2), (0x111C1, 0x111C4), (0x111DA, 0x111DA), (0x111DC, 0x111DC),
    (0x11200, 0x11211), (0x11213, 0x1122B), (0x1123F, 0x11240), (0x11280, 0x11286),
    (0x11288, 0x11288), (0x1128A, 0x1128D), (0x1128F, 0x1129D), (0x1129F, 0x112A8),
    (0x112B0, 0x112DE), (0x11305, 0x1130C), (0x1130F, 0x11310), (0x11313, 0x11328),
    (0x1132A, 0x11330), (0x11332, 0x11333), (0x11335, 0x11339), (0x1133D, 0x1133D),
    (0x11350, 0x11350), (0x1135D, 0x11361), (0x11400, 0x11434), (0x11447, 0x1144A),
    (0x1145F, 0x11461), (0x11480, 0x114AF), (0x114C4, 0x114C5), (0x114C7, 0x114C7),
    (0x11580, 0x115AE), (0x115D8, 0x115DB), (0x11600, 0x1162F), (0x11644, 0x11644),
    (0x11680, 0x116AA), (0x116B8, 0x116B8), (0x11700, 0x1171A), (0x11740, 0x11746),
    (0x11800, 0x1182B), (0x118A0, 0x118DF), (0x118FF, 0x11906), (0x11909, 0x11909),
    (0x1190C, 0x11913), (0x11915, 0x11916), (0x11918, 0x1192F), (0x1193F, 0x1193F),
    (0x11941, 0x11941), (0x119A0, 0x119A7), (0x119AA, 0x119D0), (0x119E1, 0x119E1),
    (0x119E3, 0x119E3), (0x11A00, 0x11A00), (0x11A0B, 0x11A32), (0x11A3A, 0x11A3A),
    (0x11A50, 0x11A50), (0x11A5C, 0x11A89), (0x11A9D, 0x11A9D), (0x11AB0, 0x11AF8),
    (0x11C00, 0x11C08), (0x11C0A, 0x11C2E), (0x11C40, 0x11C40), (0x11C72, 0x11C8F),
    (0x11D00, 0x11D06), (0x11D08, 0x11D09), (0x11D0B, 0x11D30), (0x11D46, 0x11D46),
    (0x11D60, 0x11D65), (0x11D67, 0x11D68), (0x11D6A, 0x11D89), (0x11D98, 0x11D98),
    (0x11EE0, 0x11EF2), (0x11F02, 0x11F02), (0x11F04, 0x11F10), (0x11F12, 0x11F33),
    (0x11FB0, 0x11FB0), (0x12000, 0x12399), (0x12400, 0x1246E), (0x12480, 0x12543),
    (0x12F90, 0x12FF0), (0x13000, 0x1342F), (0x13441, 0x13446), (0x14400, 0x14646),
    (0x16800, 0x16A38), (0x16A40, 0x16A5E), (0x16A70, 0x16ABE), (0x16AD0, 0x16AED),
    (0x16B00, 0x16B2F), (0x16B40, 0x16B43), (0x16B63, 0x16B77), (0x16B7D, 0x16B8F),
    (0x16E40, 0x16E7F), (0x16F00, 0x16F4A), (0x16F50, 0x16F50), (0x16F93, 0x16F9F),
    (0x16FE0, 0x16FE1), (0x16FE3, 0x16FE3), (0x17000, 0x187F7), (0x18800, 0x18CD5),
    (0x18D00, 0x18D08), (0x1AFF0, 0x1AFF3), (0x1AFF5, 0x1AFFB), (0x1AFFD, 0x1AFFE),
    (0x1B000, 0x1B122), (0x1B132, 0x1B132), (0x1B150, 0x1B152), (0x1B155, 0x1B155),
    (0x1B164, 0x1B167), (0x1B170, 0x1B2FB), (0x1BC00, 0x1BC6A), (0x1BC70, 0x1BC7C),
    (0x1BC80, 0x1BC88), (0x1BC90, 0x1BC99), (0x1D400, 0x1D454), (0x1D456, 0x1D49C),
    (0x1D49E, 0x1D49F), (0x1D4A2, 0x1D4A2), (0x1D4A5, 0x1D4A6), (0x1D4A9, 0x1D4AC),
    (0x1D4AE, 0x1D4B9), (0x1D4BB, 0x1D4BB), (0x1D4BD, 0x1D4C3), (0x1D4C5, 0x1D505),
    (0x1D507, 0x1D50A), (0x1D50D, 0x1D514), (0x1D516, 0x1D51C), (0x1D51E, 0x1D539),
    (0x1D53B, 0x1D53E), (0x1D540, 0x1D544), (0x1D546, 0x1D546), (0x1D54A, 0x1D550),
    (0x1D552, 0x1D6A5), (0x1D6A8, 0x1D6C0), (0x1D6C2, 0x1D6DA), (0x1D6DC, 0x1D6FA),
    (0x1D6FC, 0x1D714), (0x1D716, 0x1D734), (0x1D736, 0x1D74E), (0x1D750, 0x1D76E),
    (0x1D770, 0x1D788), (0x1D78A, 0x1D7A8), (0x1D7AA, 0x1D7C2), (0x1D7C4, 0x1D7CB),
    (0x1DF00, 0x1DF1E), (0x1DF25, 0x1DF2A), (0x1E030, 0x1E06D), (0x1E100, 0x1E12C),
    (0x1E137, 0x1E13D), (0x1E14E, 0x1E14E), (0x1E290, 0x1E2AD), (0x1E2C0, 0x1E2EB),
    (0x1E4D0, 0x1E4EB), (0x1E7E0, 0x1E7E6), (0x1E7E8, 0x1E7EB), (0x1E7ED, 0x1E7EE),
    (0x1E7F0, 0x1E7FE), (0x1E800, 0x1E8C4), (0x1E900, 0x1E943), (0x1E94B, 0x1E94B),
    (0x1EE00, 0x1EE03), (0x1EE05, 0x1EE1F), (0x1EE21, 0x1EE22), (0x1EE24, 0x1EE24),
    (0x1EE27, 0x1EE27), (0x1EE29, 0x1EE32), (0x1EE34, 0x1EE37), (0x1EE39, 0x1EE39),
    (0x1EE3B, 0x1EE3B), (0x1EE42, 0x1EE42), (0x1EE47, 0x1EE47), (0x1EE49, 0x1EE49),
    (0x1EE4B, 0x1EE4B), (0x1EE4D, 0x1EE4F), (0x1EE51, 0x1EE52), (0x1EE54, 0x1EE54),
    (0x1EE57, 0x1EE57), (0x1EE59, 0x1EE59), (0x1EE5B, 0x1EE5B), (0x1EE5D, 0x1EE5D),
    (0x1EE5F, 0x1EE5F), (0x1EE61, 0x1EE62), (0x1EE64, 0x1EE64), (0x1EE67, 0x1EE6A),
    (0x1EE6C, 0x1EE72), (0x1EE74, 0x1EE77), (0x1EE79, 0x1EE7C), (0x1EE7E, 0x1EE7E),
    (0x1EE80, 0x1EE89), (0x1EE8B, 0x1EE9B), (0x1EEA1, 0x1EEA3), (0x1EEA5, 0x1EEA9),
    (0x1EEAB, 0x1EEBB), (0x20000, 0x2A6DF), (0x2A700, 0x2B739), (0x2B740, 0x2B81D),
    (0x2B820, 0x2CEA1), (0x2CEB0, 0x2EBE0), (0x2EBF0, 0x2EE5D), (0x2F800, 0x2FA1D),
    (0x30000, 0x3134A), (0x31350, 0x323AF),
];

#[rustfmt::skip]
const ID_CONTINUE: &[(u32, u32)] = &[
    (0xAA, 0xAA), (0xB5, 0xB5), (0xB7, 0xB7), (0xBA, 0xBA), (0xC0, 0xD6), (0xD8, 0xF6),
    (0xF8, 0x2C1), (0x2C6, 0x2D1), (0x2E0, 0x2E4), (0x2EC, 0x2EC), (0x2EE, 0x2EE),
    (0x300, 0x374), (0x376, 0x377), (0x37A, 0x37D), (0x37F, 0x37F), (0x386, 0x38A),
    (0x38C, 0x38C), (0x38E, 0x3A1), (0x3A3, 0x3F5), (0x3F7, 0x481), (0x483, 0x487),
    (0x48A, 0x52F), (0x531, 0x556), (0x559, 0x559), (0x560, 0x588), (0x591, 0x5BD),
    (0x5BF, 0x5BF), (0x5C1, 0x5C2), (0x5C4, 0x5C5), (0x5C7, 0x5C7), (0x5D0, 0x5EA),
    (0x5EF, 0x5F2), (0x610, 0x61A), (0x620, 0x669), (0x66E, 0x6D3), (0x6D5, 0x6DC),
    (0x6DF, 0x6E8), (0x6EA, 0x6FC), (0x6FF, 0x6FF), (0x710, 0x74A), (0x74D, 0x7B1),
    (0x7C0, 0x7F5), (0x7FA, 0x7FA), (0x7FD, 0x7FD), (0x800, 0x82D), (0x840, 0x85B),
    (0x860, 0x86A), (0x870, 0x887), (0x889, 0x88E), (0x898, 0x8E1), (0x8E3, 0x963),
    (0x966, 0x96F), (0x971, 0x983), (0x985, 0x98C), (0x98F, 0x990), (0x993, 0x9A8),
    (0x9AA, 0x9B0), (0x9B2, 0x9B2), (0x9B6, 0x9B9), (0x9BC, 0x9C4), (0x9C7, 0x9C8),
    (0x9CB, 0x9CE), (0x9D7, 0x9D7), (0x9DC, 0x9DD), (0x9DF, 0x9E3), (0x9E6, 0x9F1),
    (0x9FC, 0x9FC), (0x9FE, 0x9FE), (0xA01, 0xA03), (0xA05, 0xA0A), (0xA0F, 0xA10),
    (0xA13, 0xA28), (0xA2A, 0xA30), (0xA32, 0xA33), (0xA35, 0xA36), (0xA38, 0xA39),
    (0xA3C, 0xA3C), (0xA3E, 0xA42), (0xA47, 0xA48), (0xA4B, 0xA4D), (0xA51, 0xA51),
    (0xA59, 0xA5C), (0xA5E, 0xA5E), (0xA66, 0xA75), (0xA81, 0xA83), (0xA85, 0xA8D),
    (0xA8F, 0xA91), (0xA93, 0xAA8), (0xAAA, 0xAB0), (0xAB2, 0xAB3), (0xAB5, 0xAB9),
    (0xABC, 0xAC5), (0xAC7, 0xAC9), (0xACB, 0xACD), (0xAD0, 0xAD0), (0xAE0, 0xAE3),
    (0xAE6, 0xAEF), (0xAF9, 0xAFF), (0xB01, 0xB03), (0xB05, 0xB0C), (0xB0F, 0xB10),
    (0xB13, 0xB28), (0xB2A, 0xB30), (0xB32, 0xB33), (0xB35, 0xB39), (0xB3C, 0xB44),
    (0xB47, 0xB48), (0xB4B, 0xB4D), (0xB55, 0xB57), (0xB5C, 0xB5D), (0xB5F, 0xB63),
    (0xB66, 0xB6F), (0xB71, 0xB71), (0xB82, 0xB83), (0xB85, 0xB8A), (0xB8E, 0xB90),
    (0xB92, 0xB95), (0xB99, 0xB9A), (0xB9C, 0xB9C), (0xB9E, 0xB9F), (0xBA3, 0xBA4),
    (0xBA8, 0xBAA), (0xBAE, 0xBB9), (0xBBE, 0xBC2), (0xBC6, 0xBC8), (0xBCA, 0xBCD),
    (0xBD0, 0xBD0), (0xBD7, 0xBD7), (0xBE6, 0xBEF), (0xC00, 0xC0C), (0xC0E, 0xC10),
    (0xC12, 0xC28), (0xC2A, 0xC39), (0xC3C, 0xC44), (0xC46, 0xC48), (0xC4A, 0xC4D),
    (0xC55, 0xC56), (0xC58, 0xC5A), (0xC5D, 0xC5D), (0xC60, 0xC63), (0xC66, 0xC6F),
    (0xC80, 0xC83), (0xC85, 0xC8C), (0xC8E, 0xC90), (0xC92, 0xCA8), (0xCAA, 0xCB3),
    (0xCB5, 0xCB9), (0xCBC, 0xCC4), (0xCC6, 0xCC8), (0xCCA, 0xCCD), (0xCD5, 0xCD6),
    (0xCDD, 0xCDE), (0xCE0, 0xCE3), (0xCE6, 0xCEF), (0xCF1, 0xCF3), (0xD00, 0xD0C),
    (0xD0E, 0xD10), (0xD12, 0xD44), (0xD46, 0xD48), (0xD4A, 0xD4E), (0xD54, 0xD57),
    (0xD5F, 0xD63), (0xD66, 0xD6F), (0xD7A, 0xD7F), (0xD81, 0xD83), (0xD85, 0xD96),
    (0xD9A, 0xDB1), (0xDB3, 0xDBB), (0xDBD, 0xDBD), (0xDC0, 0xDC6), (0xDCA, 0xDCA),
    (0xDCF, 0xDD4), (0xDD6, 0xDD6), (0xDD8, 0xDDF), (0xDE6, 0xDEF), (0xDF2, 0xDF3),
    (0xE01, 0xE3A), (0xE40, 0xE4E), (0xE50, 0xE59), (0xE81, 0xE82), (0xE84, 0xE84),
    (0xE86, 0xE8A), (0xE8C, 0xEA3), (0xEA5, 0xEA5), (0xEA7, 0xEBD), (0xEC0, 0xEC4),
    (0xEC6, 0xEC6), (0xEC8, 0xECE), (0xED0, 0xED9), (0xEDC, 0xEDF), (0xF00, 0xF00),
    (0xF18, 0xF19), (0xF20, 0xF29), (0xF35, 0xF35), (0xF37, 0xF37), (0xF39, 0xF39),
    (0xF3E, 0xF47), (0xF49, 0xF6C), (0xF71, 0xF84), (0xF86, 0xF97), (0xF99, 0xFBC),
    (0xFC6, 0xFC6), (0x1000, 0x1049), (0x1050, 0x109D), (0x10A0, 0x10C5), (0x10C7, 0x10C7),
    (0x10CD, 0x10CD), (0x10D0, 0x10FA), (0x10FC, 0x1248), (0x124A, 0x124D), (0x1250, 0x1256),
    (0x1258, 0x1258), (0x125A, 0x125D), (0x1260, 0x1288), (0x128A, 0x128D), (0x1290, 0x12B0),
    (0x12B2, 0x12B5), (0x12B8, 0x12BE), (0x12C0, 0x12C0), (0x12C2, 0x12C5), (0x12C8, 0x12D6),
    (0x12D8, 0x1310), (0x1312, 0x1315), (0x1318, 0x135A), (0x135D, 0x135F), (0x1369, 0x1371),
    (0x1380, 0x138F), (0x13A0, 0x13F5), (0x13F8, 0x13FD), (0x1401, 0x166C), (0x166F, 0x167F),
    (0x1681, 0x169A), (0x16A0, 0x16EA), (0x16EE, 0x16F8), (0x1700, 0x1715), (0x171F, 0x1734),
    (0x1740, 0x1753), (0x1760, 0x176C), (0x176E, 0x1770), (0x1772, 0x1773), (0x1780, 0x17D3),
    (0x17D7, 0x17D7), (0x17DC, 0x17DD), (0x17E0, 0x17E9), (0x180B, 0x180D), (0x180F, 0x1819),
    (0x1820, 0x1878), (0x1880, 0x18AA), (0x18B0, 0x18F5), (0x1900, 0x191E), (0x1920, 0x192B),
    (0x1930, 0x193B), (0x1946, 0x196D), (0x1970, 0x1974), (0x1980, 0x19AB), (0x19B0, 0x19C9),
    (0x19D0, 0x19DA), (0x1A00, 0x1A1B), (0x1A20, 0x1A5E), (0x1A60, 0x1A7C), (0x1A7F, 0x1A89),
    (0x1A90, 0x1A99), (0x1AA7, 0x1AA7), (0x1AB0, 0x1ABD), (0x1ABF, 0x1ACE), (0x1B00, 0x1B4C),
    (0x1B50, 0x1B59), (0x1B6B, 0x1B73), (0x1B80, 0x1BF3), (0x1C00, 0x1C37), (0x1C40, 0x1C49),
    (0x1C4D, 0x1C7D), (0x1C80, 0x1C88), (0x1C90, 0x1CBA), (0x1CBD, 0x1CBF), (0x1CD0, 0x1CD2),
    (0x1CD4, 0x1CFA), (0x1D00, 0x1F15), (0x1F18, 0x1F1D), (0x1F20, 0x1F45), (0x1F48, 0x1F4D),
    (0x1F50, 0x1F57), (0x1F59, 0x1F59), (0x1F5B, 0x1F5B), (0x1F5D, 0x1F5D), (0x1F5F, 0x1F7D),
    (0x1F80, 0x1FB4), (0x1FB6, 0x1FBC), (0x1FBE, 0x1FBE), (0x1FC2, 0x1FC4), (0x1FC6, 0x1FCC),
    (0x1FD0, 0x1FD3), (0x1FD6, 0x1FDB), (0x1FE0, 0x1FEC), (0x1FF2, 0x1FF4), (0x1FF6, 0x1FFC),
    (0x200C, 0x200D), (0x203F, 0x2040), (0x2054, 0x2054), (0x2071, 0x2071), (0x207F, 0x207F),
    (0x2090, 0x209C), (0x20D0, 0x20DC), (0x20E1, 0x20E1), (0x20E5, 0x20F0), (0x2102, 0x2102),
    (0x2107, 0x2107), (0x210A, 0x2113), (0x2115, 0x2115), (0x2118, 0x211D), (0x2124, 0x2124),
    (0x2126, 0x2126), (0x2128, 0x2128), (0x212A, 0x2139), (0x213C, 0x213F), (0x2145, 0x2149),
    (0x214E, 0x214E), (0x2160, 0x2188), (0x2C00, 0x2CE4), (0x2CEB, 0x2CF3), (0x2D00, 0x2D25),
    (0x2D27, 0x2D27), (0x2D2D, 0x2D2D), (0x2D30, 0x2D67), (0x2D6F, 0x2D6F), (0x2D7F, 0x2D96),
    (0x2DA0, 0x2DA6), (0x2DA8, 0x2DAE), (0x2DB0, 0x2DB6), (0x2DB8, 0x2DBE), (0x2DC0, 0x2DC6),
    (0x2DC8, 0x2DCE), (0x2DD0, 0x2DD6), (0x2DD8, 0x2DDE), (0x2DE0, 0x2DFF), (0x3005, 0x3007),
    (0x3021, 0x302F), (0x3031, 0x3035), (0x3038, 0x303C), (0x3041, 0x3096), (0x3099, 0x309F),
    (0x30A1, 0x30FF), (0x3105, 0x312F), (0x3131, 0x318E), (0x31A0, 0x31BF), (0x31F0, 0x31FF),
    (0x3400, 0x4DBF), (0x4E00, 0xA48C), (0xA4D0, 0xA4FD), (0xA500, 0xA60C), (0xA610, 0xA62B),
    (0xA640, 0xA66F), (0xA674, 0xA67D), (0xA67F, 0xA6F1), (0xA717, 0xA71F), (0xA722, 0xA788),
    (0xA78B, 0xA7CA), (0xA7D0, 0xA7D1), (0xA7D3, 0xA7D3), (0xA7D5, 0xA7D9), (0xA7F2, 0xA827),
    (0xA82C, 0xA82C), (0xA840, 0xA873), (0xA880, 0xA8C5), (0xA8D0, 0xA8D9), (0xA8E0, 0xA8F7),
    (0xA8FB, 0xA8FB), (0xA8FD, 0xA92D), (0xA930, 0xA953), (0xA960, 0xA97C), (0xA980, 0xA9C0),
    (0xA9CF, 0xA9D9), (0xA9E0, 0xA9FE), (0xAA00, 0xAA36), (0xAA40, 0xAA4D), (0xAA50, 0xAA59),
    (0xAA60, 0xAA76), (0xAA7A, 0xAAC2), (0xAADB, 0xAADD), (0xAAE0, 0xAAEF), (0xAAF2, 0xAAF6),
    (0xAB01, 0xAB06), (0xAB09, 0xAB0E), (0xAB11, 0xAB16), (0xAB20, 0xAB26), (0xAB28, 0xAB2E),
    (0xAB30, 0xAB5A), (0xAB5C, 0xAB69), (0xAB70, 0xABEA), (0xABEC, 0xABED), (0xABF0, 0xABF9),
    (0xAC00, 0xD7A3), (0xD7B0, 0xD7C6), (0xD7CB, 0xD7FB), (0xF900, 0xFA6D), (0xFA70, 0xFAD9),
    (0xFB00, 0xFB06), (0xFB13, 0xFB17), (0xFB1D, 0xFB28), (0xFB2A, 0xFB36), (0xFB38, 0xFB3C),
    (0xFB3E, 0xFB3E), (0xFB40, 0xFB41), (0xFB43, 0xFB44), (0xFB46, 0xFBB1), (0xFBD3, 0xFD3D),
    (0xFD50, 0xFD8F), (0xFD92, 0xFDC7), (0xFDF0, 0xFDFB), (0xFE00, 0xFE0F), (0xFE20, 0xFE2F),
    (0xFE33, 0xFE34), (0xFE4D, 0xFE4F), (0xFE70, 0xFE74), (0xFE76, 0xFEFC), (0xFF10, 0xFF19),
    (0xFF21, 0xFF3A), (0xFF3F, 0xFF3F), (0xFF41, 0xFF5A), (0xFF65, 0xFFBE), (0xFFC2, 0xFFC7),
    (0xFFCA, 0xFFCF), (0xFFD2, 0xFFD7), (0xFFDA, 0xFFDC), (0x10000, 0x1000B),
    (0x1000D, 0x10026), (0x10028, 0x1003A), (0x1003C, 0x1003D), (0x1003F, 0x1004D),
    (0x10050, 0x1005D), (0x10080, 0x100FA), (0x10140, 0x10174), (0x101FD, 0x101FD),
    (0x10280, 0x1029C), (0x102A0, 0x102D0), (0x102E0, 0x102E0), (0x10300, 0x1031F),
    (0x1032D, 0x1034A), (0x10350, 0x1037A), (0x10380, 0x1039D), (0x103A0, 0x103C3),
    (0x103C8, 0x103CF), (0x103D1, 0x103D5), (0x10400, 0x1049D), (0x104A0, 0x104A9),
    (0x104B0, 0x104D3), (0x104D8, 0x104FB), (0x10500, 0x10527), (0x10530, 0x10563),
    (0x10570, 0x1057A), (0x1057C, 0x1058A), (0x1058C, 0x10592), (0x10594, 0x10595),
    (0x10597, 0x105A1), (0x105A3, 0x105B1), (0x105B3, 0x105B9), (0x105BB, 0x105BC),
    (0x10600, 0x10736), (0x10740, 0x10755), (0x10760, 0x10767), (0x10780, 0x10785),
    (0x10787, 0x107B0), (0x107B2, 0x107BA), (0x10800, 0x10805), (0x10808, 0x10808),
    (0x1080A, 0x10835), (0x10837, 0x10838), (0x1083C, 0x1083C), (0x1083F, 0x10855),
    (0x10860, 0x10876), (0x10880, 0x1089E), (0x108E0, 0x108F2), (0x108F4, 0x108F5),
    (0x10900, 0x10915), (0x10920, 0x10939), (0x10980, 0x109B7), (0x109BE, 0x109BF),
    (0x10A00, 0x10A03), (0x10A05, 0x10A06), (0x10A0C, 0x10A13), (0x10A15, 0x10A17),
    (0x10A19, 0x10A35), (0x10A38, 0x10A3A), (0x10A3F, 0x10A3F), (0x10A60, 0x10A7C),
    (0x10A80, 0x10A9C), (0x10AC0, 0x10AC7), (0x10AC9, 0x10AE6), (0x10B00, 0x10B35),
    (0x10B40, 0x10B55), (0x10B60, 0x10B72), (0x10B80, 0x10B91), (0x10C00, 0x10C48),
    (0x10C80, 0x10CB2), (0x10CC0, 0x10CF2), (0x10D00, 0x10D27), (0x10D30, 0x10D39),
    (0x10E80, 0x10EA9), (0x10EAB, 0x10EAC), (0x10EB0, 0x10EB1), (0x10EFD, 0x10F1C),
    (0x10F27, 0x10F27), (0x10F30, 0x10F50), (0x10F70, 0x10F85), (0x10FB0, 0x10FC4),
    (0x10FE0, 0x10FF6), (0x11000, 0x11046), (0x11066, 0x11075), (0x1107F, 0x110BA),
    (0x110C2, 0x110C2), (0x110D0, 0x110E8), (0x110F0, 0x110F9), (0x11100, 0x11134),
    (0x11136, 0x1113F), (0x11144, 0x11147), (0x11150, 0x11173), (0x11176, 0x11176),
    (0x11180, 0x111C4), (0x111C9, 0x111CC), (0x111CE, 0x111DA), (0x111DC, 0x111DC),
    (0x11200, 0x11211), (0x11213, 0x11237), (0x1123E, 0x11241), (0x11280, 0x11286),
    (0x11288, 0x11288), (0x1128A, 0x1128D), (0x1128F, 0x1129D), (0x1129F, 0x112A8),
    (0x112B0, 0x112EA), (0x112F0, 0x112F9), (0x11300, 0x11303), (0x11305, 0x1130C),
    (0x1130F, 0x11310), (0x11313, 0x11328), (0x1132A, 0x11330), (0x11332, 0x11333),
    (0x11335, 0x11339), (0x1133B, 0x11344), (0x11347, 0x11348), (0x1134B, 0x1134D),
    (0x11350, 0x11350), (0x11357, 0x11357), (0x1135D, 0x11363), (0x11366, 0x1136C),
    (0x11370, 0x11374), (0x11400, 0x1144A), (0x11450, 0x11459), (0x1145E, 0x11461),
    (0x11480, 0x114C5), (0x114C7, 0x114C7), (0x114D0, 0x114D9), (0x11580, 0x115B5),
    (0x115B8, 0x115C0), (0x115D8, 0x115DD), (0x11600, 0x11640), (0x11644, 0x11644),
    (0x11650, 0x11659), (0x11680, 0x116B8), (0x116C0, 0x116C9), (0x11700, 0x1171A),
    (0x1171D, 0x1172B), (0x11730, 0x11739), (0x11740, 0x11746), (0x11800, 0x1183A),
    (0x118A0, 0x118E9), (0x118FF, 0x11906), (0x11909, 0x11909), (0x1190C, 0x11913),
    (0x11915, 0x11916), (0x11918, 0x11935), (0x11937, 0x11938), (0x1193B, 0x11943),
    (0x11950, 0x11959), (0x119A0, 0x119A7), (0x119AA, 0x119D7), (0x119DA, 0x119E1),
    (0x119E3, 0x119E4), (0x11A00, 0x11A3E), (0x11A47, 0x11A47), (0x11A50, 0x11A99),
    (0x11A9D, 0x11A9D), (0x11AB0, 0x11AF8), (0x11C00, 0x11C08), (0x11C0A, 0x11C36),
    (0x11C38, 0x11C40), (0x11C50, 0x11C59), (0x11C72, 0x11C8F), (0x11C92, 0x11CA7),
    (0x11CA9, 0x11CB6), (0x11D00, 0x11D06), (0x11D08, 0x11D09), (0x11D0B, 0x11D36),
    (0x11D3A, 0x11D3A), (0x11D3C, 0x11D3D), (0x11D3F, 0x11D47), (0x11D50, 0x11D59),
    (0x11D60, 0x11D65), (0x11D67, 0x11D68), (0x11D6A, 0x11D8E), (0x11D90, 0x11D91),
    (0x11D93, 0x11D98), (0x11DA0, 0x11DA9), (0x11EE0, 0x11EF6), (0x11F00, 0x11F10),
    (0x11F12, 0x11F3A), (0x11F3E, 0x11F42), (0x11F50, 0x11F59), (0x11FB0, 0x11FB0),
    (0x12000, 0x12399), (0x12400, 0x1246E), (0x12480, 0x12543), (0x12F90, 0x12FF0),
    (0x13000, 0x1342F), (0x13440, 0x13455), (0x14400, 0x14646), (0x16800, 0x16A38),
    (0x16A40, 0x16A5E), (0x16A60, 0x16A69), (0x16A70, 0x16ABE), (0x16AC0, 0x16AC9),
    (0x16AD0, 0x16AED), (0x16AF0, 0x16AF4), (0x16B00, 0x16B36), (0x16B40, 0x16B43),
    (0x16B50, 0x16B59), (0x16B63, 0x16B77), (0x16B7D, 0x16B8F), (0x16E40, 0x16E7F),
    (0x16F00, 0x16F4A), (0x16F4F, 0x16F87), (0x16F8F, 0x16F9F), (0x16FE0, 0x16FE1),
    (0x16FE3, 0x16FE4), (0x16FF0, 0x16FF1), (0x17000, 0x187F7), (0x18800, 0x18CD5),
    (0x18D00, 0x18D08), (0x1AFF0, 0x1AFF3), (0x1AFF5, 0x1AFFB), (0x1AFFD, 0x1AFFE),
    (0x1B000, 0x1B122), (0x1B132, 0x1B132), (0x1B150, 0x1B152), (0x1B155, 0x1B155),
    (0x1B164, 0x1B167), (0x1B170, 0x1B2FB), (0x1BC00, 0x1BC6A), (0x1BC70, 0x1BC7C),
    (0x1BC80, 0x1BC88), (0x1BC90, 0x1BC99), (0x1BC9D, 0x1BC9E), (0x1CF00, 0x1CF2D),
    (0x1CF30, 0x1CF46), (0x1D165, 0x1D169), (0x1D16D, 0x1D172), (0x1D17B, 0x1D182),
    (0x1D185, 0x1D18B), (0x1D1AA, 0x1D1AD), (0x1D242, 0x1D244), (0x1D400, 0x1D454),
    (0x1D456, 0x1D49C), (0x1D49E, 0x1D49F), (0x1D4A2, 0x1D4A2), (0x1D4A5, 0x1D4A6),
    (0x1D4A9, 0x1D4AC), (0x1D4AE, 0x1D4B9), (0x1D4BB, 0x1D4BB), (0x1D4BD, 0x1D4C3),
    (0x1D4C5, 0x1D505), (0x1D507, 0x1D50A), (0x1D50D, 0x1D514), (0x1D516, 0x1D51C),
    (0x1D51E, 0x1D539), (0x1D53B, 0x1D53E), (0x1D540, 0x1D544), (0x1D546, 0x1D546),
    (0x1D54A, 0x1D550), (0x1D552, 0x1D6A5), (0x1D6A8, 0x1D6C0), (0x1D6C2, 0x1D6DA),
    (0x1D6DC, 0x1D6FA), (0x1D6FC, 0x1D714), (0x1D716, 0x1D734), (0x1D736, 0x1D74E),
    (0x1D750, 0x1D76E), (0x1D770, 0x1D788), (0x1D78A, 0x1D7A8), (0x1D7AA, 0x1D7C2),
    (0x1D7C4, 0x1D7CB), (0x1D7CE, 0x1D7FF), (0x1DA00, 0x1DA36), (0x1DA3B, 0x1DA6C),
    (0x1DA75, 0x1DA75), (0x1DA84, 0x1DA84), (0x1DA9B, 0x1DA9F), (0x1DAA1, 0x1DAAF),
    (0x1DF00, 0x1DF1E), (0x1DF25, 0x1DF2A), (0x1E000, 0x1E006), (0x1E008, 0x1E018),
    (0x1E01B, 0x1E021), (0x1E023, 0x1E024), (0x1E026, 0x1E02A), (0x1E030, 0x1E06D),
    (0x1E08F, 0x1E08F), (0x1E100, 0x1E12C), (0x1E130, 0x1E13D), (0x1E140, 0x1E149),
    (0x1E14E, 0x1E14E), (0x1E290, 0x1E2AE), (0x1E2C0, 0x1E2F9), (0x1E4D0, 0x1E4F9),
    (0x1E7E0, 0x1E7E6), (0x1E7E8, 0x1E7EB), (0x1E7ED, 0x1E7EE), (0x1E7F0, 0x1E7FE),
    (0x1E800, 0x1E8C4), (0x1E8D0, 0x1E8D6), (0x1E900, 0x1E94B), (0x1E950, 0x1E959),
    (0x1EE00, 0x1EE03), (0x1EE05, 0x1EE1F), (0x1EE21, 0x1EE22), (0x1EE24, 0x1EE24),
    (0x1EE27, 0x1EE27), (0x1EE29, 0x1EE32), (0x1EE34, 0x1EE37), (0x1EE39, 0x1EE39),
    (0x1EE3B, 0x1EE3B), (0x1EE42, 0x1EE42), (0x1EE47, 0x1EE47), (0x1EE49, 0x1EE49),
    (0x1EE4B, 0x1EE4B), (0x1EE4D, 0x1EE4F), (0x1EE51, 0x1EE52), (0x1EE54, 0x1EE54),
    (0x1EE57, 0x1EE57), (0x1EE59, 0x1EE59), (0x1EE5B, 0x1EE5B), (0x1EE5D, 0x1EE5D),
    (0x1EE5F, 0x1EE5F), (0x1EE61, 0x1EE62), (0x1EE64, 0x1EE64), (0x1EE67, 0x1EE6A),
    (0x1EE6C, 0x1EE72), (0x1EE74, 0x1EE77), (0x1EE79, 0x1EE7C), (0x1EE7E, 0x1EE7E),
    (0x1EE80, 0x1EE89), (0x1EE8B, 0x1EE9B), (0x1EEA1, 0x1EEA3), (0x1EEA5, 0x1EEA9),
    (0x1EEAB, 0x1EEBB), (0x1FBF0, 0x1FBF9), (0x20000, 0x2A6DF), (0x2A700, 0x2B739),
    (0x2B740, 0x2B81D), (0x2B820, 0x2CEA1), (0x2CEB0, 0x2EBE0), (0x2EBF0, 0x2EE5D),
    (0x2F800, 0x2FA1D), (0x30000, 0x3134A), (0x31350, 0x323AF), (0xE0100, 0xE01EF),
];

/// The rest of `line` after the keyword `word`, without leading whitespace,
/// if `line` starts with it as a whole word.
fn keyword<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(word)?;
    (!rest.chars().next().is_some_and(is_word_char)).then(|| rest.trim_start())
}

/// `line` without a leading Rust visibility: `pub`, `pub(crate)`, ….
fn strip_visibility(line: &str) -> &str {
    let Some(rest) = keyword(line, "pub") else {
        return line;
    };
    match rest.strip_prefix('(') {
        Some(inner) => match inner.find(')') {
            Some(close) => inner[close + 1..].trim_start(),
            None => line,
        },
        None => rest,
    }
}

/// An identifier char in JavaScript, `$` included.
fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ch == '$'
}

#[cfg(test)]
#[path = "search_followup_tests.rs"]
mod followup_tests;

#[cfg(test)]
#[path = "search_import_tests.rs"]
mod import_tests;

#[cfg(test)]
mod tests {
    use std::sync::{Condvar, Mutex};

    use super::*;

    /// A project of `files` Rust files calling `other`.
    fn callers(files: usize) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for ix in 0..files {
            let path = dir.path().join(format!("many/m{ix:02}.rs"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "fn m() { other(); }\n").unwrap();
        }
        dir
    }

    fn unbuilt(root: &Path) -> SearchNavigation {
        SearchNavigation::new(root, Arc::new(RwLock::new(SymbolIndex::new(root))))
    }

    fn request(root: &Path, text: &str) -> NavRequest {
        NavRequest {
            root: root.to_path_buf(),
            rel: PathBuf::from("main.rs"),
            language: Some("rust"),
            text: Text::new(text),
            offset: 0,
        }
    }

    fn other(root: &Path) -> NavRequest {
        request(root, "other();\n")
    }

    /// `source`'s imports, uncancelled.
    pub(super) fn imports<'a>(language: &str, source: &'a str) -> Vec<&'a str> {
        import_lines(language, source, &AtomicBool::new(false)).unwrap()
    }

    /// Work that says when it starts, then waits to be let go.
    #[derive(Clone, Default)]
    struct Gate(Arc<(Mutex<(usize, bool)>, Condvar)>);

    impl Gate {
        fn pass(&self) {
            let (state, changed) = &*self.0;
            let mut state = state.lock().unwrap();
            state.0 += 1;
            changed.notify_all();
            drop(changed.wait_while(state, |state| !state.1));
        }

        fn extract(
            &self,
        ) -> impl Fn(&Path, &Path, &dyn Fn() -> bool) -> Result<FileSymbols, Skipped> + use<>
        {
            let gate = self.clone();
            move |_, _, _| {
                gate.pass();
                Err(Skipped::ParseFailed)
            }
        }

        fn wait_started(&self) {
            let (state, changed) = &*self.0;
            let state = state.lock().unwrap();
            drop(changed.wait_while(state, |state| state.0 == 0));
        }

        fn started(&self) -> usize {
            self.0.0.lock().unwrap().0
        }

        fn release(&self) {
            let (state, changed) = &*self.0;
            state.lock().unwrap().1 = true;
            changed.notify_all();
        }
    }

    /// Sets `cancel` once `gate` has started.
    fn cancel_when_started(gate: &Gate) -> (Arc<AtomicBool>, thread::JoinHandle<()>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let canceller = thread::spawn({
            let (gate, cancel) = (gate.clone(), cancel.clone());
            move || {
                gate.wait_started();
                cancel.store(true, Ordering::Relaxed);
            }
        });
        (cancel, canceller)
    }

    #[test]
    fn the_cap_counts_failed_extractions() {
        let dir = callers(80);
        let nav = unbuilt(dir.path())
            .with_budget(Duration::from_secs(60))
            .with_extract(|_, _, _| Err(Skipped::ParseFailed));
        let defs = nav
            .definitions(&other(dir.path()), &AtomicBool::new(false))
            .unwrap();
        assert!(defs.items.is_empty());
        assert!(defs.partial);
        assert_eq!(
            nav.stats(),
            SearchStats {
                searches: 1,
                files_extracted: 64,
            }
        );
    }

    #[test]
    fn the_deadline_doesnt_wait_for_an_extraction() {
        let dir = callers(2);
        let gate = Gate::default();
        let nav = unbuilt(dir.path())
            .with_budget(Duration::from_millis(300))
            .with_extract(gate.extract());
        let defs = nav
            .definitions(&other(dir.path()), &AtomicBool::new(false))
            .unwrap();
        // The extraction is still waiting, and the answer came anyway.
        assert_eq!(gate.started(), 1);
        assert!(defs.items.is_empty());
        assert!(defs.partial);
        gate.release();
    }

    #[test]
    fn a_cancel_mid_request_doesnt_wait_for_an_extraction() {
        let dir = callers(2);
        let gate = Gate::default();
        let nav = unbuilt(dir.path())
            .with_budget(Duration::from_secs(600))
            .with_extract(gate.extract());
        let (cancel, canceller) = cancel_when_started(&gate);
        let answer = nav.definitions(&other(dir.path()), &cancel);
        assert_eq!(answer, Err(NavError::Cancelled));
        assert_eq!(gate.started(), 1);
        gate.release();
        canceller.join().unwrap();
    }

    #[test]
    fn fallback_threads_are_bounded() {
        let dir = callers(2);
        let gate = Gate::default();
        let nav = unbuilt(dir.path())
            .with_budget(Duration::from_millis(100))
            .with_extract(gate.extract());
        // Each fallback blocks in its first extraction, past its request.
        for _ in 0..FALLBACK_THREADS + 3 {
            let defs = nav
                .definitions(&other(dir.path()), &AtomicBool::new(false))
                .unwrap();
            assert!(defs.partial);
        }
        let stats = nav.stats();
        assert_eq!(stats.searches, FALLBACK_THREADS);
        assert!(gate.started() <= FALLBACK_THREADS);
        // Let go, they end, and the next request searches again.
        gate.release();
        let freed = Instant::now();
        while nav.fallback_slots.busy.load(Ordering::Acquire) > 0 {
            assert!(freed.elapsed() < Duration::from_secs(30));
            thread::sleep(Duration::from_millis(5));
        }
        let _ = nav.definitions(&other(dir.path()), &AtomicBool::new(false));
        assert_eq!(nav.stats().searches, FALLBACK_THREADS + 1);
    }

    #[test]
    fn same_file_definitions_dont_outlast_the_deadline_or_a_cancel() {
        let dir = tempfile::tempdir().unwrap();
        let text = "fn helper() {}\nfn main() { helper(); }\n";
        let req = || NavRequest {
            offset: text.rfind("helper").unwrap(),
            ..request(dir.path(), text)
        };
        let found = unbuilt(dir.path())
            .with_budget(Duration::from_secs(60))
            .definitions(&req(), &AtomicBool::new(false))
            .unwrap();
        assert_eq!(found.items.len(), 1);
        assert_eq!(found.items[0].tier, Tier::SameFile);

        // A query still running at the deadline is left out.
        let blocked = |gate: &Gate, budget| {
            let gate = gate.clone();
            unbuilt(dir.path())
                .with_budget(budget)
                .with_run_symbols(move |job| {
                    gate.pass();
                    job.run()
                })
        };
        let gate = Gate::default();
        let late = blocked(&gate, Duration::from_millis(300))
            .definitions(&req(), &AtomicBool::new(false))
            .unwrap();
        assert!(late.items.is_empty());
        assert_eq!(gate.started(), 1);
        gate.release();

        // And a cancel meanwhile is answered at once.
        let gate = Gate::default();
        let nav = blocked(&gate, Duration::from_secs(600));
        let (cancel, canceller) = cancel_when_started(&gate);
        assert_eq!(nav.definitions(&req(), &cancel), Err(NavError::Cancelled));
        assert_eq!(gate.started(), 1);
        gate.release();
        canceller.join().unwrap();
    }

    #[test]
    fn a_slow_parse_is_given_up() {
        let dir = tempfile::tempdir().unwrap();
        let mut text = String::from("fn helper() {}\n");
        while text.len() < 1 << 20 {
            text.push_str("fn caller() { helper(); helper(); helper(); helper(); }\n");
        }
        let req = NavRequest {
            offset: text.rfind("helper").unwrap(),
            ..request(dir.path(), &text)
        };
        let nav = unbuilt(dir.path()).with_budget(Duration::from_secs(60));
        let started = Instant::now();
        let full = nav.definitions(&req, &AtomicBool::new(false)).unwrap();
        let took = started.elapsed();
        assert_eq!(full.items[0].tier, Tier::SameFile);

        // Out of time: answered without the text's syntax.
        let nav = unbuilt(dir.path()).with_budget(Duration::from_millis(1));
        let late = nav.definitions(&req, &AtomicBool::new(false)).unwrap();
        assert_eq!(late.word.as_deref(), Some("helper"));
        assert!(late.items.is_empty());

        // Cancelled meanwhile: answered well before the parse would end.
        let nav = unbuilt(dir.path()).with_budget(Duration::from_secs(60));
        let cancel = Arc::new(AtomicBool::new(false));
        let canceller = thread::spawn({
            let cancel = cancel.clone();
            move || {
                thread::sleep(took / 10);
                cancel.store(true, Ordering::Relaxed);
            }
        });
        let started = Instant::now();
        assert_eq!(nav.definitions(&req, &cancel), Err(NavError::Cancelled));
        assert!(
            started.elapsed() < took,
            "{:?} of {took:?}",
            started.elapsed()
        );
        canceller.join().unwrap();
    }

    #[test]
    fn import_lines_follow_open_lists() {
        let rust = "use crate::{\n    util,\n    lexer,\n};\nmod a;\nmod tests {\n    fn b() {}\n}\npub(crate) use x::y;\n";
        assert_eq!(
            imports("rust", rust),
            [
                "use crate::{",
                "    util,",
                "    lexer,",
                "};",
                "mod a;",
                "pub(crate) use x::y;"
            ]
        );
        let go = "import (\n\t\"fmt\"\n\t\"x/util\"\n)\nfunc main() {}\n";
        assert_eq!(
            imports("go", go),
            ["import (", "\t\"fmt\"", "\t\"x/util\"", ")"]
        );
        let python = "from util import (\n    a,\n    b,\n)\nx = 1\n";
        assert_eq!(
            imports("python", python),
            ["from util import (", "    a,", "    b,", ")"]
        );
    }

    #[test]
    fn javascript_imports_are_their_module_paths() {
        let cases: &[(&str, &[&str])] = &[
            ("import {\n  a,\n} from './models'", &["./models"]),
            ("import { helper }\n  from './util'", &["./util"]),
            ("import { helper }\n\nfrom './util'", &["./util"]),
            ("import { helper } from\n  './util'", &["./util"]),
            ("import\n  helper\nfrom './util'", &["./util"]),
            ("import helper, { a } from './util'", &["./util"]),
            ("import * as u from './util'", &["./util"]),
            ("import type { T } from './util'", &["./util"]),
            ("import { 'quoted' as q } from './util'", &["./util"]),
            ("import { a } /* x */ from // y\n './util'", &["./util"]),
            ("import './side-effect'", &["./side-effect"]),
            ("export * from './c'", &["./c"]),
            ("export * as c from './c'", &["./c"]),
            ("export {\n  w,\n}\nfrom './w'", &["./w"]),
            ("export\n{ w } from './w'", &["./w"]),
            ("export type { T } from './t'", &["./t"]),
            ("const d = require('./d')", &["./d"]),
            ("export { local }\nlet a = 1", &[]),
            ("export function from() {}", &[]),
            ("export const x = from('./y')", &[]),
            ("const m = import('./lazy')", &[]),
            ("import.meta.url", &[]),
            ("imports.util()", &[]),
        ];
        for (source, modules) in cases {
            assert_eq!(&imports("typescript", source), modules, "{source:?}");
        }
        // Statements after one another.
        let several = "import a from './a'\nimport {\n  b,\n} from './b'\nexport { c }\nexport * from './c'\n";
        assert_eq!(imports("tsx", several), ["./a", "./b", "./c"]);
    }

    #[test]
    fn import_keywords_take_any_whitespace() {
        for (language, line) in [
            ("rust", "use\tcrate::util;"),
            ("rust", "pub(crate)\tuse crate::util;"),
            ("rust", "extern\tcrate util;"),
            ("rust", "mod\tutil;"),
            ("python", "from\tutil import helper"),
            ("python", "import\tutil"),
            ("java", "import\tpkg.util;"),
            ("go", "import\t\"x/util\""),
            ("c", "#\tinclude \"util.h\""),
        ] {
            assert_eq!(imports(language, line), [line], "{language}");
        }
        assert_eq!(
            imports("javascript", "import\t{ a }\tfrom\t'./util'"),
            ["./util"]
        );
        for (language, line) in [
            ("rust", "user::util;"),
            ("rust", "mod util {"),
            ("python", "important = util"),
        ] {
            assert!(imports(language, line).is_empty(), "{language}: {line}");
        }
    }

    #[test]
    fn module_names() {
        assert_eq!(module_name(Path::new("src/util.rs")), Some("util"));
        assert_eq!(module_name(Path::new("src/util/mod.rs")), Some("util"));
        assert_eq!(module_name(Path::new("pkg/__init__.py")), Some("pkg"));
        assert!(has_word("./my-module.js", "my-module"));
        assert!(!has_word("use crate::utility;", "util"));
    }
}
