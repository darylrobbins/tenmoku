//! [`LspNavigation`]: go-to-definition, references and workspace symbols
//! from language servers, through fe-nav's [`NavigationProvider`] seam
//! (docs/decisions/lsp-client.md §5).
//!
//! - **One provider for every language**, bound to a manager and its
//!   workspace root. It asks the manager which server documents of a
//!   request's language go to, and is [`Readiness::Ready`] only once that
//!   server is [`ServerStatus::Ready`]: until then search answers.
//! - **Calls block** on the caller's thread, polling its cancel flag every
//!   [`POLL`]. A cancel drops the request's future, which sends
//!   `$/cancelRequest`, and answers [`NavError::Cancelled`].
//! - **Requests on a document** go out only if the server has the version
//!   the request was made on (§4.1, through [`LspManager::document_request`]).
//!   Anything that keeps the server from answering (no buffer, a buffer
//!   that moved on, a file too large to sync, a timeout, a crash, a
//!   `ContentModified`) is [`NavError::Unavailable`], so search answers.
//!   So is an empty answer, unless the server has said it's done
//!   ([`Reply::empty_is_final`]); a non-empty one is final.
//! - **Offsets** (§5): a location in the request's own file is decoded
//!   against [`NavRequest::text`], one in another open document against
//!   the text the server held when the request went out, and both carry
//!   that buffer version as [`Location::snapshot`]. An unopened file is
//!   read as the editor's loader reads it, once per answer.

use kiln_l10n::tr;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError, Weak};
use std::time::Duration;

use fe_buffer::{BufferId, Version};
use fe_nav::{
    Definition, Definitions, FileReferences, Location, NavError, NavRequest, NavigationProvider,
    OutlineKind, Precision, Readiness, Reference, ReferencesSummary, Related, Tier,
    WorkspaceSymbol,
};
use fe_text::{Bias, Text};
use futures::FutureExt;
use futures::future::{self, Either};
use lsp_types::request::{
    GotoDeclaration, GotoDefinition, GotoImplementation, GotoTypeDefinition, References, Request,
    WorkspaceSymbolRequest,
};
use lsp_types::{
    DeclarationCapability, GotoDefinitionParams, GotoDefinitionResponse,
    ImplementationProviderCapability, OneOf, ReferenceContext, ReferenceParams, ServerCapabilities,
    SymbolKind, TypeDefinitionProviderCapability, Url, WorkspaceSymbolParams,
    WorkspaceSymbolResponse,
};

use crate::document::{Admitted, SentDocument};
use crate::encoding::{PositionEncoding, from_lsp};
use crate::event::{LspError, ServerStatus};
use crate::manager::LspManager;
use crate::workspace_edit::file_path;

/// How often a blocked call checks its cancel flag.
pub(crate) const POLL: Duration = Duration::from_millis(10);

/// The most bytes either side of a reference its preview looks at, as
/// search cuts previews.
const PREVIEW_BYTES: usize = 512;

/// The longest name a definition takes from its target's text; a longer
/// (or multi-line) range is a whole item, named by the word asked about.
const NAME_BYTES: usize = 256;

/// Why an empty answer gives way to search (§3.4).
fn not_final() -> String {
    tr!("lsp-nav-not-final").to_string()
}

/// Answers navigation requests from the language servers of an
/// [`LspManager`], for its workspace at one root.
///
/// It holds the manager weakly, behind the host's mutex, and locks it only
/// to send a request or read a status, never while waiting: once the host
/// drops the manager, it answers nothing.
pub struct LspNavigation {
    manager: Weak<Mutex<LspManager>>,
    root: PathBuf,
}

impl LspNavigation {
    /// Its [`NavigationProvider::name`].
    pub const NAME: &'static str = "language server";

    /// Answers from `manager`'s servers for the workspace at `root`, as
    /// the manager has it (absolute, resolved).
    pub fn new(manager: Weak<Mutex<LspManager>>, root: impl Into<PathBuf>) -> Self {
        Self {
            manager,
            root: root.into(),
        }
    }

    /// A definition-shaped request `R`, decoded as definitions are.
    ///
    /// With `provided`, the request is sent only once the server's
    /// capabilities are known, and if they say it provides the feature.
    fn goto<R>(
        &self,
        req: &NavRequest,
        cancel: &AtomicBool,
        provided: Option<Provided>,
    ) -> Result<Definitions, NavError>
    where
        R: Request<Params = GotoDefinitionParams, Result = Option<GotoDefinitionResponse>>,
    {
        let (buffer, version) = request_buffer(req)?;
        let offset = req.offset;
        let sent = self.with_manager(|manager| {
            let params = move |at: &Admitted| GotoDefinitionParams {
                text_document_position_params: at.position(offset),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            };
            let reply = match provided {
                Some((feature, provides)) => manager
                    .document_request_if_provided::<R>(buffer, version, feature, provides, params)
                    .boxed(),
                None => manager
                    .document_request::<R>(buffer, version, params)
                    .boxed(),
            };
            (reply, manager.max_file_bytes())
        });
        let (reply, max_bytes) = sent.ok_or_else(gone)?;
        let (reply, admitted) = wait(reply, cancel).map_err(|err| match (err, provided) {
            (NavError::Unavailable(why), Some((feature, _)))
                if why == LspError::Unavailable(feature.into()).to_string() =>
            {
                NavError::Unsupported
            }
            (err, _) => err,
        })?;
        let targets = targets(reply.result);
        if targets.is_empty() && !reply.empty_is_final {
            return Err(NavError::Unavailable(not_final()));
        }
        let word = req.word();
        let mut decoder = Decoder::new(&self.root, admitted.encoding, max_bytes, cancel)
            .with_request(req, &admitted)
            .with_open(&admitted.others);
        let items: Vec<Definition> = (targets.into_iter())
            .filter_map(|(uri, range)| decoder.decode(&uri, range))
            .map(|found| {
                let name = found.name().or_else(|| word.clone()).unwrap_or_default();
                Definition {
                    location: found.location,
                    name,
                    kind: None,
                    container: None,
                    tier: found.tier,
                }
            })
            .collect();
        check(cancel)?;
        if items.is_empty() && !decoder.dropped.is_empty() {
            return Err(NavError::Unavailable(decoder.dropped_why()));
        }
        Ok(Definitions {
            word,
            unambiguous: items.len() == 1,
            items,
            partial: false,
        })
    }

    /// `read` of the manager, if the host still has it. A request's future
    /// leaves it boxed: its type borrows the manager, though it doesn't.
    fn with_manager<T>(&self, read: impl FnOnce(&LspManager) -> T) -> Option<T> {
        let manager = self.manager.upgrade()?;
        let manager = manager.lock().unwrap_or_else(PoisonError::into_inner);
        Some(read(&manager))
    }
}

impl LspNavigation {
    /// The locations a lens's `editor.action.showReferences` or
    /// `rust-analyzer.showReferences` command lists, its third argument
    /// (after the lens's document and position); `None` for any other
    /// command, or arguments that aren't those.
    pub fn lens_locations(command: &lsp_types::Command) -> Option<Vec<lsp_types::Location>> {
        if !Self::is_references_command(command) {
            return None;
        }
        let locations = command.arguments.as_ref()?.get(2)?.clone();
        serde_json::from_value(locations).ok()
    }

    /// Whether `command` is one of the client commands that list references
    /// (`lens_locations` parses their arguments).
    pub fn is_references_command(command: &lsp_types::Command) -> bool {
        matches!(
            command.command.as_str(),
            "editor.action.showReferences" | "rust-analyzer.showReferences"
        )
    }

    /// `locations`, a server's, decoded and grouped as [`references`]
    /// does its reply, for an answer the host already has (a code lens's):
    /// the files in path order, with previews and no definition marks.
    ///
    /// `origin` is the lens's [`context`](crate::CodeLens::context): the
    /// server's text of the lens's file and of its other open documents as
    /// they were when it was asked for, and its position encoding, decode
    /// the locations, and the lens's file is placed first; files the server
    /// doesn't have open are read from disk.
    ///
    /// [`references`]: NavigationProvider::references
    pub fn decode_references(
        &self,
        root: &Path,
        origin: &Admitted,
        locations: Vec<lsp_types::Location>,
        cancel: &AtomicBool,
    ) -> Result<Vec<FileReferences>, NavError> {
        let max_bytes = self
            .with_manager(|manager| manager.max_file_bytes())
            .ok_or_else(gone)?;
        let mut decoder = Decoder::new(root, origin.encoding, max_bytes, cancel)
            .with_origin(origin)
            .with_open(&origin.others);
        let files = group_references(&mut decoder, &locations, &[]);
        check(cancel)?;
        if files.is_empty() && !decoder.dropped.is_empty() {
            return Err(NavError::Unavailable(decoder.dropped_why()));
        }
        Ok(sorted(files))
    }
}

impl NavigationProvider for LspNavigation {
    fn name(&self) -> &str {
        Self::NAME
    }

    fn precision(&self) -> Precision {
        Precision::Exact
    }

    /// For a language: as its server is (§5). With none (workspace
    /// symbols): ready once any server for the root is.
    fn readiness(&self, language: Option<&str>) -> Readiness {
        let Some(language) = language else {
            let servers = self.with_manager(|manager| manager.servers_at(&self.root));
            let statuses = servers.unwrap_or_default().into_iter();
            return (statuses.map(|(_, status)| readiness(&status)))
                .min_by_key(|readiness| match readiness {
                    Readiness::Ready => 0,
                    Readiness::NotYet => 1,
                    Readiness::Unsupported => 2,
                })
                .unwrap_or(Readiness::Unsupported);
        };
        let server = self.with_manager(|manager| manager.language_server(&self.root, language));
        match server.flatten() {
            None => Readiness::Unsupported,
            // Configured, and started by the first document that needs it.
            Some((_, None)) => Readiness::NotYet,
            Some((_, Some(status))) => readiness(&status),
        }
    }

    fn definitions(&self, req: &NavRequest, cancel: &AtomicBool) -> Result<Definitions, NavError> {
        self.goto::<GotoDefinition>(req, cancel, None)
    }

    /// The server's declarations, type definitions or implementations, if
    /// it provides them; one that doesn't is asked nothing.
    fn related(
        &self,
        kind: Related,
        req: &NavRequest,
        cancel: &AtomicBool,
    ) -> Result<Definitions, NavError> {
        match kind {
            Related::Declaration => {
                self.goto::<GotoDeclaration>(req, cancel, Some(("declaration", declares)))
            }
            Related::TypeDefinition => self.goto::<GotoTypeDefinition>(
                req,
                cancel,
                Some(("typeDefinition", defines_types)),
            ),
            Related::Implementation => {
                self.goto::<GotoImplementation>(req, cancel, Some(("implementation", implements)))
            }
        }
    }

    /// The server's references, with a definition request alongside to
    /// mark which are definitions, handed over a file at a time, the
    /// request's own first, then the others by path.
    fn references(
        &self,
        req: &NavRequest,
        cancel: &AtomicBool,
        sink: &mut dyn FnMut(Vec<FileReferences>),
    ) -> Result<ReferencesSummary, NavError> {
        let (buffer, version) = request_buffer(req)?;
        let offset = req.offset;
        let sent = self.with_manager(|manager| {
            // Admitted together, so both are asked of the same text.
            let (references, definitions) = manager
                .document_request_pair::<References, GotoDefinition>(
                    buffer,
                    version,
                    move |at| ReferenceParams {
                        text_document_position: at.position(offset),
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                        context: ReferenceContext {
                            include_declaration: true,
                        },
                    },
                    move |at| GotoDefinitionParams {
                        text_document_position_params: at.position(offset),
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    },
                );
            (references, definitions, manager.max_file_bytes())
        });
        let (references, definitions, max_bytes) = sent.ok_or_else(gone)?;
        // The references decide, as soon as they come: a failure or an
        // empty answer doesn't wait for the definitions, which only mark
        // definitions among them (none, if they fail). The definitions are
        // polled meanwhile, so their own timeout runs (and cancels them);
        // dropping a future cancels its request.
        let mut definitions = definitions.fuse();
        let mut defined = None;
        let first = async {
            let mut references = references.fuse();
            loop {
                futures::select_biased! {
                    answer = references => return answer,
                    answer = definitions => defined = Some(answer),
                }
            }
        };
        let (reply, admitted) = wait(first, cancel)?;
        let found = reply.result.unwrap_or_default();
        let word = req.word();
        if found.is_empty() && !reply.empty_is_final {
            return Err(NavError::Unavailable(not_final()));
        }
        let definitions = match (found.is_empty(), defined) {
            (true, _) => None,
            (false, Some(answer)) => answer.ok(),
            (false, None) => wait(definitions, cancel)
                .map(Some)
                .or_else(|err| match err {
                    NavError::Cancelled => Err(err),
                    _ => Ok(None),
                })?,
        };
        let mut decoder = Decoder::new(&self.root, admitted.encoding, max_bytes, cancel)
            .with_request(req, &admitted)
            .with_open(&admitted.others);
        let defined: Vec<(PathBuf, std::ops::Range<usize>)> = definitions
            .map(|reply| targets(reply.result))
            .unwrap_or_default()
            .into_iter()
            .filter_map(|(uri, range)| decoder.decode(&uri, range))
            .map(|found| (found.location.rel, found.location.range))
            .collect();
        // A definition that can't be read only goes unmarked: it doesn't
        // make the references unavailable.
        decoder.dropped.clear();
        let files = group_references(&mut decoder, &found, &defined);
        check(cancel)?;
        if files.is_empty() && !decoder.dropped.is_empty() {
            return Err(NavError::Unavailable(decoder.dropped_why()));
        }
        let files = sorted(files);
        let mut summary = ReferencesSummary {
            word,
            files_searched: files.len(),
            files_verified: files.len(),
            ..ReferencesSummary::default()
        };
        let mut delivered = false;
        for file in files {
            if cancel.load(Ordering::Relaxed) {
                if !delivered {
                    return Err(NavError::Cancelled);
                }
                summary.cancelled = true;
                break;
            }
            sink(vec![file]);
            delivered = true;
        }
        Ok(summary)
    }

    /// `workspace/symbol` from every server ready for the root, ranked
    /// together by search's fuzzy matcher before the best `limit` are kept.
    fn workspace_symbols(
        &self,
        query: &str,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<Vec<WorkspaceSymbol>, NavError> {
        // As search: an empty query matches nothing.
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let sent = self.with_manager(|manager| {
            let ready = (manager.servers_at(&self.root).into_iter())
                .filter(|(_, status)| *status == ServerStatus::Ready);
            let replies: Vec<_> = ready
                .map(|(server, _)| {
                    let params = WorkspaceSymbolParams {
                        query: query.to_owned(),
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                    };
                    manager
                        .request_with_documents::<WorkspaceSymbolRequest>(server, params)
                        .boxed()
                })
                .collect();
            (replies, manager.max_file_bytes())
        });
        let (replies, max_bytes) = sent.ok_or_else(gone)?;
        if replies.is_empty() {
            return Err(NavError::Unavailable(tr!("lsp-nav-none-ready").to_string()));
        }
        let answers = wait(
            async move { Ok::<_, LspError>(future::join_all(replies).await) },
            cancel,
        )?;
        let mut symbols = Vec::new();
        let mut empty_is_final = true;
        let mut failed = None;
        // Why a symbol was left out, if one was: its file couldn't be read.
        let mut dropped = None;
        for answer in answers {
            let (reply, opened) = match answer {
                Ok(answer) => answer,
                Err(err) => {
                    failed = Some(err);
                    continue;
                }
            };
            empty_is_final &= reply.empty_is_final;
            let found = flatten(reply.result);
            let mut decoder = Decoder::new(&self.root, opened.encoding, max_bytes, cancel)
                .with_open(&opened.documents);
            for (name, kind, container, uri, range) in found {
                if let Some(found) = decoder.decode(&uri, range) {
                    symbols.push(WorkspaceSymbol {
                        location: found.location,
                        name,
                        kind: outline_kind(kind),
                        container,
                        indices: Vec::new(),
                    });
                }
            }
            if dropped.is_none() && !decoder.dropped.is_empty() {
                dropped = Some(decoder.dropped_why());
            }
        }
        check(cancel)?;
        if symbols.is_empty() {
            // Not a final "none": the symbols it gave couldn't be shown.
            if let Some(why) = dropped {
                return Err(NavError::Unavailable(why));
            }
            if let Some(err) = failed {
                return Err(unavailable(err));
            }
            if !empty_is_final {
                return Err(NavError::Unavailable(not_final()));
            }
        }
        Ok(WorkspaceSymbol::rank(query, symbols, limit))
    }
}

/// What a server's status means for routing (§5).
fn readiness(status: &ServerStatus) -> Readiness {
    match status {
        ServerStatus::Ready => Readiness::Ready,
        ServerStatus::Stopped(_) => Readiness::Unsupported,
        _ => Readiness::NotYet,
    }
}

/// The buffer and version the request was made on: without them, a server
/// can't be asked (§5 gap 2).
fn request_buffer(req: &NavRequest) -> Result<(BufferId, Version), NavError> {
    // A file of no language has no server: `readiness(None)` speaks for
    // workspace symbols, which name no file.
    if req.language.is_none() {
        return Err(NavError::Unsupported);
    }
    req.buffer
        .ok_or_else(|| NavError::Unavailable(tr!("lsp-nav-no-buffer").to_string()))
}

fn check(cancel: &AtomicBool) -> Result<(), NavError> {
    match cancel.load(Ordering::Relaxed) {
        true => Err(NavError::Cancelled),
        false => Ok(()),
    }
}

fn gone() -> NavError {
    NavError::Unavailable(tr!("lsp-nav-gone").to_string())
}

fn unavailable(err: LspError) -> NavError {
    NavError::Unavailable(err.to_string())
}

/// Blocks on `answer`, checking `cancel` every [`POLL`]. A cancel drops
/// `answer`, which cancels its requests.
fn wait<T>(
    answer: impl Future<Output = Result<T, LspError>>,
    cancel: &AtomicBool,
) -> Result<T, NavError> {
    let mut answer = pin!(answer);
    async_io::block_on(async {
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(NavError::Cancelled);
            }
            let tick = async_io::Timer::after(POLL);
            if let Either::Left((answer, _)) = future::select(answer.as_mut(), tick).await {
                return answer.map_err(|err| match err {
                    // The server's own cancel isn't the user's.
                    _ if cancel.load(Ordering::Relaxed) => NavError::Cancelled,
                    err => unavailable(err),
                });
            }
        }
    })
}

/// A definition reply's targets: a link's name (its selection range), or
/// a location's range.
fn targets(reply: Option<GotoDefinitionResponse>) -> Vec<(Url, lsp_types::Range)> {
    match reply {
        None => Vec::new(),
        Some(GotoDefinitionResponse::Scalar(location)) => vec![(location.uri, location.range)],
        Some(GotoDefinitionResponse::Array(locations)) => (locations.into_iter())
            .map(|location| (location.uri, location.range))
            .collect(),
        Some(GotoDefinitionResponse::Link(links)) => (links.into_iter())
            .map(|link| (link.target_uri, link.target_selection_range))
            .collect(),
    }
}

type Found = (String, SymbolKind, Option<String>, Url, lsp_types::Range);

/// A workspace symbol reply's symbols, in order. One with no range (a
/// server expecting `workspaceSymbol/resolve`, which isn't offered) is at
/// its file's start.
fn flatten(reply: Option<WorkspaceSymbolResponse>) -> Vec<Found> {
    match reply {
        None => Vec::new(),
        Some(WorkspaceSymbolResponse::Flat(symbols)) => (symbols.into_iter())
            .map(|symbol| {
                let location = symbol.location;
                let container = symbol.container_name;
                (
                    symbol.name,
                    symbol.kind,
                    container,
                    location.uri,
                    location.range,
                )
            })
            .collect(),
        Some(WorkspaceSymbolResponse::Nested(symbols)) => (symbols.into_iter())
            .map(|symbol| {
                let (uri, range) = match symbol.location {
                    OneOf::Left(location) => (location.uri, location.range),
                    OneOf::Right(location) => (location.uri, lsp_types::Range::default()),
                };
                (symbol.name, symbol.kind, symbol.container_name, uri, range)
            })
            .collect(),
    }
}

/// fe-syntax's nearest kind for an LSP symbol kind.
fn outline_kind(kind: SymbolKind) -> OutlineKind {
    match kind {
        SymbolKind::FILE | SymbolKind::MODULE | SymbolKind::NAMESPACE | SymbolKind::PACKAGE => {
            OutlineKind::Module
        }
        SymbolKind::CLASS => OutlineKind::Class,
        SymbolKind::METHOD | SymbolKind::CONSTRUCTOR => OutlineKind::Method,
        SymbolKind::PROPERTY | SymbolKind::FIELD | SymbolKind::EVENT => OutlineKind::Field,
        SymbolKind::ENUM => OutlineKind::Enum,
        SymbolKind::INTERFACE => OutlineKind::Interface,
        SymbolKind::FUNCTION | SymbolKind::OPERATOR => OutlineKind::Function,
        SymbolKind::CONSTANT => OutlineKind::Constant,
        SymbolKind::ENUM_MEMBER => OutlineKind::Variant,
        SymbolKind::STRUCT => OutlineKind::Struct,
        SymbolKind::TYPE_PARAMETER => OutlineKind::Type,
        _ => OutlineKind::Variable,
    }
}

/// The line of `range`'s start in `text`, or a window of it around
/// `range`, as search previews a hit, and `range` in it.
fn preview(text: &Text, range: std::ops::Range<usize>) -> (String, Vec<std::ops::Range<usize>>) {
    let row = text.offset_to_point(range.start).row;
    let row_start = text.row_start(row);
    let row_end = row_start + text.row_len(row) as usize;
    let end = range.end.min(row_end);
    let from = text.clip_offset(
        row_start.max(range.start.saturating_sub(PREVIEW_BYTES)),
        Bias::Left,
    );
    let to = text.clip_offset(row_end.min(end + PREVIEW_BYTES), Bias::Right);
    let line = text.slice_to_string(from..to);
    Reference::preview(&line, range.start - from..end - from)
}

/// `found` decoded and grouped by file, each reference marked a definition
/// if it is one of `defined`.
fn group_references(
    decoder: &mut Decoder<'_>,
    found: &[lsp_types::Location],
    defined: &[(PathBuf, std::ops::Range<usize>)],
) -> Vec<(Tier, FileReferences)> {
    let mut files: Vec<(Tier, FileReferences)> = Vec::new();
    for target in found {
        let Some(found) = decoder.decode(&target.uri, target.range) else {
            continue;
        };
        let at = &found.location;
        let is_definition =
            (defined.iter()).any(|(rel, range)| *rel == at.rel && *range == at.range);
        let (preview, preview_ranges) = preview(&found.text, at.range.clone());
        let reference = Reference {
            line_number: u64::from(at.row) + 1,
            preview,
            preview_ranges,
            verified: true,
            is_definition,
            location: found.location,
        };
        match files
            .iter_mut()
            .find(|(_, file)| file.rel == reference.location.rel)
        {
            Some((_, file)) => file.refs.push(reference),
            None => files.push((
                found.tier,
                FileReferences {
                    rel: reference.location.rel.clone(),
                    refs: vec![reference],
                },
            )),
        }
    }
    files
}

/// `files` in the order references hands them over (the request's own file
/// first, then by path), each file's references in order and without
/// repeats.
fn sorted(mut files: Vec<(Tier, FileReferences)>) -> Vec<FileReferences> {
    files.sort_by(|(a, a_file), (b, b_file)| {
        (a != &Tier::SameFile, &a_file.rel).cmp(&(b != &Tier::SameFile, &b_file.rel))
    });
    (files.into_iter())
        .map(|(_, mut file)| {
            file.refs
                .sort_by_key(|reference| reference.location.range.start);
            file.refs
                .dedup_by(|a, b| a.location.range == b.location.range);
            file
        })
        .collect()
}

/// A location decoded, with the text it's in.
struct Decoded {
    location: Location,
    tier: Tier,
    text: Text,
}

impl Decoded {
    /// What the location names: its text, if that's on one line and
    /// short.
    fn name(&self) -> Option<String> {
        let range = self.location.range.clone();
        let one_line = self.text.offset_to_point(range.end).row == self.location.row;
        (!range.is_empty() && range.len() <= NAME_BYTES && one_line)
            .then(|| self.text.slice_to_string(range))
    }
}

/// Decodes a reply's locations, each against the text it's in (§5
/// "Offsets").
struct Decoder<'a> {
    root: &'a Path,
    encoding: PositionEncoding,
    max_bytes: usize,
    /// The request's own document: where the server has it, where the
    /// workbench has it, its text and its snapshot.
    own: Option<(&'a Url, PathBuf, &'a Text, (BufferId, Version))>,
    /// The other documents the server had open.
    open: &'a [SentDocument],
    /// Unopened files read for this answer: `None` for one that couldn't
    /// be.
    read: HashMap<PathBuf, Option<Text>>,
    /// Files a location was left out for, and why.
    dropped: Vec<(Url, String)>,
    /// Once set, nothing more is decoded (or read).
    cancel: &'a AtomicBool,
}

impl<'a> Decoder<'a> {
    fn new(
        root: &'a Path,
        encoding: PositionEncoding,
        max_bytes: usize,
        cancel: &'a AtomicBool,
    ) -> Self {
        Self {
            cancel,
            root,
            encoding,
            max_bytes,
            own: None,
            open: &[],
            read: HashMap::new(),
            dropped: Vec::new(),
        }
    }

    /// Decodes the request's own file against its text, which is what the
    /// server had (it was admitted at that version).
    fn with_request(mut self, req: &'a NavRequest, admitted: &'a Admitted) -> Self {
        let snapshot = (admitted.document.buffer, admitted.document.sent.version);
        self.own = Some((&admitted.document.uri, req.rel.clone(), &req.text, snapshot));
        self
    }

    /// As [`with_request`](Self::with_request), for a document the host
    /// has no request of: its file is the uri's, relative to the root.
    fn with_origin(mut self, admitted: &'a Admitted) -> Self {
        let snapshot = (admitted.document.buffer, admitted.document.sent.version);
        let document = &admitted.document;
        if let Ok(path) = file_path(&document.uri) {
            let rel = path
                .strip_prefix(self.root)
                .map_or_else(|_| path.clone(), Path::to_owned);
            self.own = Some((&document.uri, rel, &document.sent.text, snapshot));
        }
        self
    }

    fn with_open(mut self, open: &'a [SentDocument]) -> Self {
        self.open = open;
        self
    }

    fn dropped_why(&self) -> String {
        let (uri, why) = &self.dropped[0];
        tr!("lsp-nav-dropped", uri = uri.to_string(), why = why.as_str()).to_string()
    }

    /// `range` in the file at `uri`, or `None` (recorded in `dropped`) if
    /// the file isn't one that can be read; `None` for all once the
    /// request is cancelled, which the caller checks for.
    fn decode(&mut self, uri: &Url, range: lsp_types::Range) -> Option<Decoded> {
        if self.cancel.load(Ordering::Relaxed) {
            return None;
        }
        // A server may spell the same file with other percent-escapes than
        // ours (`%5Bslug%5D` for `[slug]`), so files match by decoded path
        // as well as by URI.
        let decoded = file_path(uri);
        let same_path = |other: &Url| matches!((&decoded, file_path(other)), (Ok(path), Ok(other)) if *path == other);
        let (rel, text, snapshot, tier) = match self.own {
            Some((own, ref rel, text, snapshot)) if own == uri || same_path(own) => {
                (rel.clone(), text.clone(), Some(snapshot), Tier::SameFile)
            }
            _ => {
                let path = match decoded.clone() {
                    Ok(path) => path,
                    Err(_) => {
                        self.dropped
                            .push((uri.clone(), tr!("lsp-nav-not-a-file").to_string()));
                        return None;
                    }
                };
                let rel = path
                    .strip_prefix(self.root)
                    .map_or_else(|_| path.clone(), Path::to_owned);
                let found = (self.open.iter())
                    .find(|document| document.uri == *uri || same_path(&document.uri));
                match found {
                    Some(document) => {
                        let snapshot = (document.buffer, document.sent.version);
                        (
                            rel,
                            document.sent.text.clone(),
                            Some(snapshot),
                            Tier::Project,
                        )
                    }
                    None => {
                        let max_bytes = self.max_bytes;
                        let read = self
                            .read
                            .entry(path)
                            .or_insert_with_key(|path| load(path, max_bytes));
                        let Some(text) = read.clone() else {
                            self.dropped
                                .push((uri.clone(), tr!("lsp-nav-unreadable").to_string()));
                            return None;
                        };
                        (rel, text, None, Tier::Project)
                    }
                }
            }
        };
        let start = from_lsp(&text, range.start, self.encoding);
        let end = from_lsp(&text, range.end, self.encoding).max(start);
        let location = Location {
            rel,
            range: start..end,
            row: text.offset_to_point(start).row,
            snapshot,
        };
        Some(Decoded {
            location,
            tier,
            text,
        })
    }
}

/// The text of the file at `path`, decoded as the editor's loader
/// (`loader::read`) does (a BOM dropped, a legacy encoding detected), if it's
/// a regular file no larger than `max_bytes`. Opened without blocking, so a
/// FIFO in its place can't hang the request.
fn load(path: &Path, max_bytes: usize) -> Option<Text> {
    let mut options = File::options();
    options.read(true);
    // Windows has no FIFOs to wait on.
    #[cfg(unix)]
    options.custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32);
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > max_bytes as u64 {
        return None;
    }
    // It may have grown since.
    let mut limited = file.take(max_bytes as u64 + 1);
    let mut raw = Vec::new();
    limited.read_to_end(&mut raw).ok()?;
    if raw.len() > max_bytes {
        return None;
    }
    Text::from_reader_detecting(raw.as_slice())
        .ok()
        .map(|decoded| decoded.text)
}

/// A feature's name, and whether a server's capabilities provide it.
type Provided = (&'static str, fn(&ServerCapabilities) -> bool);

/// Whether the server provides declarations.
fn declares(caps: &ServerCapabilities) -> bool {
    (caps.declaration_provider.as_ref())
        .is_some_and(|provider| !matches!(provider, DeclarationCapability::Simple(false)))
}

/// Whether the server provides type definitions.
fn defines_types(caps: &ServerCapabilities) -> bool {
    (caps.type_definition_provider.as_ref()).is_some_and(|provider| {
        !matches!(provider, TypeDefinitionProviderCapability::Simple(false))
    })
}

/// Whether the server provides implementations.
fn implements(caps: &ServerCapabilities) -> bool {
    (caps.implementation_provider.as_ref()).is_some_and(|provider| {
        !matches!(provider, ImplementationProviderCapability::Simple(false))
    })
}
