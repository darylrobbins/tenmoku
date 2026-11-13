//! Languages loaded at run time from WebAssembly grammars, such as those
//! extensions contribute ([`register_language`]), and the per-thread parsers
//! that parse them ([`ThreadParser`]).
//!
//! A module is compiled once, on a shared [`Engine`], into a language every
//! thread can use. Parsing it needs a [`WasmStore`] in the parser, which
//! instantiates each module it parses, so each thread keeps one parser with
//! its store instead of making one per parse.
//!
//! A grammar's code runs under a deadline ([`GrammarDeadlines`]): loading
//! it, and each parse. The engine checks it once an epoch, which a ticker
//! thread advances while any deadline is running; code still running when it
//! passes stops (the vendored tree-sitter's patch, `vendor/tree-sitter`).

use kiln_l10n::tr;
use std::{
    cell::Cell,
    collections::BTreeSet,
    fmt, fs,
    hash::{DefaultHasher, Hash as _, Hasher as _},
    io::{self, Read as _},
    ops::{Deref, DerefMut},
    path::{Path, PathBuf},
    sync::{Condvar, Mutex, OnceLock, PoisonError, RwLock},
    time::{Duration, Instant},
};

use tree_sitter::{
    LANGUAGE_VERSION, LanguageError, MIN_COMPATIBLE_LANGUAGE_VERSION, Parser, QueryError,
    WasmStore,
    wasmtime::{Config, Engine},
};

use crate::{
    CommentTokens, Language, detect,
    language::{self, QueryKind},
};

/// The most [`register_language`] reads of a module or a query file.
pub const MAX_GRAMMAR_BYTES: u64 = 20 << 20;

/// How long a grammar's code may run before it's stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrammarDeadlines {
    /// Loading it, from compiling the module (for which there's time to
    /// spare) to its initializer's end; also instantiating it in each
    /// parsing thread.
    pub load: Duration,
    /// Each parse, as long as a full parse is given on another thread.
    pub parse: Duration,
}

impl GrammarDeadlines {
    pub const DEFAULT: Self = Self {
        load: Duration::from_secs(10),
        parse: Duration::from_secs(5),
    };
}

impl Default for GrammarDeadlines {
    fn default() -> Self {
        Self::DEFAULT
    }
}

static DEADLINES: RwLock<GrammarDeadlines> = RwLock::new(GrammarDeadlines::DEFAULT);

/// Sets the deadlines for loads and parses started from now on.
pub fn set_grammar_deadlines(deadlines: GrammarDeadlines) {
    *DEADLINES.write().unwrap_or_else(PoisonError::into_inner) = deadlines;
}

fn deadlines() -> GrammarDeadlines {
    *DEADLINES.read().unwrap_or_else(PoisonError::into_inner)
}

/// A grammar to load at run time, and the files it's for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GrammarSpec {
    /// What [`crate::language_by_name`] and injections call it.
    pub language: String,
    /// The module `tree-sitter build --wasm` makes. Its file name says which
    /// grammar it holds, as the CLI names it: `tree-sitter-<grammar>.wasm`
    /// or `<grammar>.wasm`, a `-` in `<grammar>` standing for `_`.
    pub wasm: PathBuf,
    /// File extensions, without the dot, and whole file names it's chosen
    /// for, ignoring case. An extension may be compound, such as `blade.php`
    /// for `x.blade.php`: it wins over any language's plain `php`, and the
    /// longest compound one wins; see [`Language::file_has_extension`].
    pub extensions: Vec<String>,
    pub file_names: Vec<String>,
    /// The highlight query; without one the language parses (for folding
    /// and selection) but isn't coloured.
    pub highlights: Option<PathBuf>,
    /// The query marking code in other languages, as in Markdown fences.
    pub injections: Option<PathBuf>,
    /// Whether the first of several patterns capturing a node wins, as in
    /// tree-sitter's own highlighter and most grammars' queries. Otherwise the
    /// last does, as in Neovim's.
    pub earlier_patterns_win: bool,
    /// The token starting a line comment, for [`crate::comment_tokens`].
    pub line_comment: Option<String>,
    /// The tokens opening and closing a block comment.
    pub block_comment: Option<(String, String)>,
    /// Interpreters whose `#!` line picks it for a file no path decides,
    /// by base name, as [`crate::language_name_for_shebang`] matches the
    /// built-in ones: `elixir` takes `#!/usr/bin/env elixir` and
    /// `#!/usr/bin/elixir1.17`.
    pub shebangs: Vec<String>,
}

/// Why [`register_language`] refused a grammar.
#[derive(Debug)]
pub enum GrammarError {
    /// A file couldn't be read.
    Read { file: PathBuf, error: io::Error },
    /// A file is over [`MAX_GRAMMAR_BYTES`].
    TooLarge { file: PathBuf, bytes: u64 },
    /// The module isn't a grammar this tree-sitter loads: not WebAssembly,
    /// without the `tree_sitter_<grammar>` function its name says, or of an
    /// ABI version outside those supported.
    Module { file: PathBuf, message: String },
    /// A query doesn't compile against the grammar; `error` has its row and
    /// column, zero-based.
    Query { file: PathBuf, error: QueryError },
    /// The module's code was still running at the load's deadline
    /// ([`GrammarDeadlines::load`]), as an initializer that never returns is.
    Deadline { file: PathBuf, deadline: Duration },
    /// A built-in language has this name, extension or file name; the
    /// built-in keeps it.
    Clash {
        language: String,
        built_in: &'static str,
        clash: Clash,
    },
}

/// What a [`GrammarError::Clash`] is over.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Clash {
    Name,
    Extension(String),
    FileName(String),
    /// A [shebang](GrammarSpec::shebangs) interpreter, some name of which
    /// a built-in's would also match.
    Interpreter(String),
}

impl GrammarError {
    /// The file at fault: all but a clash have one.
    pub fn file(&self) -> Option<&Path> {
        match self {
            Self::Read { file, .. }
            | Self::TooLarge { file, .. }
            | Self::Module { file, .. }
            | Self::Query { file, .. }
            | Self::Deadline { file, .. } => Some(file),
            Self::Clash { .. } => None,
        }
    }
}

impl fmt::Display for GrammarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { file, error } => f.write_str(&tr!(
                "syntax-grammar-file-error",
                file = file.display().to_string(),
                error = error.to_string()
            )),
            Self::TooLarge { file, bytes } => f.write_str(&tr!(
                "syntax-grammar-too-large",
                file = file.display().to_string(),
                bytes = *bytes,
                max = MAX_GRAMMAR_BYTES
            )),
            Self::Module { file, message } => f.write_str(&tr!(
                "syntax-grammar-file-error",
                file = file.display().to_string(),
                error = message.as_str()
            )),
            Self::Query { file, error } => f.write_str(&tr!(
                "syntax-grammar-file-error",
                file = file.display().to_string(),
                error = error.to_string()
            )),
            Self::Deadline { file, deadline } => f.write_str(&tr!(
                "syntax-grammar-deadline",
                file = file.display().to_string(),
                deadline = format!("{deadline:?}")
            )),
            Self::Clash {
                language,
                built_in,
                clash,
            } => {
                let (kind, value) = match clash {
                    Clash::Name => ("name", String::new()),
                    Clash::Extension(extension) => ("extension", format!("{extension:?}")),
                    Clash::FileName(name) => ("file-name", format!("{name:?}")),
                    Clash::Interpreter(name) => ("interpreter", format!("{name:?}")),
                };
                f.write_str(&tr!(
                    "syntax-grammar-clash",
                    language = language.as_str(),
                    kind = kind,
                    value = value,
                    built_in = *built_in
                ))
            }
        }
    }
}

impl std::error::Error for GrammarError {}

/// Loads `spec`'s grammar and queries and adds the language for its files,
/// after the built-in languages: lookups by name, path and injection find a
/// built-in first, and an earlier registered language before a later one.
///
/// Registering a name again replaces it, unless its files are unchanged, which
/// returns the language already loaded. Registrations run one at a time.
/// Each language loaded stays in memory, for the documents still using it.
///
/// Compiling the module takes a while (see the crate's tests), so call it off
/// the main thread.
pub fn register_language(spec: &GrammarSpec) -> Result<&'static Language, GrammarError> {
    if let Some((built_in, clash)) = language::built_in_clash(spec) {
        return Err(GrammarError::Clash {
            language: spec.language.clone(),
            built_in,
            clash,
        });
    }
    let _loading = loading();
    let wasm = read(&spec.wasm)?;
    let highlights = spec.highlights.as_deref().map(read_text).transpose()?;
    let injections = spec.injections.as_deref().map(read_text).transpose()?;
    let grammar_name = grammar_name(&spec.wasm);

    let mut hasher = DefaultHasher::new();
    (&wasm, &highlights, &injections, &grammar_name).hash(&mut hasher);
    spec.earlier_patterns_win.hash(&mut hasher);
    let version = hasher.finish();
    let (name, loaded) = {
        let registry = registry().read().unwrap_or_else(PoisonError::into_inner);
        let existing = registry.iter().find(|entry| entry.name == spec.language);
        (
            existing.map(|entry| entry.name),
            existing
                .filter(|entry| entry.version == version)
                .map(|entry| entry.language),
        )
    };
    let language = match loaded {
        Some(language) => language,
        None => {
            let grammar = load(&spec.wasm, &grammar_name, &wasm)?;
            let name = name.unwrap_or_else(|| String::leak(spec.language.clone()));
            let mut language = Language::new(
                name,
                grammar,
                highlights.as_deref().unwrap_or_default(),
                spec.earlier_patterns_win,
                injections.as_deref(),
            )
            .map_err(|(kind, error)| {
                let file = match kind {
                    QueryKind::Highlights => spec.highlights.clone(),
                    QueryKind::Injections => spec.injections.clone(),
                };
                GrammarError::Query {
                    file: file.unwrap_or_default(),
                    error,
                }
            })?;
            language.wasm = Some(spec.wasm.clone());
            &*Box::leak(Box::new(language))
        }
    };
    let entry = Registered {
        name: language.name(),
        extensions: spec.extensions.clone(),
        file_names: spec.file_names.clone(),
        comments: comment_tokens_of(spec),
        shebangs: (spec.shebangs.iter())
            .filter(|name| !name.is_empty())
            .cloned()
            .collect(),
        language,
        version,
    };
    let mut registry = registry().write().unwrap_or_else(PoisonError::into_inner);
    match registry.iter_mut().find(|other| other.name == entry.name) {
        Some(other) => *other = entry,
        None => registry.push(entry),
    }
    Ok(language)
}

/// Withdraws the language registered as `name` ([`register_language`]):
/// files opened after, and injections found after, no longer get it,
/// while a [`Syntax`](crate::Syntax) made with it keeps it. Waits for a
/// registration under way, so one that started before can't publish
/// after. Returns whether it was registered.
pub fn unregister_language(name: &str) -> bool {
    let _loading = loading();
    let mut registry = registry().write().unwrap_or_else(PoisonError::into_inner);
    let before = registry.len();
    registry.retain(|entry| entry.name != name);
    registry.len() != before
}

/// Held from reading a grammar's files to publishing it, and while one is
/// withdrawn: two registrations of the same files then load once, and
/// the one called last, which read them last, is the one kept.
fn loading() -> std::sync::MutexGuard<'static, ()> {
    static LOADING: Mutex<()> = Mutex::new(());
    LOADING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The grammar `wasm` holds, by its file name: see [`GrammarSpec::wasm`].
fn grammar_name(wasm: &Path) -> String {
    let stem = wasm
        .file_stem()
        .map(|stem| stem.to_string_lossy())
        .unwrap_or_default();
    let stem = stem.strip_prefix("tree-sitter-").unwrap_or(&stem);
    stem.replace('-', "_")
}

fn load(
    file: &Path,
    grammar_name: &str,
    wasm: &[u8],
) -> Result<tree_sitter::Language, GrammarError> {
    let module = |message: String| GrammarError::Module {
        file: file.to_owned(),
        message,
    };
    let engine = engine().map_err(|error| module(error.to_owned()))?;
    let timeout = deadlines().load;
    // The store only instantiates the module to read the language out of
    // it; parsers instantiate it again in their own.
    let (loaded, timed_out) = within(timeout, &|| false, || {
        WasmStore::new(engine)?.load_language(grammar_name, wasm)
    });
    let grammar = loaded.map_err(|error| {
        if timed_out {
            GrammarError::Deadline {
                file: file.to_owned(),
                deadline: timeout,
            }
        } else {
            module(error.to_string())
        }
    })?;
    let abi = grammar.abi_version();
    if !(MIN_COMPATIBLE_LANGUAGE_VERSION..=LANGUAGE_VERSION).contains(&abi) {
        return Err(module(
            tr!(
                "syntax-grammar-abi",
                abi = abi.to_string(),
                min = MIN_COMPATIBLE_LANGUAGE_VERSION.to_string(),
                max = LANGUAGE_VERSION.to_string()
            )
            .to_string(),
        ));
    }
    Ok(grammar)
}

fn read(file: &Path) -> Result<Vec<u8>, GrammarError> {
    let error = |error| GrammarError::Read {
        file: file.to_owned(),
        error,
    };
    let opened = fs::File::open(file).map_err(error)?;
    let mut bytes = Vec::new();
    // One byte over the limit tells a file that's too large, whatever its
    // metadata says.
    opened
        .take(MAX_GRAMMAR_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    if bytes.len() as u64 > MAX_GRAMMAR_BYTES {
        let bytes = fs::metadata(file).map_or(MAX_GRAMMAR_BYTES + 1, |meta| meta.len());
        return Err(GrammarError::TooLarge {
            file: file.to_owned(),
            bytes,
        });
    }
    Ok(bytes)
}

fn read_text(file: &Path) -> Result<String, GrammarError> {
    String::from_utf8(read(file)?).map_err(|error| GrammarError::Read {
        file: file.to_owned(),
        error: io::Error::new(io::ErrorKind::InvalidData, error),
    })
}

/// How often the engine's epoch advances while a deadline runs, so how
/// often running code checks it.
const EPOCH: Duration = Duration::from_millis(5);

/// The engine every module is compiled on, with epoch interruption, and the
/// ticker advancing its epoch. Without a ticker, an error: a grammar's code
/// would run with no deadline.
fn engine() -> Result<&'static Engine, &'static str> {
    static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();
    let engine = ENGINE.get_or_init(|| {
        new_engine(|ticking| {
            std::thread::Builder::new()
                .name("wasm-epoch".into())
                .spawn(move || tick(&ticking))
                .map(drop)
        })
    });
    engine.as_ref().map_err(String::as_str)
}

/// An engine with epoch interruption, once `start_ticker` has started its
/// ticker with a handle to it.
fn new_engine(start_ticker: impl FnOnce(Engine) -> io::Result<()>) -> Result<Engine, String> {
    let mut config = Config::new();
    config.epoch_interruption(true);
    let engine = Engine::new(&config).map_err(|error| error.to_string())?;
    start_ticker(engine.clone())
        .map_err(|error| tr!("syntax-grammar-no-ticker", error = error.to_string()).to_string())?;
    Ok(engine)
}

/// How many [`within`] calls are running, which the ticker waits for.
static RUNNING: Mutex<usize> = Mutex::new(0);
static STARTED: Condvar = Condvar::new();

/// The ticker: advances `engine`'s epoch every [`EPOCH`] while a [`within`]
/// call is running, and sleeps while none is.
fn tick(engine: &Engine) {
    loop {
        let running = RUNNING.lock().unwrap_or_else(PoisonError::into_inner);
        drop(
            STARTED
                .wait_while(running, |running| *running == 0)
                .unwrap_or_else(PoisonError::into_inner),
        );
        std::thread::sleep(EPOCH);
        engine.increment_epoch();
    }
}

/// Runs `f`, with Wasm code this thread runs inside it stopped once
/// `timeout` from now has passed, or once `interrupt` says so: the caller's
/// own budget. Also returns whether the timeout stopped it.
fn within<R>(timeout: Duration, interrupt: &dyn Fn() -> bool, f: impl FnOnce() -> R) -> (R, bool) {
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            *RUNNING.lock().unwrap_or_else(PoisonError::into_inner) -= 1;
        }
    }
    let deadline = Instant::now().checked_add(timeout);
    let timed_out = Cell::new(false);
    let stop = || {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            timed_out.set(true);
            return true;
        }
        interrupt()
    };
    *RUNNING.lock().unwrap_or_else(PoisonError::into_inner) += 1;
    let running = Running;
    STARTED.notify_all();
    let result = tree_sitter::with_wasm_interrupt(&stop, f);
    drop(running);
    (result, timed_out.get())
}

struct Registered {
    name: &'static str,
    extensions: Vec<String>,
    file_names: Vec<String>,
    /// `None` when its spec gave no tokens, as for a language without
    /// them before they could be given.
    comments: Option<CommentTokens>,
    shebangs: Vec<String>,
    language: &'static Language,
    /// A hash of the files it was loaded from.
    version: u64,
}

fn registry() -> &'static RwLock<Vec<Registered>> {
    static REGISTRY: RwLock<Vec<Registered>> = RwLock::new(Vec::new());
    &REGISTRY
}

fn find<T>(f: impl FnMut(&Registered) -> Option<T>) -> Option<T> {
    registry()
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .find_map(f)
}

pub(crate) fn by_name(name: &str) -> Option<&'static Language> {
    find(|entry| (entry.name == name).then_some(entry.language))
}

/// The comment tokens `spec` gives, interned: `None` if it gives none.
/// An empty token is none.
fn comment_tokens_of(spec: &GrammarSpec) -> Option<CommentTokens> {
    let line = (spec.line_comment.as_deref())
        .filter(|token| !token.is_empty())
        .map(intern);
    let block = (spec.block_comment.as_ref())
        .filter(|(open, close)| !open.is_empty() && !close.is_empty())
        .map(|(open, close)| (intern(open), intern(close)));
    (line.is_some() || block.is_some()).then_some(CommentTokens { line, block })
}

/// `text` as a string that lives for the process, one copy each: a
/// grammar registered again with the same tokens leaks nothing more.
fn intern(text: &str) -> &'static str {
    static INTERNED: Mutex<BTreeSet<&'static str>> = Mutex::new(BTreeSet::new());
    let mut interned = INTERNED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(&kept) = interned.get(text) {
        return kept;
    }
    let kept = String::leak(text.to_owned());
    interned.insert(kept);
    kept
}

pub(crate) fn comment_tokens(name: &str) -> Option<CommentTokens> {
    find(|entry| (entry.name == name).then_some(entry.comments)).flatten()
}

/// The registered language whose shebangs match the interpreter `program`,
/// a base name; see [`detect::interpreter_matches`].
pub(crate) fn name_for_interpreter(program: &str) -> Option<&'static str> {
    find(|entry| {
        (entry.shebangs.iter())
            .any(|interpreter| detect::interpreter_matches(interpreter, program))
            .then_some(entry.name)
    })
}

pub(crate) fn name_for_file(file_name: &str, extension: Option<&str>) -> Option<&'static str> {
    find(|entry| {
        let named = entry
            .file_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(file_name));
        let extended = extension.is_some_and(|extension| {
            entry
                .extensions
                .iter()
                .any(|known| known.eq_ignore_ascii_case(extension))
        });
        (named || extended).then_some(entry.name)
    })
}

/// Each registered language claiming a compound extension of a file called
/// `file_name`, with the longest such claim's length, in registration order.
pub(crate) fn compound_owners(file_name: &str) -> Vec<(&'static str, usize)> {
    registry()
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .filter_map(|entry| {
            let extensions = entry.extensions.iter().map(String::as_str);
            Some((
                entry.name,
                language::longest_compound(file_name, extensions)?,
            ))
        })
        .collect()
}

/// The language an injection names, lowercase: by name, else by extension.
pub(crate) fn injected(name: &str) -> Option<&'static Language> {
    find(|entry| {
        entry
            .name
            .eq_ignore_ascii_case(name)
            .then_some(entry.language)
    })
    .or_else(|| {
        find(|entry| {
            entry
                .extensions
                .iter()
                .any(|extension| extension.eq_ignore_ascii_case(name))
                .then_some(entry.language)
        })
    })
}

pub(crate) fn names() -> Vec<&'static str> {
    registry()
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .iter()
        .map(|entry| entry.name)
        .collect()
}

thread_local! {
    static PARSER: Cell<Option<Cached>> = const { Cell::new(None) };
}

struct Cached {
    parser: Parser,
    /// Whether it has a [`WasmStore`].
    wasm: bool,
}

/// This thread's parser, back in its cache when dropped, reset to parse
/// afresh. When that one's taken already, a new one.
pub(crate) struct ThreadParser {
    cached: Option<Cached>,
    /// The WebAssembly language set, if it is one.
    wasm: Option<WasmLanguage>,
}

/// A WebAssembly language [`ThreadParser::set_language`] was given. The
/// parser is set to it in [`ThreadParser::parse`], under the parse's budget:
/// a store new to the module instantiates it, running its initializer.
struct WasmLanguage {
    name: &'static str,
    file: PathBuf,
    grammar: tree_sitter::Language,
    set: bool,
}

impl ThreadParser {
    pub(crate) fn take() -> Self {
        let cached = PARSER.try_with(Cell::take).ok().flatten();
        Self {
            cached: Some(cached.unwrap_or_else(|| Cached {
                parser: Parser::new(),
                wasm: false,
            })),
            wasm: None,
        }
    }

    /// [`Self::take`], set to `language`.
    pub(crate) fn for_language(language: &Language) -> Result<Self, LanguageError> {
        let mut parser = Self::take();
        parser.set_language(language)?;
        Ok(parser)
    }

    /// Sets the parser's language. A WebAssembly one gets the parser a store
    /// if it has none, and is set when it [parses](Self::parse).
    pub(crate) fn set_language(&mut self, language: &Language) -> Result<(), LanguageError> {
        self.wasm = None;
        let cached = self.cached();
        if !language.grammar.is_wasm() {
            return cached.parser.set_language(&language.grammar);
        }
        if !cached.wasm {
            let store = engine()
                .map_err(str::to_owned)
                .and_then(|engine| WasmStore::new(engine).map_err(|error| error.to_string()))
                .map_err(|error| {
                    log::error!("{}: no WebAssembly store: {error}", language.name());
                    LanguageError::Wasm
                })?;
            cached.parser.set_wasm_store(store)?;
            cached.wasm = true;
        }
        self.wasm = Some(WasmLanguage {
            name: language.name(),
            file: language.wasm.clone().unwrap_or_default(),
            grammar: language.grammar.clone(),
            set: false,
        });
        Ok(())
    }

    /// Runs `parse`, one parse with this parser. With a WebAssembly language
    /// it's set first, and its code stops once `interrupt` says so, the
    /// caller's budget, or at the parse deadline, which is logged.
    pub(crate) fn parse<T>(
        &mut self,
        interrupt: &dyn Fn() -> bool,
        parse: impl FnOnce(&mut Parser) -> Option<T>,
    ) -> Option<T> {
        let Self { cached, wasm } = self;
        let parser = &mut cached.as_mut().expect("taken only when dropped").parser;
        let Some(wasm) = wasm else {
            return parse(parser);
        };
        let timeout = deadlines().parse;
        let (parsed, timed_out) = within(timeout, interrupt, || {
            if !wasm.set {
                if let Err(error) = parser.set_language(&wasm.grammar) {
                    log::warn!("{}: {}: {error}", wasm.file.display(), wasm.name);
                    return None;
                }
                wasm.set = true;
            }
            parse(parser)
        });
        if parsed.is_none() && timed_out {
            log::error!(
                "{}: {} was still running at its {timeout:?} parse deadline, so stopped",
                wasm.file.display(),
                wasm.name
            );
        }
        parsed
    }

    fn cached(&mut self) -> &mut Cached {
        self.cached.as_mut().expect("taken only when dropped")
    }
}

impl Deref for ThreadParser {
    type Target = Parser;

    fn deref(&self) -> &Parser {
        &self
            .cached
            .as_ref()
            .expect("taken only when dropped")
            .parser
    }
}

impl DerefMut for ThreadParser {
    fn deref_mut(&mut self) -> &mut Parser {
        &mut self.cached().parser
    }
}

impl Drop for ThreadParser {
    fn drop(&mut self) {
        let Some(mut cached) = self.cached.take() else {
            return;
        };
        // A parse stopped part way would otherwise resume.
        cached.parser.reset();
        if cached.parser.set_included_ranges(&[]).is_err() {
            return;
        }
        // Gone at thread exit, when the parser is just dropped.
        let _ = PARSER.try_with(|slot| {
            // One with a store is the one worth keeping.
            let kept = match slot.take() {
                Some(other) if other.wasm || !cached.wasm => other,
                _ => cached,
            };
            slot.set(Some(kept));
        });
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::new_engine;

    #[test]
    fn no_ticker_is_no_engine() {
        let error = new_engine(|_| Err(io::Error::other("refused"))).unwrap_err();
        assert_eq!(error, "no thread to enforce grammars' deadlines: refused");
    }
}
