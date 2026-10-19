//! fastedit's language server client (docs/decisions/lsp-client.md).
//!
//! - [`LspManager`] starts a server for a [`ServerSpec`] and a workspace
//!   root, each on its own thread named `fe-lsp <name>`, running async-lsp's
//!   main loop and an actor that owns the session under
//!   `async_io::block_on`. No tokio, smol or other executor.
//! - A [`ServerLauncher`] hands it the server's byte streams
//!   ([`ServerIo`]), on that thread. [`LocalLauncher`] spawns a local
//!   process in its own process group; a host may pass its own to sandbox,
//!   proxy or refuse servers.
//! - Results leave in two ways, neither tied to an executor: request
//!   futures ([`LspManager::request`]) that are `Send` and may be awaited
//!   anywhere, and an `FnMut(LspEvent) + Send` sink called on server
//!   threads.
//! - Every timeout is measured on an injected [`Clock`].
//! - [`LspManager::open`] picks a document's server by its language, from
//!   built-in specs, specs extensions contribute
//!   ([`LspManager::set_extension_specs`], each made by
//!   [`extension_spec`]), the user's `[lsp]` settings
//!   ([`LspManager::update_settings`]) and specs the host registers
//!   ([`LspManager::register_spec`]), and starts one instance per (server,
//!   workspace root) the first time a document needs it.
//! - Documents are synced from the buffer's own events: the host hands
//!   [`LspManager::open`] a `fe_buffer::Subscription`, and the server's
//!   thread drains it ([`LspManager::changed`] nudges it to).
//! - A server that crashes restarts as a new session, within a rolling
//!   budget; its status events say where it is, from `Starting` through
//!   `Ready` to `Restarting(n)` or `Stopped`. Every reply says whether an
//!   empty result is final ([`Reply`]).
//! - A server's `textDocument/publishDiagnostics` are decoded on its
//!   thread against the text it was published for, and so are the replies
//!   to `textDocument/diagnostic`, which open documents are pulled with
//!   when the server advertises `diagnosticProvider` (or, for
//!   typescript-language-server, to tsserver's own diagnostic requests:
//!   [`PullDiagnostics`]). Both lists reach the
//!   sink together as [`LspEvent::Diagnostics`] in byte ranges, with
//!   [`LspEvent::DiagnosticsCleared`] when the document closes or the
//!   session ends.
//! - [`LspManager::hover`] asks for a [`Hover`] at an offset of an open
//!   buffer: its text, and its range in that buffer's version.
//! - [`LspManager::inlay_hints`] asks for the [`InlayHint`]s in a byte range of
//!   an open buffer, and [`LspManager::resolve_inlay_hint`] fills in a hint's
//!   tooltip and edits.
//! - [`LspManager::semantic_tokens`] asks for an open buffer's
//!   [`SemanticTokens`], in full or as a delta of the last ones: byte
//!   ranges of the buffer's version, with the names the server's legend
//!   gives their types and modifiers.
//! - [`LspManager::completion`] asks for [`Completions`] at an offset of
//!   an open buffer, as edits in that buffer's version, and
//!   [`LspManager::resolve`] fills in an item's documentation.
//! - Workspace edits (a rename's, a code action's, or a server's
//!   `workspace/applyEdit`, which reaches the sink as
//!   [`LspEvent::ApplyEdit`]) are decoded into [`FileEdits`]: byte edits
//!   of the version the server has of each open file, and LSP edits for
//!   the host to place in each closed one ([`workspace_edit`]).
//! - [`rename`], [`code_action`], [`formatting`] and [`signature_help`]
//!   hold those features.
//! - [`LspNavigation`] answers fe-nav's go-to-definition, references and
//!   workspace symbols from a manager's servers, once each is ready.
//!
//! Two facts for a host embedding fe-lsp:
//!
//! - async-io starts its reactor thread lazily, and async-process starts an
//!   `async-process` thread lazily, the first time a server needs them.
//! - On macOS (anything but Linux and Windows), async-process reaps
//!   children through a `SIGCHLD` handler, which async-signal registers with
//!   `signal-hook-registry`. tokio uses the same registry, so a tokio host's
//!   handler and fe-lsp's chain rather than clobber each other.
//!
//! No public item names an async-lsp type, so the client underneath can be
//! replaced without changing this API.
//!
//! This crate must never depend on GPUI.

mod clock;
pub mod code_action;
mod completion;
mod diagnostics;
mod document;
pub mod document_highlight;
mod encoding;
mod event;
mod file_operations;
pub mod formatting;
#[allow(dead_code)] // Nothing calls it until the watched-files and file-operation cards.
mod glob;
mod hover;
mod identity;
mod inlay_hint;
mod launcher;
mod manager;
mod navigation;
#[allow(dead_code)] // Nothing calls it until the watched-files and file-operation cards.
mod notify;
mod pull;
mod readiness;
mod registration;
mod registry;
pub mod rename;
mod requests;
mod restart;
mod router;
mod semantic_tokens;
mod server;
pub mod signature_help;
mod tsserver;
pub mod workspace_edit;

#[cfg(any(test, feature = "test-support"))]
#[path = "tests/fake.rs"]
pub mod fake;

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use clock::ManualClock;
pub use clock::{Clock, SystemClock};
pub use code_action::{CodeAction, Payload};
pub use completion::{CompletionEdit, CompletionItem, CompletionTrigger, Completions, kind_name};
pub use diagnostics::{
    Diagnostic, DiagnosticTag, FileDiagnostic, Related, Severity, decode_file_diagnostics,
};
pub use document::{Admitted, Opened, SentDocument, SentVersion};
pub use document_highlight::{DocumentHighlight, HighlightKind};
pub use encoding::{PositionEncoding, from_lsp, to_lsp};
pub use event::{LspError, LspEvent, MessageReply, Refresh, ServerId, ServerStatus};
pub use hover::Hover;
pub use identity::{IncarnationId, SessionId};
pub use inlay_hint::{InlayHint, InlayHintKind};
pub use launcher::{
    ExitInfo, LOGIN_SHELL_TIMEOUT, LaunchCancel, LocalLauncher, LoginPath, PullDiagnostics,
    ReadinessMode, ServerIo, ServerLauncher, ServerProcess, ServerSpec, UnversionedDiagnostics,
};
pub use manager::{DEFAULT_MAX_FILE_BYTES, KillSwitch, LspManager};
pub use navigation::LspNavigation;
pub use registry::extension_spec;
pub use requests::{ORPHANS_MAX, Reply};
pub use semantic_tokens::{SemanticToken, SemanticTokens};
pub use signature_help::{Signature, SignatureContext, SignatureHelp, SignatureTrigger};
pub use workspace_edit::{ApplyClaim, ApplyReply, EditTarget, FileEdits, decode_closed};
