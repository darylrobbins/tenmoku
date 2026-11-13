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
//! - **Servers.** Preparation is first-that-answers (§5): every server of
//!   the document with `renameProvider` is asked, and the first in server
//!   order to accept owns the rename ([`RenameTarget::owner`]).
//!   [`rename_prepared`](LspManager::rename_prepared) goes to that owner
//!   alone, only while its session has the document open as it did, and
//!   one edit set is never merged with another server's.
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
use lsp_types::{OneOf, PrepareRenameResponse, RenameParams, ServerCapabilities, WorkspaceEdit};

use crate::document::Admitted;
use crate::encoding::from_lsp;
use crate::event::{LspError, ServerId};
use crate::manager::{LspManager, Owner};
use crate::requests::{Rank, call, first_that_answers, within};
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
    /// The server whose preparation accepted the rename, which alone
    /// renames it ([`rename_prepared`](LspManager::rename_prepared)), as
    /// its session had the document open then. None for a target made with
    /// [`new`](Self::new).
    pub owner: Option<Owner>,
}

impl RenameTarget {
    pub fn new(range: Range<usize>, placeholder: impl Into<String>) -> Self {
        Self {
            range,
            placeholder: placeholder.into(),
            owner: None,
        }
    }
}

impl LspManager {
    /// What a rename at byte `offset` of open buffer `buffer`, at its
    /// `version`, would rename: `Ok(None)` if there's nothing to rename
    /// there. Sent only if a server has that version, as any
    /// [`document_request`](Self::document_request) is.
    ///
    /// First-that-answers (§5): every server of the buffer with
    /// `renameProvider` is asked at once, and the first in server order
    /// (the lead first) to accept owns the rename: the target's
    /// [`owner`](RenameTarget::owner). Each earlier server is waited for
    /// first, so which answers first doesn't matter.
    ///
    /// `Err(LspError::Server { message, .. })` if a server refuses and none
    /// accepts, and `Unavailable("rename")` if none can rename at all. A
    /// server that can't prepare gets no request, and accepts: the word at
    /// `offset` is its target. Dropping the future cancels the requests.
    pub fn prepare_rename(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> impl Future<Output = Result<Option<RenameTarget>, LspError>> + Send + 'static + use<> {
        let asks = self.ask_all(buffer, |server| {
            self.prepare_rename_on(server, buffer, version, offset)
        });
        first_that_answers(
            asks.into_iter().map(|(_, ask)| ask).collect(),
            |answer| match answer {
                Ok(Some(_)) => Rank::Taken,
                Ok(None) => Rank::Empty,
                Err(LspError::Unavailable(why)) if why == FEATURE => Rank::Unprovided,
                Err(_) => Rank::Failed,
            },
        )
    }

    /// [`prepare_rename`](Self::prepare_rename), asked of `server` alone.
    fn prepare_rename_on(
        &self,
        server: ServerId,
        buffer: BufferId,
        version: Version,
        offset: usize,
    ) -> BoxFuture<'static, Result<Option<RenameTarget>, LspError>> {
        // Decided once the server's capabilities are known.
        self.admit_on_one(
            vec![server],
            buffer,
            version,
            Some(RENAME_TIMEOUT),
            move |capabilities, answer| {
                if !renames(capabilities) {
                    return Err(LspError::Unavailable(FEATURE.into()));
                }
                if !prepares(capabilities) {
                    // No request: the version is checked, and the word read
                    // from the text the server has at it.
                    return Ok(Box::new(move |admission| {
                        let _ = answer.send(admission.map(|at| {
                            default_target(&at, offset).map(|target| target.owned(server, &at))
                        }));
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
                                (reply.result)
                                    .and_then(|reply| target(reply, &admitted, offset))
                                    .map(|target| target.owned(server, &admitted))
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
    /// server had open when asked. With no prepared target, so sent to the
    /// buffer's first server (the lead first) with `renameProvider`;
    /// [`rename_prepared`](Self::rename_prepared) sends it to the server
    /// that accepted the preparation instead.
    ///
    /// `Err(LspError::RefusedEdit)` if the edit can't be applied as the
    /// server meant, and `Unavailable("rename")` if no server can rename.
    pub fn rename(
        &self,
        buffer: BufferId,
        version: Version,
        offset: usize,
        new_name: String,
    ) -> impl Future<Output = Result<Vec<FileEdits>, LspError>> + Send + 'static + use<> {
        let reply = self
            .provider_request::<Rename>(buffer, version, FEATURE, renames, move |at| RenameParams {
                text_document_position: at.position(offset),
                new_name,
                work_done_progress_params: Default::default(),
            })
            .map(|answer| answer.map(|(reply, at, _)| (reply, at)))
            .boxed();
        self.decoded(reply)
    }

    /// The edits renaming `target`, prepared at byte `offset` of open
    /// buffer `buffer` at its `version`, to `new_name`: sent only to the
    /// target's [`owner`](RenameTarget::owner), the server that accepted
    /// the preparation, and only while its session still has the document
    /// open as it had it then. Otherwise (the server stopped or restarted
    /// since, or reopened the document) nothing is sent to any server, and
    /// the answer is `Unavailable("the language server restarted")`. A
    /// target with no owner is renamed as [`rename`](Self::rename) does.
    ///
    /// `Err(LspError::RefusedEdit)` if the edit can't be applied as the
    /// server meant.
    pub fn rename_prepared(
        &self,
        buffer: BufferId,
        version: Version,
        target: &RenameTarget,
        offset: usize,
        new_name: String,
    ) -> impl Future<Output = Result<Vec<FileEdits>, LspError>> + Send + 'static + use<> {
        let Some(owner) = target.owner else {
            return self.rename(buffer, version, offset, new_name).boxed();
        };
        let reply = self
            .owner_request::<Rename>(owner, version, FEATURE, renames, move |at| RenameParams {
                text_document_position: at.position(offset),
                new_name,
                work_done_progress_params: Default::default(),
            })
            .boxed();
        self.decoded(reply).boxed()
    }

    /// `reply`'s workspace edit, decoded, within [`RENAME_TIMEOUT`].
    fn decoded(
        &self,
        reply: BoxFuture<'static, Answered>,
    ) -> impl Future<Output = Result<Vec<FileEdits>, LspError>> + Send + 'static + use<> {
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

/// A rename's reply, with what it was admitted with.
type Answered = Result<(crate::Reply<Option<WorkspaceEdit>>, Admitted), LspError>;

/// What `Unavailable` says when no server renames.
const FEATURE: &str = "rename";

impl RenameTarget {
    /// The target, owned by `server`, which answered as `at`.
    fn owned(self, server: ServerId, at: &Admitted) -> Self {
        Self {
            owner: Some(Owner::of(server, at)),
            ..self
        }
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
            Some(RenameTarget::new(
                range.clone(),
                text.slice_to_string(range),
            ))
        }
        PrepareRenameResponse::RangeWithPlaceholder { range, placeholder } => {
            Some(RenameTarget::new(bytes(range), placeholder))
        }
        PrepareRenameResponse::DefaultBehavior { .. } => default_target(at, offset),
    }
}

/// The word at `offset` of the document the server has.
fn default_target(at: &Admitted, offset: usize) -> Option<RenameTarget> {
    let text = &at.document.sent.text;
    word_at(text, offset).map(|range| RenameTarget::new(range.clone(), text.slice_to_string(range)))
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
