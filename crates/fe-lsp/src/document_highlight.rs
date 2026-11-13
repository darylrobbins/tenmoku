//! `textDocument/documentHighlight`: the other uses of the symbol at an
//! offset, read and write uses told apart.
//!
//! - **Servers.** First-that-answers (§5): every server of the document
//!   with `documentHighlightProvider` is asked at once, and the first
//!   non-empty answer in server order (the lead first) is taken, as two
//!   servers' sets would contradict each other. With none providing,
//!   nothing is sent: `Ok(None)`, and the caller falls back to the word's
//!   own occurrences.
//! - **Ranges.** Decoded against the text the server had at the request's
//!   version, as bytes of that version, sorted by start, duplicates
//!   dropped. A highlight with no kind is [`HighlightKind::Text`].
//! - **Failures.** A `null` or empty reply, or a timeout (3 s by default,
//!   `lsp.timeouts.hover`), is `Ok(Some([]))`: the server has nothing to
//!   highlight, so there is no fallback. One server's error is the answer
//!   only if no other answers.

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use futures::FutureExt;
use lsp_types::request::DocumentHighlightRequest;
use lsp_types::{DocumentHighlightKind, DocumentHighlightParams, OneOf, ServerCapabilities};

use crate::event::LspError;
use crate::manager::LspManager;
use crate::requests::{first_that_answers, rank_list};

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
    /// `version`: the first non-empty answer in server order of its servers
    /// that provide them (see the [module](self)), each sent only if the
    /// server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if no server provides highlights: nothing is sent.
    /// `Ok(Some(vec![]))` if they have none, or don't say within
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
        // Sent to each server once its capabilities are known, and only to
        // one that provides highlights.
        let asks = self.ask_all(buffer, |server| {
            self.document_request_if_provided_on::<DocumentHighlightRequest>(
                server,
                buffer,
                version,
                FEATURE,
                highlights,
                move |at| DocumentHighlightParams {
                    text_document_position_params: at.position(offset),
                    work_done_progress_params: Default::default(),
                    partial_result_params: Default::default(),
                },
            )
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
        });
        first_that_answers(asks.into_iter().map(|(_, ask)| ask).collect(), rank_list)
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
