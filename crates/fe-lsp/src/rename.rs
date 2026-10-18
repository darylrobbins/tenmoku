//! `textDocument/prepareRename` and `textDocument/rename`: renaming a
//! symbol across the workspace, answered as a workspace edit
//! ([`crate::workspace_edit`]).
//!
//! - **Target.** [`prepare_rename`](LspManager::prepare_rename) asks what
//!   a rename at an offset would change, as a [`RenameTarget`] in bytes of
//!   the version asked about. A bare range is its own placeholder, a
//!   `defaultBehavior` reply takes the word at the offset, and a `null`
//!   reply is nothing to rename there. A server's error stays an error,
//!   for a UI to say "cannot rename this". A server that doesn't prepare
//!   gets the word at the offset, with no request.
//! - **Edits.** [`rename`](LspManager::rename) answers the decoded
//!   workspace edit, as [`Admitted::decode_workspace_edit`] does.
//! - **No rename.** A server without `renameProvider` is
//!   `Unavailable("rename")` to both, with no request.
//! - **Timeout.** [`RENAME_TIMEOUT`], for both: a rename walks the
//!   project.

use std::ops::Range;
use std::time::Duration;

use fe_buffer::{BufferId, Version};
use fe_text::{Bias, Text};
use futures::FutureExt;
use futures::future::BoxFuture;
use lsp_types::request::{PrepareRenameRequest, Rename};
use lsp_types::{OneOf, PrepareRenameResponse, RenameParams, ServerCapabilities};

use crate::document::Admitted;
use crate::encoding::from_lsp;
use crate::event::LspError;
use crate::manager::LspManager;
use crate::requests::{call, within};
use crate::workspace_edit::FileEdits;

/// How long a rename may take: more than a hover, as it walks the
/// project. Not in `lsp.timeouts`.
pub const RENAME_TIMEOUT: Duration = Duration::from_secs(10);

/// What a rename at an offset would rename.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct RenameTarget {
    /// The symbol, in bytes of the version the request was made on.
    pub range: Range<usize>,
    /// The name it has now, for the UI to start the new one from.
    pub placeholder: String,
}

impl RenameTarget {
    pub fn new(range: Range<usize>, placeholder: impl Into<String>) -> Self {
        Self {
            range,
            placeholder: placeholder.into(),
        }
    }
}

impl LspManager {
    /// What a rename at byte `offset` of open buffer `buffer`, at its
    /// `version`, would rename: `Ok(None)` if there's nothing to rename
    /// there. Sent only if the server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// `Err(LspError::Server { message, .. })` if the server refuses, and
    /// `Unavailable("rename")` if it can't rename at all. A server that
    /// can't prepare gets no request: the word at `offset` is the target.
    /// Dropping the future cancels the request.
    pub fn prepare_rename(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> impl Future<Output = Result<Option<RenameTarget>, LspError>> + Send + 'static + use<> {
        // Decided once the server's capabilities are known.
        self.admit_once_known(
            buffer,
            version,
            Some(RENAME_TIMEOUT),
            move |capabilities, answer| {
                if !renames(capabilities) {
                    return Err(LspError::Unavailable("rename".into()));
                }
                if !prepares(capabilities) {
                    // No request: the version is checked, and the word read
                    // from the text the server has at it.
                    return Ok(Box::new(move |admission| {
                        let _ = answer.send(admission.map(|at| default_target(&at, offset)));
                        Vec::new()
                    }));
                }
                Ok(Box::new(move |admission| {
                    let admitted = match admission {
                        Ok(admitted) => admitted,
                        Err(err) => {
                            let _ = answer.send(Err(err));
                            return Vec::new();
                        }
                    };
                    let params = admitted.position(offset);
                    vec![call::<PrepareRenameRequest, _>(
                        answer,
                        params,
                        move |_, reply| {
                            reply.map(|reply| {
                                reply
                                    .result
                                    .and_then(|reply| target(reply, &admitted, offset))
                            })
                        },
                    )]
                }))
            },
        )
        .boxed()
    }

    /// The edits renaming the symbol at byte `offset` of open buffer
    /// `buffer`, at its `version`, to `new_name`, decoded against what the
    /// server had open when asked. Sent as
    /// [`prepare_rename`](Self::prepare_rename) is.
    ///
    /// `Err(LspError::RefusedEdit)` if the edit can't be applied as the
    /// server meant, and `Unavailable("rename")` if it can't rename.
    pub fn rename(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        new_name: String,
    ) -> impl Future<Output = Result<Vec<FileEdits>, LspError>> + Send + 'static + use<> {
        let reply = self.document_request_if_provided::<Rename>(
            buffer,
            version,
            "rename",
            renames,
            move |at| RenameParams {
                text_document_position: at.position(offset),
                new_name,
                work_done_progress_params: Default::default(),
            },
        );
        let reply: BoxFuture<'static, _> = reply
            .map(|answer| {
                let (reply, at) = answer?;
                match reply.result {
                    Some(edit) => at.decode_workspace_edit(edit),
                    None => Ok(Vec::new()),
                }
            })
            .boxed();
        within(
            reply,
            Some(RENAME_TIMEOUT),
            &*self.clock,
            Err(LspError::Timeout),
        )
        .boxed()
    }
}

/// Whether the server renames at all.
fn renames(capabilities: &ServerCapabilities) -> bool {
    match &capabilities.rename_provider {
        None | Some(OneOf::Left(false)) => false,
        Some(OneOf::Left(true) | OneOf::Right(_)) => true,
    }
}

/// Whether the server prepares renames.
fn prepares(capabilities: &ServerCapabilities) -> bool {
    matches!(
        &capabilities.rename_provider,
        Some(OneOf::Right(options)) if options.prepare_provider == Some(true)
    )
}

/// A reply as a target, in bytes of the text the server had.
fn target(reply: PrepareRenameResponse, at: &Admitted, offset: usize) -> Option<RenameTarget> {
    let text = &at.document.sent.text;
    let bytes = |range: lsp_types::Range| {
        let start = from_lsp(text, range.start, at.encoding);
        let end = from_lsp(text, range.end, at.encoding);
        start.min(end)..end.max(start)
    };
    match reply {
        PrepareRenameResponse::Range(range) => {
            let range = bytes(range);
            Some(RenameTarget {
                placeholder: text.slice_to_string(range.clone()),
                range,
            })
        }
        PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } => Some(RenameTarget {
            range: bytes(range),
            placeholder,
        }),
        PrepareRenameResponse::DefaultBehavior { .. } => default_target(at, offset),
    }
}

/// The word at `offset` of the document the server has.
fn default_target(at: &Admitted, offset: usize) -> Option<RenameTarget> {
    let text = &at.document.sent.text;
    word_at(text, offset).map(|range| RenameTarget {
        placeholder: text.slice_to_string(range.clone()),
        range,
    })
}

/// The run of letters, digits and underscores that `offset` is in or at
/// the end of, if any.
fn word_at(text: &Text, offset: usize) -> Option<Range<usize>> {
    let offset = text.clip_offset(offset, Bias::Left);
    let point = text.offset_to_point(offset);
    let row = text.row_to_string(point.row);
    let column = (point.column as usize).min(row.len());
    let word = |c: char| c.is_alphanumeric() || c == '_';
    let start = row[..column].trim_end_matches(word).len();
    let end = column
        + row[column..]
            .find(|c| !word(c))
            .unwrap_or(row.len() - column);
    let row_start = offset - column;
    (start < end).then(|| row_start + start..row_start + end)
}

#[cfg(test)]
#[path = "rename_tests.rs"]
mod tests;
