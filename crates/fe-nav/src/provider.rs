//! The [`NavigationProvider`] seam and the [`Navigation`] router.
//!
//! A provider answers go-to-definition, find references and workspace
//! symbols: by search
//! ([`crate::SearchNavigation`]) until a precise one (a language server) is
//! ready for the file's language. The router asks one provider per request,
//! the best that's ready, and says which answered, so the UI can label a
//! search answer as such.
//!
//! Calls are blocking and run on a caller-owned background thread, with an
//! [`AtomicBool`] to cancel them, as [`fe_project::search`] and
//! [`crate::build`] do. A provider that is async inside (an LSP client)
//! blocks on its own response, so this crate needs no runtime.

use std::{
    cell::Cell,
    fmt,
    ops::Range,
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_buffer::{BufferId, Text, Version};
use fe_syntax::OutlineKind;

use crate::{FileSymbols, Symbol, SymbolIndex};

/// How a provider knows what it answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Precision {
    /// By name, from an index or a text search: it may list definitions of
    /// other things with the same name.
    Search,
    /// By the language's own resolution, as a language server gives it.
    Exact,
}

/// Whether a provider can answer for a language now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Readiness {
    Ready,
    /// It will be, once it has started or indexed.
    NotYet,
    /// It never answers for this language.
    Unsupported,
}

/// A go-to-definition request: the active buffer and the caret in it.
#[derive(Clone, Debug)]
pub struct NavRequest {
    /// The project root.
    pub root: PathBuf,
    /// The buffer's file, relative to `root`.
    pub rel: PathBuf,
    /// The buffer's language, by [`fe_syntax::Language::name`].
    pub language: Option<&'static str>,
    /// The buffer's current text, unsaved edits included.
    pub text: Text,
    /// The caret, as a byte offset into `text`.
    pub offset: usize,
    /// The buffer `text` is a snapshot of, and its version: what a
    /// provider syncing buffers (a language server's) checks the request
    /// against. `None` for a request about no open buffer.
    pub buffer: Option<(BufferId, Version)>,
}

impl NavRequest {
    /// The word at the caret, as search finds it: the longest run of
    /// identifier chars touching it, so a caret just after a word counts.
    pub fn word(&self) -> Option<String> {
        crate::search::word_at(&self.text, self.offset, self.language).map(|(_, word)| word)
    }
}

/// A place in a project file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    /// Relative to the project root. A language server may answer with a
    /// file outside it (a library's source): that one is absolute, so
    /// joining it to the root gives it as is.
    pub rel: PathBuf,
    /// Bytes in the file's text as the editor loads it (see
    /// [`crate::Symbol`]), or, with a `snapshot`, in that buffer version's
    /// text. For the request's own file, bytes in [`NavRequest::text`].
    pub range: Range<usize>,
    /// The 0-based row where `range` starts.
    pub row: u32,
    /// The open buffer, and its version, whose text `range` is in; `None`
    /// for the file's text as loaded from disk. The buffer may have been
    /// edited since: map `range` from that version before selecting it.
    pub snapshot: Option<(BufferId, Version)>,
}

/// How close a definition is to the request, best first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tier {
    /// In the request's own file.
    SameFile,
    /// In a file the request's file imports.
    Imported,
    /// Anywhere else in the project.
    Project,
}

/// A definition a request may mean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition {
    /// Where its name is.
    pub location: Location,
    pub name: String,
    /// What it defines; `None` when the provider doesn't say (a language
    /// server's locations carry no kind).
    pub kind: Option<OutlineKind>,
    /// The name of the item it's nested in, if any.
    pub container: Option<String>,
    pub tier: Tier,
}

/// The answer to a go-to-definition request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Definitions {
    /// The word at the caret, if there is one.
    pub word: Option<String>,
    /// Best first.
    pub items: Vec<Definition>,
    /// Exactly one item, or the first item's tier is strictly better than
    /// the second's: the UI jumps straight to it rather than listing them.
    pub unambiguous: bool,
    /// The answer came while the provider's index was incomplete, so there
    /// may be definitions it doesn't list.
    pub partial: bool,
}

/// A workspace symbol matching a query.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceSymbol {
    /// Where its name is.
    pub location: Location,
    pub name: String,
    pub kind: OutlineKind,
    pub container: Option<String>,
    /// The chars of `name` the query matched, as ascending char indices.
    pub indices: Vec<usize>,
}

impl WorkspaceSymbol {
    /// The best `limit` of `symbols` for `query`, best first, each with
    /// `indices` set: ranked and highlighted by the fuzzy matcher
    /// [`SymbolIndex::fuzzy`] matches with, so another provider's symbols
    /// (a language server's) rank and highlight as search's do. Equal
    /// scores keep their order in `symbols`; names it doesn't match come
    /// last, unhighlighted, in their order.
    pub fn rank(query: &str, symbols: Vec<WorkspaceSymbol>, limit: usize) -> Vec<WorkspaceSymbol> {
        let mut distinct: Vec<&str> = symbols.iter().map(|symbol| symbol.name.as_str()).collect();
        distinct.sort_unstable();
        distinct.dedup();
        // An index of the names alone, rooted nowhere: nothing is read.
        let mut index = SymbolIndex::new(PathBuf::new());
        let names = (distinct.iter())
            .map(|&name| Symbol {
                name: name.to_owned(),
                kind: OutlineKind::Variable,
                name_range: 0..0,
                range: 0..0,
                row: 0,
                container: None,
            })
            .collect();
        index.extend(vec![FileSymbols {
            rel: PathBuf::new(),
            language: "",
            len: 0,
            modified: None,
            symbols: names,
            truncated: false,
        }]);
        let matched: std::collections::HashMap<String, (u16, Vec<usize>)> =
            (index.fuzzy(query, distinct.len()).into_iter())
                .map(|found| (found.symbol.name.to_owned(), (found.score, found.indices)))
                .collect();
        let mut ranked: Vec<(Option<u16>, WorkspaceSymbol)> = (symbols.into_iter())
            .map(|mut symbol| {
                let found = matched.get(&symbol.name);
                symbol.indices = found
                    .map(|(_, indices)| indices.clone())
                    .unwrap_or_default();
                (found.map(|(score, _)| *score), symbol)
            })
            .collect();
        ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        ranked.truncate(limit);
        ranked.into_iter().map(|(_, symbol)| symbol).collect()
    }
}

/// One use of a word, as [`NavigationProvider::references`] reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reference {
    /// The word's bytes, in the file's text as the editor loads it, as
    /// [`Location`] has them, wherever that text could be read and still has
    /// the word on the line the search found it on. Otherwise (a file that
    /// changed since, couldn't be read again, or is too large to load) they
    /// are the file's bytes as the search read them; such a reference is
    /// never `verified`.
    pub location: Location,
    /// 1-based, as the search counts lines.
    pub line_number: u64,
    /// The line, or a window of it (see [`fe_project::LineMatch::preview`]).
    pub preview: String,
    /// The word's bytes in `preview`: one range, or none when the preview
    /// doesn't show it whole (a long line's window around another use, or,
    /// in a file that couldn't be loaded again, invalid UTF-8 before it).
    pub preview_ranges: Vec<Range<usize>>,
    /// Its file was parsed, and its highlights there, had in full, show
    /// the word isn't in a comment or a string. An unverified reference may
    /// be either.
    pub verified: bool,
    /// It's the name of a definition in its file, by [`fe_syntax::symbols`];
    /// only a verified reference can be.
    pub is_definition: bool,
}

impl Reference {
    /// The preview of a use at `hit`, bytes in `line`, a line of a file
    /// or a window of one around the use: `line`, or a window of it cut
    /// as search cuts its previews, and the use's bytes in it, if it shows
    /// it whole. For a provider with references of its own.
    pub fn preview(line: &str, hit: Range<usize>) -> (String, Vec<Range<usize>>) {
        let (preview, range) = crate::references::preview(line, 0, hit);
        (preview, range.into_iter().collect())
    }
}

/// One file's references, in line order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileReferences {
    /// Relative to the project root.
    pub rel: PathBuf,
    pub refs: Vec<Reference>,
}

/// How a find-references request went.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReferencesSummary {
    /// The word at the caret; `None` when there's none, and nothing was
    /// searched.
    pub word: Option<String>,
    /// Project files whose search began (the request's own file on disk
    /// included, though its hits come from the request's text).
    pub files_searched: usize,
    /// Files parsed to check their hits, the request's own included.
    pub files_verified: usize,
    /// Hits left out for being in a comment or a string.
    pub dropped: usize,
    /// The hit cap stopped the search: there may be more references.
    pub truncated: bool,
    /// The request's cancel flag was set before it ended.
    pub cancelled: bool,
}

/// Why a provider gave no answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavError {
    /// It can't answer now (a server crashed or timed out); the next
    /// provider is asked.
    Unavailable(String),
    /// The request's cancel flag was set.
    Cancelled,
    /// It doesn't do this operation; the next provider is asked.
    Unsupported,
}

impl fmt::Display for NavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NavError::Unavailable(why) => {
                f.write_str(&kiln_l10n::tr!("nav-unavailable", why = why.as_str()))
            }
            NavError::Cancelled => f.write_str(&kiln_l10n::tr!("nav-cancelled")),
            NavError::Unsupported => f.write_str(&kiln_l10n::tr!("nav-unsupported")),
        }
    }
}

impl std::error::Error for NavError {}

/// Which place related to a symbol [`NavigationProvider::related`] asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Related {
    Declaration,
    TypeDefinition,
    Implementation,
}

/// Answers navigation requests for a project.
///
/// Calls block, and run on a background thread the caller owns. `cancel` is
/// the request's: once it's set, the answer is no longer wanted, and a
/// provider should return [`NavError::Cancelled`] soon.
pub trait NavigationProvider: Send + Sync {
    /// Unique among the providers of a [`Navigation`]; it labels answers.
    fn name(&self) -> &str;

    fn precision(&self) -> Precision;

    /// Whether it can answer for files of `language` now (`None`: no
    /// language, or a request about no file, such as workspace symbols).
    fn readiness(&self, language: Option<&str>) -> Readiness;

    fn definitions(&self, req: &NavRequest, cancel: &AtomicBool) -> Result<Definitions, NavError>;

    /// The declarations, type definitions or implementations of the word at
    /// the request's caret, answered as [`definitions`](Self::definitions)
    /// are. A provider that can't keeps this default,
    /// [`NavError::Unsupported`], and the next provider is asked.
    fn related(
        &self,
        kind: Related,
        req: &NavRequest,
        cancel: &AtomicBool,
    ) -> Result<Definitions, NavError> {
        let _ = (kind, req, cancel);
        Err(NavError::Unsupported)
    }

    /// The uses of the word at the request's caret, handed to `sink` a batch
    /// of whole files at a time as they're found; the summary comes once
    /// they all have been. A provider that can't find references keeps this
    /// default, [`NavError::Unsupported`], and the next provider is asked.
    ///
    /// A cancel before any batch went to `sink` is [`NavError::Cancelled`];
    /// after one has, it's a summary with [`ReferencesSummary::cancelled`]
    /// set, and `sink` isn't called again.
    fn references(
        &self,
        req: &NavRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<ReferencesSummary, NavError> {
        let _ = (req, cancel, sink);
        Err(NavError::Unsupported)
    }

    /// The best `limit` symbols of the project matching `query`, best first.
    fn workspace_symbols(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError>;
}

/// An answer, and the provider that gave it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer<T> {
    /// The provider's [`NavigationProvider::name`].
    pub provider: String,
    pub precision: Precision,
    pub value: T,
}

/// Routes each request to one provider: the highest-precedence one that is
/// [`Readiness::Ready`] for the request's language.
///
/// Providers are added and removed through `&self`, so a host can register
/// a language server's provider after startup while requests run; a change
/// applies from the next request.
///
/// If the provider asked returns [`NavError::Unavailable`] or
/// [`NavError::Unsupported`], the next ready one is asked. Any other answer
/// is final, an empty `Ok` included: an exact provider saying there is no
/// definition isn't second-guessed by search. Answers are never merged.
#[derive(Default)]
pub struct Navigation {
    /// Best first: by precedence, highest first, then by when they were
    /// added.
    providers: RwLock<Vec<Entry>>,
}

#[derive(Clone)]
struct Entry {
    provider: Arc<dyn NavigationProvider>,
    precedence: i32,
}

impl Navigation {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `provider`, replacing any with the same name. Higher precedence
    /// is asked first; among equals, the one added first.
    pub fn add(&self, provider: Arc<dyn NavigationProvider>, precedence: i32) {
        let mut providers = self
            .providers
            .write()
            .unwrap_or_else(|err| err.into_inner());
        providers.retain(|entry| entry.provider.name() != provider.name());
        let at = providers.partition_point(|entry| entry.precedence >= precedence);
        providers.insert(
            at,
            Entry {
                provider,
                precedence,
            },
        );
    }

    /// Removes the provider named `name`. Returns whether there was one.
    pub fn remove(&self, name: &str) -> bool {
        let mut providers = self
            .providers
            .write()
            .unwrap_or_else(|err| err.into_inner());
        let before = providers.len();
        providers.retain(|entry| entry.provider.name() != name);
        providers.len() != before
    }

    /// The providers' names, in the order they're asked.
    pub fn providers(&self) -> Vec<String> {
        self.snapshot()
            .iter()
            .map(|entry| entry.provider.name().to_owned())
            .collect()
    }

    /// The definitions the word at the request's caret may mean.
    pub fn definitions(
        &self,
        req: &NavRequest,
        cancel: &AtomicBool,
    ) -> Result<Answer<Definitions>, NavError> {
        self.route(
            req.language,
            cancel,
            |provider| provider.definitions(req, cancel),
            || false,
        )
    }

    /// The declarations, type definitions or implementations the word at
    /// the request's caret may mean.
    pub fn related(
        &self,
        kind: Related,
        req: &NavRequest,
        cancel: &AtomicBool,
    ) -> Result<Answer<Definitions>, NavError> {
        self.route(
            req.language,
            cancel,
            |provider| provider.related(kind, req, cancel),
            || false,
        )
    }

    /// The uses of the word at the request's caret, handed to `sink` as
    /// they're found. A provider that fails after handing over a batch isn't
    /// followed by the next one, so answers are never mixed: its error is
    /// returned.
    pub fn references(
        &self,
        req: &NavRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<Answer<ReferencesSummary>, NavError> {
        let delivered = Cell::new(false);
        self.route(
            req.language,
            cancel,
            |provider| {
                provider.references(req, cancel, &mut |batch| {
                    delivered.set(true);
                    sink(batch);
                })
            },
            || delivered.get(),
        )
    }

    /// The best `limit` symbols of the project matching `query`.
    pub fn workspace_symbols(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Answer<Vec<WorkspaceSymbol>>, NavError> {
        self.route(
            None,
            cancel,
            |provider| provider.workspace_symbols(query, limit, cancel),
            || false,
        )
    }

    /// Asks the ready providers in order until one gives a final answer.
    /// With none ready, or none answering, the last fall-through error
    /// ([`NavError::Unsupported`] when none was asked). An error is final
    /// too once `answered` says the provider has handed part of an answer
    /// over.
    fn route<T>(
        &self,
        language: Option<&str>,
        cancel: &AtomicBool,
        mut ask: impl FnMut(&dyn NavigationProvider) -> Result<T, NavError>,
        answered: impl Fn() -> bool,
    ) -> Result<Answer<T>, NavError> {
        // Called without the lock held, so a slow provider never holds up
        // `add` or `remove`.
        let providers = self.snapshot();
        let mut last = NavError::Unsupported;
        for entry in &providers {
            if cancel.load(Ordering::Relaxed) {
                return Err(NavError::Cancelled);
            }
            let provider = &*entry.provider;
            if provider.readiness(language) != Readiness::Ready {
                continue;
            }
            match ask(provider) {
                Ok(value) => {
                    return Ok(Answer {
                        provider: provider.name().to_owned(),
                        precision: provider.precision(),
                        value,
                    });
                }
                Err(NavError::Cancelled) => return Err(NavError::Cancelled),
                Err(err) if answered() => return Err(err),
                Err(err) => last = err,
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(NavError::Cancelled);
        }
        Err(last)
    }

    fn snapshot(&self) -> Vec<Entry> {
        self.providers
            .read()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }
}

impl fmt::Debug for Navigation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Navigation")
            .field("providers", &self.providers())
            .finish()
    }
}
