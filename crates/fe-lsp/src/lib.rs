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
//!   built-in specs, the user's `[lsp]` settings
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
mod document;
mod encoding;
mod event;
mod identity;
mod launcher;
mod manager;
mod readiness;
mod registration;
mod registry;
mod requests;
mod restart;
mod router;
mod server;

#[cfg(any(test, feature = "test-support"))]
#[path = "tests/fake.rs"]
pub mod fake;

#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use clock::ManualClock;
pub use clock::{Clock, SystemClock};
pub use document::{Admitted, SentDocument, SentVersion};
pub use encoding::{PositionEncoding, from_lsp, to_lsp};
pub use event::{LspError, LspEvent, ServerId, ServerStatus};
pub use identity::{IncarnationId, SessionId};
pub use launcher::{
    ExitInfo, LOGIN_SHELL_TIMEOUT, LocalLauncher, LoginPath, ReadinessMode, ServerIo,
    ServerLauncher, ServerProcess, ServerSpec, UnversionedDiagnostics,
};
pub use manager::{DEFAULT_MAX_FILE_BYTES, LspManager};
pub use requests::{ORPHANS_MAX, Reply};
