//! `textDocument/foldingRange`: the ranges the server says can fold.
//!
//! - **Provider.** Only the document's provider is asked
//!   ([`Feature::Folding`](crate::Feature::Folding)): its lead if that has
//!   `foldingRangeProvider`, else the first add-on that does, as two
//!   servers' fold sets would make nonsense folds. With none, nothing is
//!   sent: `Ok(None)`, and the caller keeps its own fold model.
//! - **Ranges.** Whole lines only (the client declares `lineFoldingOnly`),
//!   so a range is its start and end rows, which need no position
//!   encoding. A range that ends on or before its start is dropped, as is
//!   one the server sends with no end after it.
//! - **Failures.** A `null` reply or a timeout (3 s by default,
//!   `lsp.timeouts.hover`) is `Ok(Some([]))`: the server has no folds.

use fe_buffer::{BufferId, Version};
use futures::FutureExt;
use lsp_types::request::FoldingRangeRequest;
use lsp_types::{
    FoldingRangeKind, FoldingRangeParams, FoldingRangeProviderCapability, ServerCapabilities,
    TextDocumentIdentifier,
};

use crate::event::LspError;
use crate::manager::LspManager;

/// What a folding range covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FoldKind {
    Comment,
    Imports,
    Region,
}

/// A run of lines the server says can fold: `start_row` stays visible,
/// rows after it up to `end_row` hide.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct FoldingRange {
    pub start_row: u32,
    pub end_row: u32,
    pub kind: Option<FoldKind>,
}

impl FoldingRange {
    pub fn new(start_row: u32, end_row: u32, kind: Option<FoldKind>) -> Self {
        Self {
            start_row,
            end_row,
            kind,
        }
    }
}

/// What `Unavailable` says when the server doesn't provide folding ranges.
const FEATURE: &str = "foldingRange";

impl LspManager {
    /// The folding ranges of open buffer `buffer`, at its `version`: sent
    /// only if the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if the server doesn't provide folding ranges: nothing is
    /// sent. `Ok(Some(vec![]))` if it has none, or doesn't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels it too. It doesn't borrow the manager, so it may
    /// leave the manager's lock. Ranges are in the server's order.
    pub fn folding_ranges(
        &self,
        buffer: BufferId,
        version: Version,
    ) -> impl Future<Output = Result<Option<Vec<FoldingRange>>, LspError>> + Send + 'static + use<>
    {
        let reply = self.provider_request::<FoldingRangeRequest>(
            buffer,
            version,
            FEATURE,
            provides_folding,
            |at| FoldingRangeParams {
                text_document: TextDocumentIdentifier {
                    uri: at.document.uri.clone(),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            },
        );
        // Boxed, as its type names the manager's borrow.
        reply
            .boxed()
            .map(|answer| match answer {
                Ok((reply, _, _)) => {
                    let found = (reply.result.unwrap_or_default().into_iter())
                        .filter(|range| range.end_line > range.start_line)
                        .map(|range| FoldingRange {
                            start_row: range.start_line,
                            end_row: range.end_line,
                            kind: match range.kind {
                                Some(FoldingRangeKind::Comment) => Some(FoldKind::Comment),
                                Some(FoldingRangeKind::Imports) => Some(FoldKind::Imports),
                                Some(FoldingRangeKind::Region) => Some(FoldKind::Region),
                                None => None,
                            },
                        })
                        .collect();
                    Ok(Some(found))
                }
                Err(LspError::Timeout) => Ok(Some(Vec::new())),
                Err(LspError::Unavailable(why)) if why == FEATURE => Ok(None),
                Err(err) => Err(err),
            })
            .boxed()
    }
}

/// Whether the server provides folding ranges.
pub(crate) fn provides_folding(capabilities: &ServerCapabilities) -> bool {
    !matches!(
        capabilities.folding_range_provider,
        None | Some(FoldingRangeProviderCapability::Simple(false))
    )
}

#[cfg(test)]
#[path = "folding_range_tests.rs"]
mod tests;
