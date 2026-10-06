//! The [`NavigationProvider`] seam and the [`Navigation`] router.
//!
//! A provider answers go-to-definition and workspace symbols: by search
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
    fmt,
    ops::Range,
    path::PathBuf,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use fe_buffer::Text;
use fe_syntax::OutlineKind;

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
}

/// A place in a project file.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Location {
    /// Relative to the project root.
    pub rel: PathBuf,
    /// Bytes in the file's text as the editor loads it (see
    /// [`crate::Symbol`]). For the request's own file, bytes in
    /// [`NavRequest::text`].
    pub range: Range<usize>,
    /// The 0-based row where `range` starts.
    pub row: u32,
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
    pub kind: OutlineKind,
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
            NavError::Unavailable(why) => write!(f, "unavailable: {why}"),
            NavError::Cancelled => f.write_str("cancelled"),
            NavError::Unsupported => f.write_str("unsupported"),
        }
    }
}

impl std::error::Error for NavError {}

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
        self.route(req.language, cancel, |provider| {
            provider.definitions(req, cancel)
        })
    }

    /// The best `limit` symbols of the project matching `query`.
    pub fn workspace_symbols(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Answer<Vec<WorkspaceSymbol>>, NavError> {
        self.route(None, cancel, |provider| {
            provider.workspace_symbols(query, limit, cancel)
        })
    }

    /// Asks the ready providers in order until one gives a final answer.
    /// With none ready, or none answering, the last fall-through error
    /// ([`NavError::Unsupported`] when none was asked).
    fn route<T>(
        &self,
        language: Option<&str>,
        cancel: &AtomicBool,
        ask: impl Fn(&dyn NavigationProvider) -> Result<T, NavError>,
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
