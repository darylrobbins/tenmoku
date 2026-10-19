//! `textDocument/inlayHint` and `inlayHint/resolve`: the hints a server
//! has for a range of a buffer, and a hint's tooltip and edits.
//!
//! - **Provider.** A server without `inlayHintProvider` gets no request:
//!   `Ok(None)`.
//! - **Positions.** A hint's position and its edits are decoded against
//!   the text the server had at the request's version, as bytes of that
//!   version. A label made of parts is the parts' values joined. Hints
//!   outside the asked range are dropped, and the rest sorted by offset.
//! - **Resolve.** `tooltip` and `textEdits` are resolved
//!   (`resolveSupport`), only from a server with a `resolveProvider`, and
//!   only by the session that gave the hint, while it has that opening of
//!   the document. The edits are decoded against the text of the request's
//!   version.
//! - **Failures.** A request past `lsp.timeouts.hover` (3 s) is
//!   `Ok(Some([]))`, a resolve past `lsp.timeouts.resolve` (2 s) is the
//!   hint as it was. Both cancel the request.

use std::ops::Range;
use std::sync::Arc;

use fe_buffer::{BufferId, Version};
use fe_text::{Bias, Text};
use futures::FutureExt;
use lsp_types::request::{InlayHintRequest, InlayHintResolveRequest};
use lsp_types::{
    InlayHintKind as LspKind, InlayHintLabel, InlayHintParams, InlayHintServerCapabilities,
    InlayHintTooltip, MarkupContent, OneOf, Position, ServerCapabilities, TextDocumentIdentifier,
};

use crate::document::Admitted;
use crate::encoding::{PositionEncoding, from_lsp};
use crate::event::{LspError, ServerId};
use crate::identity::{IncarnationId, SessionId};
use crate::manager::LspManager;

/// What a hint stands for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlayHintKind {
    /// The type of the text before it.
    Type,
    /// The name of the parameter of the argument after it.
    Parameter,
}

/// One hint a server has for a range.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct InlayHint {
    /// Where it shows, in bytes of the version the request was made on.
    pub offset: usize,
    /// What it shows.
    pub label: String,
    pub kind: Option<InlayHintKind>,
    pub padding_left: bool,
    pub padding_right: bool,
    /// Markdown or plain text; filled by
    /// [`resolve_inlay_hint`](LspManager::resolve_inlay_hint) if the server
    /// leaves it.
    pub tooltip: Option<String>,
    /// What accepting it edits, in bytes of the version the request was
    /// made on.
    pub edits: Vec<(Range<usize>, String)>,
    /// The server's hint, to resolve, and the server, session and
    /// document it came from: only that session can resolve it.
    raw: Raw,
    server: ServerId,
    from: (BufferId, (SessionId, IncarnationId)),
    /// What resolved edits are decoded against.
    decoded: Decoded,
}

/// The server's hint, compared as the JSON it was sent as.
#[derive(Clone, Debug)]
struct Raw(Arc<lsp_types::InlayHint>);

impl PartialEq for Raw {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_value(&*self.0).ok() == serde_json::to_value(&*other.0).ok()
    }
}

/// The text of the request's version and the session's encoding.
#[derive(Clone)]
struct Decoded {
    text: Text,
    encoding: PositionEncoding,
}

impl Decoded {
    fn offset(&self, position: Position) -> usize {
        from_lsp(&self.text, position, self.encoding)
    }

    fn edits(&self, edits: &[lsp_types::TextEdit]) -> Vec<(Range<usize>, String)> {
        (edits.iter())
            .map(|edit| {
                let start = self.offset(edit.range.start);
                let end = self.offset(edit.range.end);
                (start.min(end)..end.max(start), edit.new_text.clone())
            })
            .collect()
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

/// What `Unavailable` says when the server doesn't provide hints.
const FEATURE: &str = "inlayHint";

impl LspManager {
    /// The hints for bytes `range` of open buffer `buffer`, at its
    /// `version`: sent only if the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if the server doesn't provide hints: nothing is sent.
    /// `Ok(Some(vec![]))` if it has none, or doesn't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels it too. It doesn't borrow the manager.
    pub fn inlay_hints(
        &self,
        buffer: BufferId,
        version: Version,
        range: Range<usize>,
    ) -> impl Future<Output = Result<Option<Vec<InlayHint>>, LspError>> + Send + 'static + use<>
    {
        let server = self.server_of(buffer);
        let asked = range.clone();
        let reply = self.document_request_if_provided::<InlayHintRequest>(
            buffer,
            version,
            FEATURE,
            provides_hints,
            move |at| InlayHintParams {
                work_done_progress_params: Default::default(),
                text_document: TextDocumentIdentifier {
                    uri: at.document.uri.clone(),
                },
                range: lsp_types::Range::new(
                    at.position(range.start).position,
                    at.position(range.end).position,
                ),
            },
        );
        // Boxed, as its type names the manager's borrow.
        reply
            .boxed()
            .map(move |answer| match (answer, server) {
                (Ok((reply, admitted)), Some(server)) => {
                    let mut hints: Vec<_> = (reply.result.unwrap_or_default().into_iter())
                        .map(|hint| convert(hint, &admitted, server))
                        .filter(|hint| asked.contains(&hint.offset) || hint.offset == asked.end)
                        .collect();
                    hints.sort_by_key(|hint| hint.offset);
                    Ok(Some(hints))
                }
                (Ok(_), None) | (Err(LspError::Timeout), _) => Ok(Some(Vec::new())),
                (Err(LspError::Unavailable(why)), _) if why == FEATURE => Ok(None),
                (Err(err), _) => Err(err),
            })
            .boxed()
    }

    /// `hint` with the `tooltip` and `edits` its server resolves.
    ///
    /// The hint as it is if its server can't resolve, or doesn't answer
    /// within `lsp.timeouts.resolve` (which also cancels the request).
    pub fn resolve_inlay_hint(
        &self,
        hint: InlayHint,
    ) -> impl Future<Output = Result<InlayHint, LspError>> + Send + 'static + use<> {
        let resolves = self
            .capabilities(hint.server)
            .is_some_and(|capabilities| resolves_hints(&capabilities));
        // Sent only while the session that gave the hint has its document
        // open as it was then.
        let (buffer, identity) = hint.from;
        let reply = resolves.then(|| {
            self.request_if::<InlayHintResolveRequest>(
                hint.server,
                (*hint.raw.0).clone(),
                move |opened| {
                    (opened.documents.iter())
                        .any(|document| document.buffer == buffer && document.identity == identity)
                },
            )
            .boxed()
        });
        async move {
            let Some(reply) = reply else {
                return Ok(hint);
            };
            match reply.await {
                Ok(None) | Err(LspError::Timeout) => Ok(hint),
                Ok(Some(reply)) => {
                    let resolved = reply.result;
                    let mut hint = hint;
                    if let Some(tooltip) = resolved.tooltip.clone() {
                        hint.tooltip = Some(tooltip_text(tooltip));
                    }
                    if let Some(edits) = &resolved.text_edits {
                        hint.edits = hint.decoded.edits(edits);
                    }
                    hint.raw = Raw(Arc::new(resolved));
                    Ok(hint)
                }
                Err(err) => Err(err),
            }
        }
    }
}

/// A reply's hint, in bytes of the version asked about.
fn convert(hint: lsp_types::InlayHint, at: &Admitted, server: ServerId) -> InlayHint {
    let text = &at.document.sent.text;
    let decoded = Decoded {
        text: text.clone(),
        encoding: at.encoding,
    };
    let offset = text.clip_offset(decoded.offset(hint.position), Bias::Left);
    let label = match &hint.label {
        InlayHintLabel::String(label) => label.clone(),
        InlayHintLabel::LabelParts(parts) => parts.iter().map(|part| part.value.as_str()).collect(),
    };
    InlayHint {
        offset,
        label,
        kind: match hint.kind {
            Some(LspKind::TYPE) => Some(InlayHintKind::Type),
            Some(LspKind::PARAMETER) => Some(InlayHintKind::Parameter),
            _ => None,
        },
        padding_left: hint.padding_left == Some(true),
        padding_right: hint.padding_right == Some(true),
        tooltip: hint.tooltip.clone().map(tooltip_text),
        edits: decoded.edits(hint.text_edits.as_deref().unwrap_or(&[])),
        raw: Raw(Arc::new(hint)),
        server,
        from: (at.document.buffer, at.document.identity),
        decoded,
    }
}

fn tooltip_text(tooltip: InlayHintTooltip) -> String {
    match tooltip {
        InlayHintTooltip::String(text)
        | InlayHintTooltip::MarkupContent(MarkupContent { value: text, .. }) => text,
    }
}

/// Whether the server provides inlay hints.
fn provides_hints(capabilities: &ServerCapabilities) -> bool {
    !matches!(
        capabilities.inlay_hint_provider,
        None | Some(OneOf::Left(false))
    )
}

/// Whether the server provides inlay hints and resolves them.
fn resolves_hints(capabilities: &ServerCapabilities) -> bool {
    match &capabilities.inlay_hint_provider {
        Some(OneOf::Right(InlayHintServerCapabilities::Options(options))) => {
            options.resolve_provider == Some(true)
        }
        Some(OneOf::Right(InlayHintServerCapabilities::RegistrationOptions(options))) => {
            options.inlay_hint_options.resolve_provider == Some(true)
        }
        _ => false,
    }
}
