//! `textDocument/documentHighlight`: the other uses of the symbol at an
//! offset, read and write uses told apart.
//!
//! - **Provider.** A server without `documentHighlightProvider` gets no
//!   request: `Ok(None)`, and the caller falls back to the word's own
//!   occurrences.
//! - **Ranges.** Decoded against the text the server had at the request's
//!   version, as bytes of that version, sorted by start, duplicates
//!   dropped. A highlight with no kind is [`HighlightKind::Text`].
//! - **Failures.** A `null` or empty reply, or a timeout (3 s by default,
//!   `lsp.timeouts.hover`), is `Ok(Some([]))`: the server has nothing to
//!   highlight, so there is no fallback.

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use futures::FutureExt;
use lsp_types::request::DocumentHighlightRequest;
use lsp_types::{DocumentHighlightKind, DocumentHighlightParams, OneOf, ServerCapabilities};

use crate::event::LspError;
use crate::manager::LspManager;

/// How a highlighted range is used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum HighlightKind {
    /// A textual occurrence.
    Text,
    /// A read of the symbol.
    Read,
    /// A write to the symbol.
    Write,
}

/// One use of the symbol at an offset.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocumentHighlight {
    /// Where, in bytes of the version the request was made on.
    pub range: Range<usize>,
    pub kind: HighlightKind,
}

impl DocumentHighlight {
    pub fn new(range: Range<usize>, kind: HighlightKind) -> Self {
        Self { range, kind }
    }
}

/// What `Unavailable` says when the server doesn't provide highlights.
const FEATURE: &str = "documentHighlight";

impl LspManager {
    /// The highlights for byte `offset` of open buffer `buffer`, at its
    /// `version`: sent only if the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if the server doesn't provide highlights: nothing is
    /// sent. `Ok(Some(vec![]))` if it has none, or doesn't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels it too. It doesn't borrow the manager, so it may
    /// leave the manager's lock.
    pub fn document_highlights(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> impl Future<Output = Result<Option<Vec<DocumentHighlight>>, LspError>> + Send + 'static + use<>
    {
        // Sent once the server's capabilities are known, and only to a
        // server that provides highlights.
        let reply = self.document_request_if_provided::<DocumentHighlightRequest>(
            buffer,
            version,
            FEATURE,
            highlights,
            move |at| DocumentHighlightParams {
                text_document_position_params: at.position(offset),
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        );
        // Boxed, as its type names the manager's borrow.
        reply
            .boxed()
            .map(|answer| match answer {
                Ok((reply, admitted)) => {
                    let mut found: Vec<_> = (reply.result.unwrap_or_default().into_iter())
                        .map(|highlight| DocumentHighlight {
                            range: admitted.offsets(highlight.range),
                            kind: kind(highlight.kind),
                        })
                        .collect();
                    found.sort_by_key(|h| (h.range.start, h.range.end, h.kind as u8));
                    found.dedup();
                    Ok(Some(found))
                }
                Err(LspError::Timeout) => Ok(Some(Vec::new())),
                Err(LspError::Unavailable(why)) if why == FEATURE => Ok(None),
                Err(err) => Err(err),
            })
            .boxed()
    }
}

/// Whether the server provides document highlights.
fn highlights(capabilities: &ServerCapabilities) -> bool {
    match &capabilities.document_highlight_provider {
        None | Some(OneOf::Left(false)) => false,
        Some(OneOf::Left(true) | OneOf::Right(_)) => true,
    }
}

fn kind(kind: Option<DocumentHighlightKind>) -> HighlightKind {
    match kind {
        Some(DocumentHighlightKind::READ) => HighlightKind::Read,
        Some(DocumentHighlightKind::WRITE) => HighlightKind::Write,
        _ => HighlightKind::Text,
    }
}
