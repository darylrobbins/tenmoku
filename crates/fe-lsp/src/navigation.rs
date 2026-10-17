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

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError, Weak};
use std::time::Duration;

use fe_buffer::{BufferId, Version};
use fe_nav::{
    Definition, Definitions, FileReferences, Location, NavError, NavRequest, NavigationProvider,
    OutlineKind, Precision, Readiness, Reference, ReferencesSummary, Tier, WorkspaceSymbol,
};
use fe_text::{Bias, Text};
use futures::FutureExt;
use futures::future::{self, Either};
use lsp_types::request::{GotoDefinition, References, WorkspaceSymbolRequest};
use lsp_types::{
    GotoDefinitionParams, GotoDefinitionResponse, OneOf, ReferenceContext, ReferenceParams,
    SymbolKind, TextDocumentIdentifier, TextDocumentPositionParams, Url, WorkspaceSymbolParams,
    WorkspaceSymbolResponse,
};

use crate::document::{Admitted, SentDocument};
use crate::encoding::{PositionEncoding, from_lsp, to_lsp};
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
const NOT_FINAL: &str = "empty answer from a server without a completion signal";

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

    /// `read` of the manager, if the host still has it. A request's future
    /// leaves it boxed: its type borrows the manager, though it doesn't.
    fn with_manager<T>(&self, read: impl FnOnce(&LspManager) -> T) -> Option<T> {
        let manager = self.manager.upgrade()?;
        let manager = manager.lock().unwrap_or_else(PoisonError::into_inner);
        Some(read(&manager))
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
        let (buffer, version) = request_buffer(req)?;
        let offset = req.offset;
        let sent = self.with_manager(|manager| {
            let reply = manager.document_request::<GotoDefinition>(buffer, version, move |at| {
                GotoDefinitionParams {
                    text_document_position_params: position(at, offset),
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                }
            });
            (reply.boxed(), manager.max_file_bytes())
        });
        let (reply, max_bytes) = sent.ok_or_else(gone)?;
        let (reply, admitted) = wait(reply, cancel)?;
        let targets = targets(reply.result);
        if targets.is_empty() && !reply.empty_is_final {
            return Err(NavError::Unavailable(NOT_FINAL.into()));
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
                        text_document_position: position(at, offset),
                        work_done_progress_params: Default::default(),
                        partial_result_params: Default::default(),
                        context: ReferenceContext {
                            include_declaration: true,
                        },
                    },
                    move |at| GotoDefinitionParams {
                        text_document_position_params: position(at, offset),
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
            return Err(NavError::Unavailable(NOT_FINAL.into()));
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
        let mut files: Vec<(Tier, FileReferences)> = Vec::new();
        for target in &found {
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
        check(cancel)?;
        if files.is_empty() && !decoder.dropped.is_empty() {
            return Err(NavError::Unavailable(decoder.dropped_why()));
        }
        files.sort_by(|(a, a_file), (b, b_file)| {
            (a != &Tier::SameFile, &a_file.rel).cmp(&(b != &Tier::SameFile, &b_file.rel))
        });
        let mut summary = ReferencesSummary {
            word,
            files_searched: files.len(),
            files_verified: files.len(),
            ..ReferencesSummary::default()
        };
        let mut delivered = false;
        for (_, mut file) in files {
            if cancel.load(Ordering::Relaxed) {
                if !delivered {
                    return Err(NavError::Cancelled);
                }
                summary.cancelled = true;
                break;
            }
            file.refs
                .sort_by_key(|reference| reference.location.range.start);
            file.refs
                .dedup_by(|a, b| a.location.range == b.location.range);
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
            return Err(NavError::Unavailable("no language server is ready".into()));
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
                return Err(NavError::Unavailable(NOT_FINAL.into()));
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
        .ok_or_else(|| NavError::Unavailable("the request names no buffer".into()))
}

fn check(cancel: &AtomicBool) -> Result<(), NavError> {
    match cancel.load(Ordering::Relaxed) {
        true => Err(NavError::Cancelled),
        false => Ok(()),
    }
}

fn gone() -> NavError {
    NavError::Unavailable("the language servers have gone".into())
}

fn unavailable(err: LspError) -> NavError {
    NavError::Unavailable(err.to_string())
}

/// The request's document and caret, as the server has them.
fn position(at: &Admitted, offset: usize) -> TextDocumentPositionParams {
    let text = &at.document.sent.text;
    TextDocumentPositionParams {
        text_document: TextDocumentIdentifier {
            uri: at.document.uri.clone(),
        },
        position: to_lsp(text, text.clip_offset(offset, Bias::Left), at.encoding),
    }
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
    own: Option<(&'a Url, &'a Path, &'a Text, (BufferId, Version))>,
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
        self.own = Some((&admitted.document.uri, &req.rel, &req.text, snapshot));
        self
    }

    fn with_open(mut self, open: &'a [SentDocument]) -> Self {
        self.open = open;
        self
    }

    fn dropped_why(&self) -> String {
        let (uri, why) = &self.dropped[0];
        format!("{uri}: {why}")
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
            Some((own, rel, text, snapshot)) if own == uri || same_path(own) => {
                (rel.to_owned(), text.clone(), Some(snapshot), Tier::SameFile)
            }
            _ => {
                let path = match decoded.clone() {
                    Ok(path) => path,
                    Err(_) => {
                        self.dropped.push((uri.clone(), "not a file".into()));
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
                            self.dropped.push((uri.clone(), "couldn't be read".into()));
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
    let nonblocking = rustix::fs::OFlags::NONBLOCK.bits() as i32;
    let file = (File::options().read(true).custom_flags(nonblocking))
        .open(path)
        .ok()?;
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
