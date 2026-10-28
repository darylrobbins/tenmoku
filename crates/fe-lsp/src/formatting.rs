//! `textDocument/formatting` and `textDocument/rangeFormatting`: a
//! document, or a range of it, formatted by its server.
//!
//! - **Edits.** The reply's edits are decoded against the text the server
//!   had at the request's version, as byte edits of that version, sorted
//!   by start. Edits that overlap are an error ([`LspError::RefusedEdit`]).
//! - **No formatter.** `Ok(None)` for a server without the provider, with
//!   nothing sent (a server still initializing is waited for first), so a host can say "no formatter". `Ok(Some(vec![]))`
//!   is a server with nothing to change.
//! - **Failures.** A request past 3 s is [`LspError::Timeout`], which also
//!   cancels it, not an empty result: format on save can tell "nothing to
//!   change" from "gave up".

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use fe_text::Bias;
use futures::FutureExt;
use lsp_types::request::{Formatting, RangeFormatting};
use lsp_types::{
    DocumentFormattingParams, DocumentRangeFormattingParams, FormattingOptions, OneOf,
    TextDocumentIdentifier,
};

use crate::document::Admitted;
use crate::encoding::to_lsp;
use crate::event::LspError;
use crate::manager::LspManager;
use crate::workspace_edit::decode_closed;

/// Byte edits of the version formatted, sorted by start.
pub type Edits = Vec<(Range<usize>, String)>;

/// How to format: the editor's indentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatOptions {
    pub tab_size: u32,
    pub insert_spaces: bool,
}

impl FormatOptions {
    fn lsp(self) -> FormattingOptions {
        FormattingOptions {
            tab_size: self.tab_size,
            insert_spaces: self.insert_spaces,
            ..FormattingOptions::default()
        }
    }
}

impl LspManager {
    /// The edits that format open buffer `buffer`, at its `version`, or
    /// byte `range` of it: sent only if the server has that version, as
    /// any [`document_request`](Self::document_request) is.
    ///
    /// `Ok(None)` if the buffer's server has no formatting provider (or,
    /// for a range, no range formatting provider). `Err(Timeout)` if it
    /// doesn't answer within 3 s. It doesn't borrow the manager.
    pub fn format(
        &self,
        buffer: BufferId,
        version: Version,
        range: Option<Range<usize>>,
        options: FormatOptions,
    ) -> impl Future<Output = Result<Option<Edits>, LspError>> + Send + 'static + use<> {
        // A buffer on no server has no formatter; a server still
        // initializing (or restarting) is waited for, inside the timeout.
        let reply = self.server_of(buffer).map(|_| match range {
            None => self
                .document_request_if_provided::<Formatting>(
                    buffer,
                    version,
                    FEATURE,
                    |capabilities| offers(capabilities.document_formatting_provider.clone()),
                    move |at| DocumentFormattingParams {
                        text_document: document(at),
                        options: options.lsp(),
                        work_done_progress_params: Default::default(),
                    },
                )
                .boxed(),
            Some(range) => self
                .document_request_if_provided::<RangeFormatting>(
                    buffer,
                    version,
                    FEATURE,
                    |capabilities| offers(capabilities.document_range_formatting_provider.clone()),
                    move |at| {
                        let text = &at.document.sent.text;
                        let position = |offset| {
                            to_lsp(text, text.clip_offset(offset, Bias::Left), at.encoding)
                        };
                        DocumentRangeFormattingParams {
                            text_document: document(at),
                            range: lsp_types::Range::new(
                                position(range.start),
                                position(range.end),
                            ),
                            options: options.lsp(),
                            work_done_progress_params: Default::default(),
                        }
                    },
                )
                .boxed(),
        });
        async move {
            let Some(reply) = reply else {
                return Ok(None);
            };
            let (reply, admitted) = match reply.await {
                Ok(answered) => answered,
                Err(LspError::Unavailable(why)) if why == FEATURE => return Ok(None),
                Err(err) => return Err(err),
            };
            let edits = reply.result.unwrap_or_default();
            let text = &admitted.document.sent.text;
            decode_closed(text, &edits, admitted.encoding)
                .map(Some)
                .map_err(LspError::RefusedEdit)
        }
    }
}

/// The `Unavailable` reason for a server without the provider.
const FEATURE: &str = "formatting";

/// Whether a server's provider capability is on.
fn offers<T>(provider: Option<OneOf<bool, T>>) -> bool {
    match provider {
        Some(OneOf::Left(on)) => on,
        Some(OneOf::Right(_)) => true,
        None => false,
    }
}

fn document(at: &Admitted) -> TextDocumentIdentifier {
    TextDocumentIdentifier {
        uri: at.document.uri.clone(),
    }
}

#[cfg(test)]
#[path = "formatting_tests.rs"]
mod tests;
