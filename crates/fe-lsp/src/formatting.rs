//! `textDocument/formatting` and `textDocument/rangeFormatting`: a
//! document, or a range of it, formatted by its server; and
//! `textDocument/onTypeFormatting`, the edits a character just typed asks
//! for.
//!
//! - **Provider.** Only the document's provider of each is asked
//!   ([`Feature`]): its lead if that has the provider, else the first
//!   add-on that does, as two edit sets can't be applied together.
//! - **On type.** Sent only for one of the buffer's trigger characters
//!   ([`LspManager::on_type_formatting_triggers`]), its provider's alone:
//!   the server's static `documentOnTypeFormattingProvider`, or a dynamic
//!   registration for the document. Any other character, or a buffer whose
//!   servers' capabilities aren't known yet, is `Ok(None)` with nothing
//!   sent: typing never waits for a server to start.
//!
//! - **Edits.** The reply's edits are decoded against the text the server
//!   had at the request's version, as byte edits of that version, sorted
//!   by start. Edits that overlap are an error ([`LspError::RefusedEdit`]).
//! - **No formatter.** `Ok(None)` when no server of the document has the
//!   provider, with nothing sent (a server still initializing is waited
//!   for first), so a host can say "no formatter". `Ok(Some(vec![]))` is a
//!   server with nothing to change.
//! - **Failures.** A request past 3 s is [`LspError::Timeout`], which also
//!   cancels it, not an empty result: format on save can tell "nothing to
//!   change" from "gave up".

use std::ops::Range;

use fe_buffer::{BufferId, Version};
use fe_text::Bias;
use futures::FutureExt;
use lsp_types::request::{Formatting, OnTypeFormatting, RangeFormatting};
use lsp_types::{
    DocumentFormattingParams, DocumentOnTypeFormattingParams, DocumentRangeFormattingParams,
    FormattingOptions, OneOf, TextDocumentIdentifier,
};

use crate::document::Admitted;
use crate::encoding::to_lsp;
use crate::event::LspError;
use crate::manager::{Feature, LspManager};
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
    /// Sent to the buffer's provider of [`Feature::Formatting`] (for a
    /// range, [`Feature::RangeFormatting`]). `Ok(None)` if none of its
    /// servers has the provider. `Err(Timeout)` if the provider doesn't
    /// answer within 3 s. It doesn't borrow the manager.
    pub fn format(
        &self,
        buffer: BufferId,
        version: Version,
        range: Option<Range<usize>>,
        options: FormatOptions,
    ) -> impl Future<Output = Result<Option<Edits>, LspError>> + Send + 'static + use<> {
        // A buffer on no server has no formatter; a server still
        // initializing (or restarting) is waited for, inside the timeout.
        let reply = self.is_served(buffer).then(|| match range {
            None => self
                .provider_request::<Formatting>(
                    buffer,
                    version,
                    FEATURE,
                    |capabilities| Feature::Formatting.advertised(capabilities),
                    move |at| DocumentFormattingParams {
                        text_document: document(at),
                        options: options.lsp(),
                        work_done_progress_params: Default::default(),
                    },
                )
                .boxed(),
            Some(range) => self
                .provider_request::<RangeFormatting>(
                    buffer,
                    version,
                    FEATURE,
                    |capabilities| Feature::RangeFormatting.advertised(capabilities),
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
            let (reply, admitted, _) = match reply.await {
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

impl LspManager {
    /// The edits the server makes after `ch` was typed, leaving the caret
    /// at byte `offset` of open buffer `buffer` at its `version`: sent only
    /// to the buffer's provider of [`Feature::OnTypeFormatting`], and only if
    /// `ch` is one of its on-type formatting triggers (see the
    /// [module](self)) and it has that version.
    ///
    /// `Ok(None)` if `ch` isn't a trigger there. `Err(Timeout)` if the
    /// server doesn't answer within 3 s. It doesn't borrow the manager.
    pub fn on_type_formatting(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        ch: char,
        options: FormatOptions,
    ) -> impl Future<Output = Result<Option<Edits>, LspError>> + Send + 'static + use<> {
        let provider = self.provider_of(buffer, Feature::OnTypeFormatting);
        let triggered =
            provider.filter(|&provider| (self.on_type_triggers_on(provider, buffer)).contains(&ch));
        let reply = triggered.map(|provider| {
            // The triggers were read from the capabilities and registrations
            // known now: the request goes whichever provided them.
            self.document_request_if_provided_on::<OnTypeFormatting>(
                provider,
                buffer,
                version,
                FEATURE,
                |_| true,
                move |at| DocumentOnTypeFormattingParams {
                    text_document_position: at.position(offset),
                    ch: ch.to_string(),
                    options: options.lsp(),
                },
            )
            .boxed()
        });
        async move {
            let Some(reply) = reply else {
                return Ok(None);
            };
            let (reply, admitted) = reply.await?;
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
pub(crate) fn offers<T>(provider: Option<OneOf<bool, T>>) -> bool {
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

#[cfg(test)]
#[path = "on_type_formatting_tests.rs"]
mod on_type_formatting_tests;
