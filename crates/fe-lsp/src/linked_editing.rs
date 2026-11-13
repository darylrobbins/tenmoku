//! `textDocument/linkedEditingRange`: the ranges that are renamed together
//! with the one at an offset, such as an HTML tag's open and close names.
//!
//! - **Provider.** Only the document's provider is asked
//!   ([`Feature::LinkedEditing`](crate::Feature::LinkedEditing)): its lead
//!   if that has `linkedEditingRangeProvider`, else the first add-on that
//!   does, as linked ranges must come from one server. With none, nothing
//!   is sent: `Ok(None)`.
//! - **Ranges.** Decoded against the text the server had at the request's
//!   version, as bytes of that version, sorted by start, duplicates
//!   dropped.
//! - **Failures.** A `null` reply, or a timeout (3 s by default,
//!   `lsp.timeouts.hover`), is `Ok(Some(LinkedEditing::default()))`: no
//!   ranges are linked.

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use futures::FutureExt;
use lsp_types::request::LinkedEditingRange;
use lsp_types::{
    LinkedEditingRangeParams, LinkedEditingRangeServerCapabilities, ServerCapabilities,
};

use crate::event::LspError;
use crate::manager::LspManager;

/// The ranges linked to the one at an offset.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LinkedEditing {
    /// Where, in bytes of the version the request was made on.
    pub ranges: Vec<Range<usize>>,
    /// What the server says the ranges' text may be, as a regular
    /// expression.
    pub word_pattern: Option<String>,
}

/// What `Unavailable` says when the server doesn't provide linked editing.
const FEATURE: &str = "linkedEditingRange";

impl LspManager {
    /// The ranges linked to byte `offset` of open buffer `buffer`, at its
    /// `version`: sent only if the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if the server doesn't provide linked editing: nothing is
    /// sent. No ranges if it has none, or doesn't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels it too. It doesn't borrow the manager, so it may
    /// leave the manager's lock.
    pub fn linked_editing_ranges(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> impl Future<Output = Result<Option<LinkedEditing>, LspError>> + Send + 'static + use<>
    {
        let reply = self.provider_request::<LinkedEditingRange>(
            buffer,
            version,
            FEATURE,
            provided,
            move |at| LinkedEditingRangeParams {
                text_document_position_params: at.position(offset),
                work_done_progress_params: Default::default(),
            },
        );
        // Boxed, as its type names the manager's borrow.
        reply
            .boxed()
            .map(|answer| match answer {
                Ok((reply, admitted, _)) => Ok(Some(match reply.result {
                    None => LinkedEditing::default(),
                    Some(found) => {
                        let mut ranges: Vec<_> = (found.ranges.into_iter())
                            .map(|range| admitted.offsets(range))
                            .collect();
                        ranges.sort_by_key(|r| (r.start, r.end));
                        ranges.dedup();
                        LinkedEditing {
                            ranges,
                            word_pattern: found.word_pattern,
                        }
                    }
                })),
                Err(LspError::Timeout) => Ok(Some(LinkedEditing::default())),
                Err(LspError::Unavailable(why)) if why == FEATURE => Ok(None),
                Err(err) => Err(err),
            })
            .boxed()
    }
}

/// Whether the server provides linked editing ranges.
pub(crate) fn provided(capabilities: &ServerCapabilities) -> bool {
    !matches!(
        capabilities.linked_editing_range_provider,
        None | Some(LinkedEditingRangeServerCapabilities::Simple(false))
    )
}

#[cfg(test)]
#[path = "linked_editing_tests.rs"]
mod tests;
