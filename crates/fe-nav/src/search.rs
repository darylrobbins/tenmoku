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
use fe_syntax::{
    ImportsError, Outline, OutlineJob, OutlineKind, Syntax, language_by_name, module_specifiers,
    symbols_job,
};

use crate::{
    Definition, Definitions, FileReferences, FileSymbols, Location, NavError, NavRequest,
    NavigationProvider, Precision, Readiness, ReferenceOptions, ReferencesSummary, Skipped,
    SymbolIndex, Tier, WorkspaceSymbol,
    extract::{extract_with, is_symbol, symbol_name, symbols_language},
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
/// imports, in a language whose imports are found line by line.
const IMPORT_SCAN_BYTES: usize = 1 << 20;

/// How much of the start of a JavaScript or TypeScript request text with no
/// tree is parsed to find its imports.
const IMPORT_PARSE_BYTES: usize = 256 << 10;

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
        let Ok(root) = dunce::canonicalize(&self.root) else {
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
            // A name ending in punctuation, lib.a(a.o), has no word boundary
            // to match; the extracted name is compared whole anyway.
            whole_word: self
                .word
                .chars()
                .next()
                .is_some_and(fe_project::is_word_char)
                && self
                    .word
                    .chars()
                    .next_back()
                    .is_some_and(fe_project::is_word_char),
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
                                snapshot: None,
                            },
                            name: symbol.name,
                            kind: Some(symbol.kind),
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
            // A quoted SQL identifier, "when", is a literal node, not a
            // string: the lexer knows it by its quotes, outside comments
            // and strings. A comment is rejected either way.
            let quoted_name = language.is_some_and(|l| l.name() == "sql")
                && quoted_at(&req.text, word_range.start)
                    == Some((word_range.start, word_range.end));
            // Make has no strings: the arguments of $(call name,x) are
            // highlighted as one, and a recipe's quotes are the shell's.
            let is_make = language.is_some_and(|l| l.name() == "make");
            let checked: Vec<_> = (spans.iter())
                .filter(|span| {
                    !((quoted_name || is_make) && span.highlight.name().starts_with("string"))
                })
                .cloned()
                .collect();
            if in_comment_or_string(&checked, &word_range) {
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
                let language = syntax.language().name();
                for item in items {
                    if symbol_name(language, &item.name) != word || !is_symbol(item.kind) {
                        continue;
                    }
                    let definition = Definition {
                        location: Location {
                            rel: req.rel.clone(),
                            range: item.name_range.clone(),
                            row: item.row,
                            snapshot: None,
                        },
                        name: word.clone(),
                        kind: Some(item.kind),
                        container: (item.parent).map(|parent| {
                            symbol_name(language, &items[parent as usize].name).to_owned()
                        }),
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
                        snapshot: None,
                    },
                    name: symbol.name.to_owned(),
                    kind: Some(symbol.kind),
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
        let scripting = req.language.is_some_and(is_script);
        let import_source: Option<Cow<'_, str>> = match (&source, req.language) {
            (_, None) => None,
            (Some(source), _) => Some(Cow::Borrowed(source)),
            (None, Some(_)) => {
                let limit = if scripting {
                    IMPORT_PARSE_BYTES
                } else {
                    IMPORT_SCAN_BYTES
                };
                let end = (req.text).clip_offset(limit, Bias::Left);
                Some(Cow::Owned(req.text.slice_to_string(0..end)))
            }
        };
        self.reached(Phase::Imports);
        let imports = match (req.language, &import_source) {
            (Some(language), Some(source)) if scripting => match &syntax {
                Some(syntax) => module_paths(syntax, source, cancel)?,
                None => script_imports(language, source, Some(deadline), cancel)?,
            },
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
                    snapshot: None,
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
/// identifier chars touching it, so a caret just after a word counts. In
/// Bash and Make a name may hold more, `my-func` or `build/main.o`; in SQL
/// a quoted one is the text between its quotes, `audit log`, as
/// [`symbol_name`] has it.
pub(crate) fn word_at(
    text: &Text,
    offset: usize,
    language: Option<&str>,
) -> Option<(Range<usize>, String)> {
    let dollar = matches!(language, Some("javascript" | "typescript" | "tsx"));
    // The extra chars of a name, past the search's word chars.
    let extra: &[char] = match language {
        Some("bash") => &['-', '.'],
        Some("make") => &['-', '.', '/', '+'],
        _ => &[],
    };
    // The search's word chars, so the word is one it finds whole.
    let is_word = |ch: char| fe_project::is_word_char(ch) || (dollar && ch == '$');
    let is_name = |ch: char| is_word(ch) || extra.contains(&ch);
    let offset = text.clip_offset(offset, Bias::Left);
    let row = text.offset_to_point(offset).row;
    let row_start = text.row_start(row);
    let line = text.row_to_string(row);
    let column = offset - row_start;
    let found = |start: usize, end: usize| {
        (start < end).then(|| {
            (
                row_start + start..row_start + end,
                line[start..end].to_owned(),
            )
        })
    };
    if language == Some("sql")
        && let Some((start, end)) = quoted_at(text, offset)
    {
        let name = text.slice_to_string(start..end);
        return Some((start..end, name));
    }
    let run = |is: &dyn Fn(char) -> bool| {
        let start = (line[..column].char_indices().rev())
            .take_while(|&(_, ch)| is(ch))
            .last()
            .map_or(column, |(at, _)| at);
        let end = (line[column..].char_indices())
            .find(|&(_, ch)| !is(ch))
            .map_or(line.len(), |(at, _)| column + at);
        (start, end)
    };
    let (mut start, mut end) = run(&is_word);
    if !extra.is_empty() {
        let (long_start, long_end) = run(&is_name);
        // A variable's expansion, $FOO-bar, is the variable.
        let head = &line[..long_start];
        // Make's names may hold the dash, $(BUILD-DIR), so only Bash's.
        let expanded = language == Some("bash") && (head.ends_with('$') || head.ends_with("${"));
        if !expanded && long_start <= start && end <= long_end && long_start < long_end {
            (start, end) = (long_start, long_end);
            if language == Some("make") {
                (start, end) = make_name(&line, start, end);
            }
        }
    }
    found(start, end)
}

/// The text between the quotes, `"` or `` ` ``, of the quoted identifier
/// in `text` that `offset` is in or at the end of, if not empty, as byte
/// offsets. Comments and strings, `'it''s'`, are skipped whole, and an
/// identifier may span rows.
fn quoted_at(text: &Text, offset: usize) -> Option<(usize, usize)> {
    let source = text.slice_to_string(0..text.len());
    let rest = |at: usize| &source[at..];
    let mut at = 0;
    while at <= offset && at < source.len() {
        let here = rest(at);
        let ch = here.chars().next()?;
        if here.starts_with("--") {
            at += here.find('\n').unwrap_or(here.len());
        } else if let Some(body) = here.strip_prefix("/*") {
            at += body.find("*/").map_or(here.len(), |end| end + 4);
        } else if matches!(ch, '"' | '`' | '\'') {
            // Up to the closing quote, a doubled one inside being the quote.
            let mut close = None;
            let mut chars = here[1..].char_indices().peekable();
            while let Some((i, c)) = chars.next() {
                if c != ch {
                    continue;
                }
                if chars.peek().is_some_and(|&(_, next)| next == ch) {
                    chars.next();
                    continue;
                }
                close = Some(at + 1 + i);
                break;
            }
            let close = close?;
            if ch != '\'' && at < offset && offset <= close && at + 1 < close {
                return Some((at + 1, close));
            }
            at = close + 1;
        } else {
            at += ch.len_utf8();
        }
    }
    None
}

/// The Make target `start..end` of `line` as a whole name: the archive
/// member `lib.a(a.o)` from either of its parts, and `$(OUT)/stamp` from
/// the part after the expansion.
fn make_name(line: &str, mut start: usize, mut end: usize) -> (usize, usize) {
    let name_char = |ch: char| ch.is_alphanumeric() || "_-./+".contains(ch);
    // Inside the parentheses of lib.a(a.o).
    let head = &line[..start];
    if head.ends_with('(') && !head.ends_with("$(") && line[end..].starts_with(')') {
        let archive = &head[..head.len() - 1];
        let from = (archive.char_indices().rev())
            .take_while(|&(_, ch)| name_char(ch))
            .last()
            .map(|(at, _)| at);
        if let Some(from) = from {
            return (from, end + 1);
        }
    }
    // The member's archive, lib.a of lib.a(a.o).
    if let Some(inner) = line[end..].strip_prefix('(')
        && let Some(close) = inner.find(')')
        && close > 0
        && inner[..close].chars().all(name_char)
    {
        end += 1 + close + 1;
    }
    // An expansion, $(OUT) or ${OUT}, before the path.
    let head = line[..start].trim_end_matches('/');
    if (head.len() < start || line[start..].starts_with('/'))
        && (head.ends_with(')') || head.ends_with('}'))
    {
        let open = if head.ends_with(')') { "$(" } else { "${" };
        if let Some(at) = head.rfind(open)
            && !head[at..].contains([' ', '\t'])
        {
            start = at;
        }
    }
    (start, end)
}

/// Whether every byte of `word` is under a comment or string highlight.
pub(crate) fn in_comment_or_string(
    spans: &[fe_syntax::HighlightSpan],
    word: &Range<usize>,
) -> bool {
    let mut covered: Vec<Range<usize>> = (spans.iter())
        .filter(|span| {
            let name = span.highlight.name();
            // A symbol, as Make's macro name, is a name, not text.
            name.starts_with("comment")
                || (name.starts_with("string") && !name.starts_with("string.special.symbol"))
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
            !hint.is_some_and(|hint| def.kind.is_some_and(|kind| hint.matches(kind))),
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

/// Whether the language's imports are found with a syntax tree: JavaScript,
/// TypeScript and TSX.
fn is_script(language: &str) -> bool {
    matches!(language, "javascript" | "typescript" | "tsx")
}

/// The module paths `syntax` imports, from `source`, the text it was parsed
/// from. `Err(Cancelled)` once `cancel` is set.
fn module_paths<'a>(
    syntax: &Syntax,
    source: &'a str,
    cancel: &AtomicBool,
) -> Result<Vec<&'a str>, NavError> {
    let mut cancelled = || {
        #[cfg(test)]
        query_polled();
        cancel.load(Ordering::Relaxed)
    };
    match module_specifiers(syntax, &mut cancelled) {
        Ok(ranges) => Ok(ranges.into_iter().map(|range| &source[range]).collect()),
        Err(ImportsError::Cancelled) => Err(NavError::Cancelled),
        Err(ImportsError::Unsupported | ImportsError::NotParsed) => Ok(Vec::new()),
    }
}

/// The module paths of a JavaScript, TypeScript or TSX `source`, which has
/// no tree yet: its first [`IMPORT_PARSE_BYTES`] are parsed, until `cancel`
/// is set or `deadline` passes. If that doesn't finish, it has no imports
/// here: a lexical guess finds phantom ones, and missing an `Imported`
/// boost only costs some ranking.
fn script_imports<'a>(
    language: &str,
    source: &'a str,
    deadline: Option<Instant>,
    cancel: &AtomicBool,
) -> Result<Vec<&'a str>, NavError> {
    let Some(language) = language_by_name(language) else {
        return Ok(Vec::new());
    };
    let source = &source[..source.floor_char_boundary(IMPORT_PARSE_BYTES)];
    // A request's text has only `\n` line breaks. Any other `\r` is read as
    // one, which keeps every offset.
    let text = source.replace('\r', "\n");
    let mut syntax = Syntax::new(language, Buffer::new(&text).current());
    let parsed = syntax.parse_until(Duration::MAX, || {
        cancel.load(Ordering::Relaxed)
            || deadline.is_some_and(|deadline| Instant::now() >= deadline)
    });
    if cancel.load(Ordering::Relaxed) {
        return Err(NavError::Cancelled);
    }
    if !parsed {
        return Ok(Vec::new());
    }
    module_paths(&syntax, source, cancel)
}

/// The lines of `source`'s import statements, for a language that has
/// them. A statement not complete on its first line (`use a::{`,
/// `import (`) runs on to the line that completes it. For JavaScript,
/// TypeScript and TSX, the module paths of its imports instead, from a tree
/// parsed here (see [`script_imports`]). `Err(Cancelled)` once `cancel` is
/// set, which it looks at every [`CANCEL_EVERY`] lines.
fn import_lines<'a>(
    language: &str,
    source: &'a str,
    cancel: &AtomicBool,
) -> Result<Vec<&'a str>, NavError> {
    if is_script(language) {
        return script_imports(language, source, None, cancel);
    }
    let lines: Vec<&str> = source.lines().collect();
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
        "c" | "cpp" => {
            let directive = line.strip_prefix('#')?.trim_start();
            keyword(directive, "include").map(|_| Statement::LINE)
        }
        _ => None,
    }
}

/// Work an import step does, for [`Poll`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Work {
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
}

// What a test does each time the import query looks at the cancel flag, on
// its own thread.
#[cfg(test)]
thread_local! {
    static QUERY_POLLED: std::cell::RefCell<Option<Box<dyn FnMut()>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn query_polled() {
    QUERY_POLLED.with(|hook| {
        if let Some(hook) = hook.borrow_mut().as_mut() {
            hook();
        }
    });
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
#[path = "search_tree_import_tests.rs"]
mod tree_import_tests;

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
            buffer: None,
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

        fn wait_started(&self, count: usize) {
            let (state, changed) = &*self.0;
            let state = state.lock().unwrap();
            drop(changed.wait_while(state, |state| state.0 < count));
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
        cancel_when_started_count(gate, 1)
    }

    /// Sets `cancel` once `gate` has started `count` times.
    fn cancel_when_started_count(
        gate: &Gate,
        count: usize,
    ) -> (Arc<AtomicBool>, thread::JoinHandle<()>) {
        let cancel = Arc::new(AtomicBool::new(false));
        let canceller = thread::spawn({
            let (gate, cancel) = (gate.clone(), cancel.clone());
            move || {
                gate.wait_started(count);
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
        // Under load the deadline can pass before the request reaches the
        // extraction, which proves nothing: try again with twice the budget.
        let mut budget = Duration::from_millis(300);
        let cap = Duration::from_secs(10);
        loop {
            let gate = Gate::default();
            let nav = unbuilt(dir.path())
                .with_budget(budget)
                .with_extract(gate.extract());
            let defs = nav
                .definitions(&other(dir.path()), &AtomicBool::new(false))
                .unwrap();
            if gate.started() == 0 {
                gate.release();
                assert!(
                    budget < cap,
                    "no extraction started within a budget of {budget:?}"
                );
                budget = (budget * 2).min(cap);
                continue;
            }
            // The extraction is still waiting, and the answer came anyway.
            assert_eq!(gate.started(), 1);
            assert!(defs.items.is_empty());
            assert!(defs.partial);
            gate.release();
            return;
        }
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
            .with_budget(Duration::from_secs(600))
            .with_extract(gate.extract());
        // Each fallback blocks in its first extraction, past its request. A
        // request is cancelled once its fallback is known to be blocked, so
        // nothing here depends on how fast a thread runs.
        for i in 0..FALLBACK_THREADS {
            let (cancel, canceller) = cancel_when_started_count(&gate, i + 1);
            let answer = nav.definitions(&other(dir.path()), &cancel);
            assert_eq!(answer, Err(NavError::Cancelled));
            canceller.join().unwrap();
        }
        // With every slot held, the rest return at once without searching.
        for _ in 0..3 {
            let defs = nav
                .definitions(&other(dir.path()), &AtomicBool::new(false))
                .unwrap();
            assert!(defs.partial);
        }
        let stats = nav.stats();
        assert_eq!(stats.searches, FALLBACK_THREADS);
        assert_eq!(gate.started(), FALLBACK_THREADS);
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
            buffer: None,
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
            buffer: None,
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
