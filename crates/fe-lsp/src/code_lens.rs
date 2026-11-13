//! `textDocument/codeLens` and `codeLens/resolve`: the lenses a server has
//! for a document, as ranges with the command each one runs.
//!
//! - **Servers.** Merged (docs/decisions/q5j03ax2af-lsp-add-on-servers.md
//!   §5): every server of the document with `codeLensProvider` is asked at
//!   once, at the one version; one without gets no request, and with none
//!   the answer is `Ok(None)`. Lenses are never deduped: each keeps its
//!   server, which alone resolves it and runs its command. A server that
//!   fails or times out contributes nothing; the answer fails only if
//!   every server did.
//! - **Positions.** A lens's range is decoded against the text the server
//!   had at the request's version, as bytes of that version. Lenses are
//!   sorted by where their range starts, then by server order.
//! - **Resolve.** Only a lens without a `command` is resolved, only from a
//!   server whose provider has a `resolveProvider`, and only by the session
//!   that gave the lens, while it has that opening of the document.
//! - **Failures.** A request past `lsp.timeouts.hover` (3 s) is
//!   `Ok(Some([]))`, a resolve past `lsp.timeouts.resolve` (2 s) is the
//!   lens as it was. Both cancel the request.
//! - **Command.** A lens's command its server lists in
//!   `executeCommandProvider` goes to it as `workspace/executeCommand`
//!   ([`LspManager::run_lens_command`]); any other, such as the client
//!   command `rust-analyzer.showReferences`, is the host's to run and is
//!   never sent.

use kiln_l10n::tr;
use std::ops::Range;
use std::sync::Arc;

use fe_buffer::{BufferId, Version};
use futures::FutureExt;
use lsp_types::request::{CodeLensRequest, CodeLensResolve};
use lsp_types::{CodeLensParams, ServerCapabilities, TextDocumentIdentifier};

use crate::document::Admitted;
use crate::encoding::from_lsp;
use crate::event::{LspError, ServerId};
use crate::identity::{IncarnationId, SessionId};
use crate::manager::LspManager;
use crate::requests::{Gate, merged};

/// One lens a server has for a document.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct CodeLens {
    /// What it sits on, in bytes of the version the request was made on.
    pub range: Range<usize>,
    /// What it shows and runs; none until
    /// [`resolve_code_lens`](LspManager::resolve_code_lens) fills it, if the
    /// server leaves it.
    pub command: Option<lsp_types::Command>,
    /// The server's lens, to resolve, and the server, session and document
    /// it came from: only that session can resolve it.
    raw: Arc<lsp_types::CodeLens>,
    server: ServerId,
    from: (BufferId, (SessionId, IncarnationId)),
    /// What the resolved range is decoded against.
    decoded: Decoded,
    /// The request's admitted context: the document at the version asked
    /// about and the server's other open documents as they were then.
    context: Context,
}

impl CodeLens {
    /// What the server had of the document and of its other open
    /// documents when this lens was asked for, to decode the locations in
    /// its command against ([`LspNavigation::decode_references`]
    /// (crate::LspNavigation::decode_references)); it doesn't change as the
    /// buffers do.
    pub fn context(&self) -> &Admitted {
        &self.context.0
    }
}

/// A request's [`Admitted`], shared by the lenses it answered.
#[derive(Clone, Debug)]
struct Context(Arc<Admitted>);

impl PartialEq for Context {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone)]
struct Decoded {
    text: fe_text::Text,
    encoding: crate::encoding::PositionEncoding,
}

impl Decoded {
    fn range(&self, range: lsp_types::Range) -> Range<usize> {
        let start = from_lsp(&self.text, range.start, self.encoding);
        let end = from_lsp(&self.text, range.end, self.encoding);
        start.min(end)..end.max(start)
    }
}

impl std::fmt::Debug for Decoded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Decoded")
            .field("len", &self.text.len())
            .field("encoding", &self.encoding)
            .finish()
    }
}

impl PartialEq for Decoded {
    fn eq(&self, other: &Self) -> bool {
        self.encoding == other.encoding && self.text.len() == other.text.len()
    }
}

/// What `Unavailable` says when the server doesn't provide lenses.
const FEATURE: &str = "codeLens";

impl LspManager {
    /// The lenses for open buffer `buffer` at its `version`, merged across
    /// its servers with `codeLensProvider` (see the [module](self)): each
    /// sent only if its server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if no server provides lenses: nothing is sent.
    /// `Ok(Some(vec![]))` if they have none, or don't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels every request still out. It doesn't borrow the
    /// manager.
    pub fn code_lenses(
        &self,
        buffer: BufferId,
        version: Version,
    ) -> impl Future<Output = Result<Option<Vec<CodeLens>>, LspError>> + Send + 'static + use<>
    {
        let asks = self.ask_each::<CodeLensRequest, _>(
            buffer,
            version,
            Gate::Advertised,
            FEATURE,
            provides_lenses,
            |at| CodeLensParams {
                text_document: TextDocumentIdentifier {
                    uri: at.document.uri.clone(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
            |server, reply, admitted| {
                let admitted = Arc::new(admitted);
                (reply.result.unwrap_or_default().into_iter())
                    .map(|lens| convert(lens, &admitted, server))
                    .collect::<Vec<_>>()
            },
        );
        merged(asks).map(|answer| match answer {
            Ok(Some(parts)) => {
                // Stable: server order among lenses at one range.
                let mut lenses: Vec<_> = parts.into_iter().flatten().collect();
                lenses.sort_by_key(|lens| (lens.range.start, lens.range.end));
                Ok(Some(lenses))
            }
            Ok(None) => Ok(None),
            Err(LspError::Timeout) => Ok(Some(Vec::new())),
            Err(err) => Err(err),
        })
    }

    /// `lens` with the `command` its server resolves.
    ///
    /// The lens as it is if its server can't resolve, or doesn't answer
    /// within `lsp.timeouts.resolve` (which also cancels the request).
    pub fn resolve_code_lens(
        &self,
        lens: CodeLens,
    ) -> impl Future<Output = Result<CodeLens, LspError>> + Send + 'static + use<> {
        // A lens that has its command is as resolved as it gets.
        let resolves = lens.command.is_none()
            && self.capabilities(lens.server).is_some_and(|capabilities| {
                (capabilities.code_lens_provider)
                    .is_some_and(|provider| provider.resolve_provider == Some(true))
            });
        // Sent only while the session that gave the lens has its document
        // open as it was then: the same version, as edits drained by the
        // server since the lens was queued count.
        let (buffer, identity) = lens.from;
        let version = lens.context.0.document.sent.version;
        let reply = resolves.then(|| {
            self.request_if::<CodeLensResolve>(lens.server, (*lens.raw).clone(), move |opened| {
                (opened.documents.iter()).any(|document| {
                    document.buffer == buffer
                        && document.identity == identity
                        && document.sent.version == version
                })
            })
            .boxed()
        });
        async move {
            let Some(reply) = reply else {
                return Ok(lens);
            };
            match reply.await {
                Ok(None) | Err(LspError::Timeout) => Ok(lens),
                Ok(Some(reply)) => {
                    let resolved = reply.result;
                    let mut lens = lens;
                    lens.range = lens.decoded.range(resolved.range);
                    lens.command = resolved.command.clone();
                    lens.raw = Arc::new(resolved);
                    Ok(lens)
                }
                Err(err) => Err(err),
            }
        }
    }

    /// Has `lens`'s server run its command (`workspace/executeCommand`).
    /// Any edit it applies in doing so arrives as
    /// [`LspEvent::ApplyEdit`](crate::LspEvent::ApplyEdit).
    ///
    /// Nothing is sent for a lens with no command (`Ok`), nor for one its
    /// server doesn't list in `executeCommandProvider.commands`
    /// (`Unavailable`): a client command is never the server's.
    pub fn run_lens_command(
        &self,
        lens: &CodeLens,
    ) -> impl Future<Output = Result<(), LspError>> + Send + 'static + use<> {
        let Some(command) = lens.command.as_ref() else {
            return async { Ok(()) }.boxed();
        };
        let listed = self.capabilities(lens.server).is_some_and(|capabilities| {
            (capabilities.execute_command_provider.as_ref())
                .is_some_and(|provider| provider.commands.contains(&command.command))
        });
        if !listed {
            let err = LspError::Unavailable(
                tr!(
                    "lsp-not-a-server-command",
                    command = command.command.as_str()
                )
                .to_string()
                .into(),
            );
            return async move { Err(err) }.boxed();
        }
        self.execute_command(lens.server, command, lens.from)
            .boxed()
    }
}

/// A reply's lens, in bytes of the version asked about.
fn convert(lens: lsp_types::CodeLens, at: &Arc<Admitted>, server: ServerId) -> CodeLens {
    let decoded = Decoded {
        text: at.document.sent.text.clone(),
        encoding: at.encoding,
    };
    CodeLens {
        range: decoded.range(lens.range),
        command: lens.command.clone(),
        raw: Arc::new(lens),
        server,
        from: (at.document.buffer, at.document.identity),
        decoded,
        context: Context(at.clone()),
    }
}

/// Whether the server provides code lenses.
fn provides_lenses(capabilities: &ServerCapabilities) -> bool {
    capabilities.code_lens_provider.is_some()
}
