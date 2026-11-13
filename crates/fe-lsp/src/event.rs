//! What fe-lsp reports: events through the manager's sink, and request
//! errors.

use std::sync::{Arc, Mutex, PoisonError};

use fe_buffer::{BufferId, Version};
use futures::channel::oneshot;
use kiln_l10n::tr;

use crate::diagnostics::{Diagnostic, FileDiagnostic};
use crate::encoding::PositionEncoding;
use crate::identity::{IncarnationId, SessionId};
use crate::workspace_edit::{ApplyReply, FileEdits};

/// Identifies one server instance started by an [`LspManager`](crate::LspManager).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ServerId(pub(crate) u64);

/// What a server's refresh request asks the client to ask for again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refresh {
    /// `workspace/semanticTokens/refresh`.
    SemanticTokens,
    /// `workspace/inlayHint/refresh`.
    InlayHints,
    /// `workspace/codeLens/refresh`.
    CodeLens,
}

/// A notification fe-lsp handed a server that may change its index with
/// no edit to the documents it's asked about
/// (docs/decisions/q5j03ax2af-lsp-add-on-servers.md §5, "Asking again").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Notice {
    /// `workspace/didChangeWatchedFiles`.
    WatchedFiles,
    /// `workspace/didChangeConfiguration`.
    Configuration,
}

/// Sent to the manager's sink, on the server's thread.
///
/// A host showing diagnostics applies a `Diagnostics` only while its
/// `(session, incarnation)` is [`LspManager::active_identity`](crate::LspManager::active_identity)
/// for its buffer and server, and not one a `DiagnosticsCleared` it has
/// seen named (docs/decisions/lsp-client.md §4.1).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum LspEvent {
    /// A server's state changed.
    Status {
        server: ServerId,
        name: String,
        status: ServerStatus,
    },
    /// The server asked for a message to be shown (`window/showMessage`).
    ShowMessage {
        server: ServerId,
        name: String,
        typ: lsp_types::MessageType,
        message: String,
    },
    /// The server asked a question (`window/showMessageRequest`) with
    /// `actions` to choose from, the titles in the order it sent them.
    /// Answer through `reply`; unanswered, it's answered `null`, dismissed.
    ShowMessageRequest {
        server: ServerId,
        name: String,
        typ: lsp_types::MessageType,
        message: String,
        actions: Vec<String>,
        reply: MessageReply,
    },
    /// A server's diagnostics for an open buffer: all of them, replacing
    /// those it sent before. Their ranges are bytes of the buffer at
    /// `version`, which the buffer may have moved on from.
    Diagnostics {
        buffer: BufferId,
        session: SessionId,
        incarnation: IncarnationId,
        version: Version,
        server: ServerId,
        items: Vec<Diagnostic>,
    },
    /// `server`'s diagnostics for a file it doesn't have open, replacing
    /// those it sent for `uri` before (none clears them). Their ranges are
    /// as sent, in `encoding`: decode them against the file's text with
    /// [`decode_file_diagnostics`](crate::decode_file_diagnostics). `path`
    /// is set for a `file:` URI only. The host keeps them until the next for
    /// that URI, or until the server's `Status` becomes `Restarting` or
    /// `Stopped` (the session that sent them has ended).
    FileDiagnostics {
        server: ServerId,
        uri: lsp_types::Url,
        path: Option<std::path::PathBuf>,
        encoding: PositionEncoding,
        items: Vec<FileDiagnostic>,
    },
    /// The incarnation `(session, incarnation)` of `buffer` on `server` has
    /// ended (its `didClose`, or its session's end), and was retired from
    /// the active identities first: its diagnostics no longer stand.
    DiagnosticsCleared {
        buffer: BufferId,
        session: SessionId,
        incarnation: IncarnationId,
        server: ServerId,
    },
    /// The server asked for a feature's answers to be asked for again, in
    /// every document it has open. The client capabilities that invite
    /// these requests are advertised by the feature cards, not here.
    Refresh { server: ServerId, what: Refresh },
    /// `what` was just handed to the server (not necessarily read yet). Its
    /// answers may change once it has dealt with it, so a host may ask it
    /// again for those it shows; [`LspManager::repoll_due`](crate::LspManager::repoll_due)
    /// catches what changes later.
    Notified { server: ServerId, what: Notice },
    /// The server asked for `edits` to be applied (`workspace/applyEdit`,
    /// a code action's command, say), under `label` for undo. Answer
    /// through `reply`: unanswered after 5 s, it's answered not applied.
    ApplyEdit {
        server: ServerId,
        label: Option<String>,
        edits: Vec<FileEdits>,
        reply: ApplyReply,
    },
}

/// The host's answer to a `window/showMessageRequest`, sent with
/// [`LspEvent::ShowMessageRequest`]. Clones share it and only the first
/// answer counts. Dropped unanswered by every clone, it answers `null`
/// (dismissed). It has no timeout: the request blocks nothing, and if the
/// server's session ends first the answer goes nowhere.
#[derive(Clone)]
pub struct MessageReply(Arc<MessageSlot>);

pub(crate) struct MessageSlot {
    /// The request's actions, as the server sent them.
    actions: Vec<lsp_types::MessageActionItem>,
    answer: Mutex<Option<oneshot::Sender<Option<lsp_types::MessageActionItem>>>>,
}

impl MessageReply {
    pub(crate) fn new(
        actions: Vec<lsp_types::MessageActionItem>,
    ) -> (
        Self,
        oneshot::Receiver<Option<lsp_types::MessageActionItem>>,
    ) {
        let (send, receive) = oneshot::channel();
        let slot = MessageSlot {
            actions,
            answer: Mutex::new(Some(send)),
        };
        (Self(Arc::new(slot)), receive)
    }

    /// Answers with the action at `index`, as the server sent it. An index
    /// out of range answers `null`.
    pub fn choose(self, index: usize) {
        self.send(self.0.actions.get(index).cloned());
    }

    /// Answers `null`: the question was dismissed.
    pub fn dismiss(self) {
        self.send(None);
    }

    fn send(&self, answer: Option<lsp_types::MessageActionItem>) {
        let taken = (self.0.answer.lock())
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(taken) = taken {
            let _ = taken.send(answer);
        }
    }
}

impl PartialEq for MessageReply {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for MessageReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MessageReply")
            .field("actions", &self.0.actions.len())
            .finish()
    }
}

/// Where a server is in its life.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ServerStatus {
    /// From launch until it's ready, with no counted progress open.
    Starting,
    /// Not ready yet, with work-done progress open: the title of the
    /// latest begun.
    Indexing { title: String },
    /// Ready (§3.4), until the session ends: requests go to it.
    Ready,
    /// The *n*th crash within 3 minutes ended its session; it starts again
    /// after a backoff (§3.5).
    Restarting(u32),
    /// Gone for good: `None` after a requested stop, otherwise why it
    /// stopped (it couldn't launch, or crashed too often). Its thread has
    /// ended or is about to.
    Stopped(Option<String>),
}

/// Why a request got no answer.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LspError {
    /// The server isn't running, stopped, or went away before answering.
    ServerDown,
    /// The request was cancelled (`RequestCancelled`).
    Cancelled,
    Timeout,
    /// The server can't answer this; something else should.
    Unavailable(Unavailable),
    /// The server says its answer would be stale (`ContentModified`,
    /// `ServerCancelled`): ask again.
    Retry,
    /// The server answered with an error.
    Server {
        code: i32,
        message: String,
    },
    /// The reply couldn't be read.
    Protocol(String),
    /// A workspace edit that can't be applied as the server meant it, and
    /// why (crate::workspace_edit).
    RefusedEdit(String),
}

impl std::fmt::Display for LspError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ServerDown => f.write_str(&tr!("lsp-error-server-down")),
            Self::Cancelled => f.write_str(&tr!("lsp-error-cancelled")),
            Self::Timeout => f.write_str(&tr!("lsp-error-timeout")),
            Self::Unavailable(why) => why.fmt(f),
            Self::Retry => f.write_str(&tr!("lsp-error-retry")),
            Self::Server { code, message } => f.write_str(&tr!(
                "lsp-error-server",
                message = message.as_str(),
                code = *code
            )),
            Self::Protocol(why) => f.write_str(&tr!("lsp-error-protocol", why = why.as_str())),
            Self::RefusedEdit(why) => {
                f.write_str(&tr!("lsp-error-refused-edit", why = why.as_str()))
            }
        }
    }
}

impl std::error::Error for LspError {}

/// Why [`LspError::Unavailable`]: one of a request on a document's kinds
/// (§4.1), which callers match on, or free text (a feature the server
/// doesn't provide, such as `"rename"`, or another reason). Each kind's
/// text is resolved in the active locale when it's displayed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unavailable {
    /// The buffer moved on from the version the request was computed on.
    BufferChanged,
    /// The document is over the size limit, so the server doesn't have it.
    TooLarge,
    /// The document isn't open on a language server.
    NotOpen,
    /// Anything else, as it's shown.
    Other(String),
}

impl std::fmt::Display for Unavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferChanged => f.write_str(&tr!("lsp-unavailable-buffer-changed")),
            Self::TooLarge => f.write_str(&tr!("lsp-unavailable-too-large")),
            Self::NotOpen => f.write_str(&tr!("lsp-unavailable-not-open")),
            Self::Other(why) => f.write_str(why),
        }
    }
}

impl From<&str> for Unavailable {
    fn from(why: &str) -> Self {
        Self::Other(why.to_owned())
    }
}

impl From<String> for Unavailable {
    fn from(why: String) -> Self {
        Self::Other(why)
    }
}

/// Free text equals its text; a kind equals no string, whatever the
/// locale: match it.
impl PartialEq<str> for Unavailable {
    fn eq(&self, other: &str) -> bool {
        matches!(self, Self::Other(why) if why == other)
    }
}

impl PartialEq<&str> for Unavailable {
    fn eq(&self, other: &&str) -> bool {
        *self == **other
    }
}

/// [`LspError::Unavailable`]'s reasons for a request on a document (§4.1).
/// The buffer moved on from the version the request was computed on.
pub(crate) fn buffer_changed() -> Unavailable {
    Unavailable::BufferChanged
}
/// The document is over the size limit, so the server doesn't have it.
pub(crate) fn too_large() -> Unavailable {
    Unavailable::TooLarge
}
/// The document isn't open on a language server.
pub(crate) fn not_open() -> Unavailable {
    Unavailable::NotOpen
}

impl LspError {
    pub(crate) fn from_lsp(err: async_lsp::Error) -> Self {
        match err {
            async_lsp::Error::ServiceStopped | async_lsp::Error::Eof => Self::ServerDown,
            async_lsp::Error::Response(err) => match err.code {
                async_lsp::ErrorCode::REQUEST_CANCELLED => Self::Cancelled,
                async_lsp::ErrorCode::CONTENT_MODIFIED | async_lsp::ErrorCode::SERVER_CANCELLED => {
                    Self::Retry
                }
                code => Self::Server {
                    code: code.0,
                    message: err.message,
                },
            },
            err => Self::Protocol(err.to_string()),
        }
    }
}

/// How much of each server's stderr and `window/logMessage` is kept.
pub(crate) const LOG_BYTES: usize = 64 * 1024;

/// The last [`LOG_BYTES`] a server wrote to stderr or logged, across its
/// sessions, read with [`LspManager::log`](crate::LspManager::log).
#[derive(Default)]
pub(crate) struct LogRing(std::collections::VecDeque<u8>);

pub(crate) type SharedLog = std::sync::Arc<std::sync::Mutex<LogRing>>;

impl LogRing {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let bytes = &bytes[bytes.len().saturating_sub(LOG_BYTES)..];
        let over = (self.0.len() + bytes.len()).saturating_sub(LOG_BYTES);
        self.0.drain(..over);
        self.0.extend(bytes);
    }

    /// What's kept, from its first whole character on.
    pub(crate) fn text(&self) -> String {
        let bytes: Vec<u8> = self.0.iter().copied().collect();
        let start = bytes
            .iter()
            .position(|&b| b & 0xc0 != 0x80)
            .unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[start..]).into_owned()
    }
}

/// Here, not in `tests/`: these name the typed reason.
#[cfg(test)]
mod unavailable_tests {
    use super::Unavailable;

    #[test]
    fn a_kind_is_matched_never_compared_with_its_text() {
        assert_eq!(Unavailable::BufferChanged.to_string(), "buffer changed");
        assert_ne!(Unavailable::BufferChanged, "buffer changed");
        assert_ne!(Unavailable::NotOpen, "not open on a language server");
        assert_eq!(Unavailable::from("rename"), "rename");
        assert_eq!(Unavailable::from(String::from("rename")), *"rename");
        assert_ne!(
            Unavailable::from("buffer changed"),
            Unavailable::BufferChanged
        );
    }
}
