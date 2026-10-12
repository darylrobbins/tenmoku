//! `textDocument/codeAction` and `codeAction/resolve`: quick fixes and
//! refactors at a range, whose edits are workspace edits
//! ([`crate::workspace_edit`]) and whose commands the server may carry out
//! through `workspace/applyEdit`.
//!
//! - **Request.** The range and the diagnostics the host shows over it,
//!   in the session's encoding, each with the `data` its server gave it.
//!   Timeout 3 s (`lsp.timeouts.hover`'s): an empty list.
//! - **Order.** Quick fixes first, preferred ones first, then as the
//!   server sent them. A bare `Command` is an action with that title and
//!   no kind. Disabled actions are kept, with their reason, and never run.
//! - **Edits.** Decoded when the action arrives, against the document
//!   version the request was made on; [`resolve_code_action`] decodes the
//!   resolved edit against the documents the server has then.
//!
//! [`resolve_code_action`]: LspManager::resolve_code_action

use std::ops::Range;
use std::sync::Arc;

use fe_buffer::{BufferId, Version};
use fe_text::Bias;
use futures::FutureExt;
use lsp_types::request::{CodeActionRequest, CodeActionResolveRequest, ExecuteCommand};
use lsp_types::{
    CodeActionContext, CodeActionKind, CodeActionOrCommand, CodeActionParams,
    CodeActionTriggerKind, ExecuteCommandParams, NumberOrString, TextDocumentIdentifier,
};

use crate::diagnostics::{Diagnostic, DiagnosticTag, Severity};
use crate::document::{Admitted, Opened};
use crate::encoding::to_lsp;
use crate::event::LspError;
use crate::event::ServerId;
use crate::identity::{IncarnationId, SessionId};
use crate::manager::LspManager;
use crate::workspace_edit::FileEdits;

/// What a server offers to do at a range.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct CodeAction {
    pub title: String,
    /// As the server gave it (`quickfix`, `refactor.extract`, ...).
    pub kind: Option<String>,
    pub is_preferred: bool,
    /// Why it can't be run now, if it can't: to grey it out with.
    pub disabled: Option<String>,
    /// How many of the diagnostics the request carried it says it fixes.
    pub diagnostics_fixed: usize,
    /// What's needed to resolve and run it.
    pub payload: Payload,
}

impl CodeAction {
    /// A quick fix, or a kind within one (`quickfix.import`).
    pub fn is_quick_fix(&self) -> bool {
        self.kind.as_deref().is_some_and(|kind| {
            kind == CodeActionKind::QUICKFIX.as_str()
                || kind
                    .strip_prefix(CodeActionKind::QUICKFIX.as_str())
                    .is_some_and(|rest| rest.starts_with('.'))
        })
    }
}

/// What an action keeps of the server's reply: opaque to the host.
#[derive(Clone, Debug)]
pub struct Payload {
    server: ServerId,
    /// The document it was asked for, as that session had it open: only
    /// then does it mean anything to the server.
    from: (BufferId, (SessionId, IncarnationId)),
    /// A bare `Command` in the reply, which isn't resolved.
    bare: bool,
    raw: Arc<lsp_types::CodeAction>,
    /// Its edit, decoded when it came.
    edits: Result<Vec<FileEdits>, LspError>,
}

impl LspManager {
    /// The code actions for `range` (bytes of `version`) of open buffer
    /// `buffer`, with `diagnostics`: those the host shows overlapping it.
    ///
    /// Empty if the server offers none, or doesn't say within
    /// `lsp.timeouts.hover` (which also cancels the request). Dropping the
    /// future cancels it too. It doesn't borrow the manager.
    pub fn code_actions(
        &self,
        buffer: BufferId,
        version: Version,
        range: Range<usize>,
        diagnostics: Vec<Diagnostic>,
    ) -> impl Future<Output = Result<Vec<CodeAction>, LspError>> + Send + 'static + use<> {
        let server = self.server_of(buffer);
        let given = diagnostics.clone();
        let reply = self.document_request::<CodeActionRequest>(buffer, version, move |at| {
            let text = &at.document.sent.text;
            let end = text.clip_offset(range.end, Bias::Right);
            let start = text.clip_offset(range.start, Bias::Left).min(end);
            CodeActionParams {
                text_document: TextDocumentIdentifier {
                    uri: at.document.uri.clone(),
                },
                range: lsp_types::Range::new(
                    to_lsp(text, start, at.encoding),
                    to_lsp(text, end, at.encoding),
                ),
                context: CodeActionContext {
                    diagnostics: diagnostics.iter().map(|d| encode(d, at)).collect(),
                    only: None,
                    trigger_kind: Some(CodeActionTriggerKind::INVOKED),
                },
                work_done_progress_params: Default::default(),
                partial_result_params: Default::default(),
            }
        });
        // Boxed, as its type names the manager's borrow.
        reply.boxed().map(move |answer| match (answer, server) {
            (Ok((reply, admitted)), Some(server)) => {
                let sent: Vec<_> = given.iter().map(|d| encode(d, &admitted)).collect();
                let mut actions: Vec<_> = (reply.result.into_iter().flatten())
                    .map(|action| convert(action, &admitted, server, &sent))
                    .collect();
                // Stable: the server's order within each rank.
                let rank =
                    |action: &CodeAction| match (action.is_quick_fix(), action.diagnostics_fixed) {
                        (true, 1..) => 0,
                        (true, 0) => 1,
                        (false, _) => 2,
                    };
                actions.sort_by_key(|action| (rank(action), !action.is_preferred));
                Ok(actions)
            }
            (Ok(_), None) | (Err(LspError::Timeout), _) => Ok(Vec::new()),
            (Err(err), _) => Err(err),
        })
    }

    /// `action` with the edit its server resolves: `codeAction/resolve`, if
    /// the server advertises `resolveProvider` and the action has no edit.
    /// Otherwise, or if the server doesn't answer within
    /// `lsp.timeouts.resolve`, the action as it is.
    pub fn resolve_code_action(
        &self,
        action: &CodeAction,
    ) -> impl Future<Output = Result<CodeAction, LspError>> + Send + 'static + use<> {
        let action = action.clone();
        let server = action.payload.server;
        let from = action.payload.from;
        let resolves = action.disabled.is_none()
            && !action.payload.bare
            && action.payload.raw.edit.is_none()
            && self.capabilities(server).is_some_and(|capabilities| {
                matches!(
                    capabilities.code_action_provider,
                    Some(lsp_types::CodeActionProviderCapability::Options(
                        lsp_types::CodeActionOptions {
                            resolve_provider: Some(true),
                            ..
                        }
                    ))
                )
            });
        let reply = resolves.then(|| {
            self.request_with_documents_if::<CodeActionResolveRequest>(
                server,
                (*action.payload.raw).clone(),
                move |opened| has_open(opened, from),
            )
            .boxed()
        });
        async move {
            let Some(reply) = reply else {
                return Ok(action);
            };
            match reply.await {
                Err(LspError::Timeout) => Ok(action),
                Err(err) => Err(err),
                // The session or document it came from has gone.
                Ok(None) => Ok(action),
                Ok(Some((reply, opened))) => {
                    let mut action = action;
                    let resolved = reply.result;
                    action.payload.edits = match resolved.edit.clone() {
                        Some(edit) => opened.decode_workspace_edit(edit),
                        None => Ok(Vec::new()),
                    };
                    action.payload.raw = Arc::new(lsp_types::CodeAction {
                        // What the host shows stays as it was.
                        title: action.payload.raw.title.clone(),
                        diagnostics: action.payload.raw.diagnostics.clone(),
                        command: resolved
                            .command
                            .clone()
                            .or_else(|| action.payload.raw.command.clone()),
                        ..resolved
                    });
                    Ok(action)
                }
            }
        }
    }

    /// `action`'s edit decoded: empty if it has none. The edit of an
    /// action as it came, or as [`resolve_code_action`](Self::resolve_code_action)
    /// gave it.
    pub fn code_action_edits(&self, action: &CodeAction) -> Result<Vec<FileEdits>, LspError> {
        action.payload.edits.clone()
    }

    /// Has `action`'s server run its command, with its arguments
    /// (`workspace/executeCommand`). Any edit it applies in doing so
    /// arrives as [`LspEvent::ApplyEdit`](crate::LspEvent::ApplyEdit).
    ///
    /// Nothing is sent for an action with no command, and a disabled one
    /// is an error: it's never run.
    pub fn run_code_action_command(
        &self,
        action: &CodeAction,
    ) -> impl Future<Output = Result<(), LspError>> + Send + 'static + use<> {
        let server = action.payload.server;
        let refused = action
            .disabled
            .as_ref()
            .map(|why| LspError::Unavailable(format!("disabled: {why}")));
        let from = action.payload.from;
        let reply = match (&refused, &action.payload.raw.command) {
            (None, Some(command)) => Some(
                self.request_if::<ExecuteCommand>(
                    server,
                    ExecuteCommandParams {
                        command: command.command.clone(),
                        arguments: command.arguments.clone().unwrap_or_default(),
                        work_done_progress_params: Default::default(),
                    },
                    move |opened| has_open(opened, from),
                )
                .boxed(),
            ),
            _ => None,
        };
        async move {
            if let Some(err) = refused {
                return Err(err);
            }
            match reply {
                Some(reply) => match reply.await? {
                    Some(_) => Ok(()),
                    None => Err(LspError::Unavailable(
                        "the action is from an earlier session or opening".into(),
                    )),
                },
                None => Ok(()),
            }
        }
    }
}

/// Whether the server has the document an action came from open as it was.
fn has_open(opened: &Opened, (buffer, identity): (BufferId, (SessionId, IncarnationId))) -> bool {
    (opened.documents.iter())
        .any(|document| document.buffer == buffer && document.identity == identity)
}

/// `diagnostic` as the server has the document `at`.
fn encode(diagnostic: &Diagnostic, at: &Admitted) -> lsp_types::Diagnostic {
    let text = &at.document.sent.text;
    let end = text.clip_offset(diagnostic.range.end, Bias::Right);
    let start = text
        .clip_offset(diagnostic.range.start, Bias::Left)
        .min(end);
    let tags: Vec<_> = (diagnostic.tags.iter())
        .map(|tag| match tag {
            DiagnosticTag::Unnecessary => lsp_types::DiagnosticTag::UNNECESSARY,
            DiagnosticTag::Deprecated => lsp_types::DiagnosticTag::DEPRECATED,
        })
        .collect();
    lsp_types::Diagnostic {
        range: lsp_types::Range::new(
            to_lsp(text, start, at.encoding),
            to_lsp(text, end, at.encoding),
        ),
        severity: diagnostic.severity.map(|severity| match severity {
            Severity::Error => lsp_types::DiagnosticSeverity::ERROR,
            Severity::Warning => lsp_types::DiagnosticSeverity::WARNING,
            Severity::Information => lsp_types::DiagnosticSeverity::INFORMATION,
            Severity::Hint => lsp_types::DiagnosticSeverity::HINT,
        }),
        // A code of digits was a number as often as not.
        code: diagnostic
            .code
            .as_ref()
            .map(|code| match code.parse::<i32>() {
                Ok(number) if number.to_string() == *code => NumberOrString::Number(number),
                _ => NumberOrString::String(code.clone()),
            }),
        source: diagnostic.source.clone(),
        message: diagnostic.message.clone(),
        tags: (!tags.is_empty()).then_some(tags),
        data: diagnostic.data.clone(),
        ..Default::default()
    }
}

/// One entry of a reply as a [`CodeAction`]. `sent` are the diagnostics the
/// request carried, as it carried them.
fn convert(
    entry: CodeActionOrCommand,
    at: &Admitted,
    server: ServerId,
    sent: &[lsp_types::Diagnostic],
) -> CodeAction {
    let (raw, bare) = match entry {
        CodeActionOrCommand::Command(command) => (
            lsp_types::CodeAction {
                title: command.title.clone(),
                command: Some(command),
                ..Default::default()
            },
            true,
        ),
        CodeActionOrCommand::CodeAction(action) => (action, false),
    };
    let edits = match raw.edit.clone() {
        Some(edit) => at.decode_workspace_edit(edit),
        None => Ok(Vec::new()),
    };
    let fixed = (raw.diagnostics.iter().flatten())
        .filter(|fixed| {
            sent.iter().any(|sent| {
                (sent.range, &sent.message, &sent.code)
                    == (fixed.range, &fixed.message, &fixed.code)
            })
        })
        .count();
    CodeAction {
        title: raw.title.clone(),
        kind: raw.kind.as_ref().map(|kind| kind.as_str().to_owned()),
        is_preferred: raw.is_preferred == Some(true),
        disabled: raw
            .disabled
            .as_ref()
            .map(|disabled| disabled.reason.clone()),
        diagnostics_fixed: fixed,
        payload: Payload {
            server,
            from: (at.document.buffer, at.document.identity),
            bare,
            raw: Arc::new(raw),
            edits,
        },
    }
}

#[cfg(test)]
#[path = "code_action_tests.rs"]
mod tests;
