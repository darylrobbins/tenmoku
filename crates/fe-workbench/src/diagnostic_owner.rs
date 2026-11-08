//! Who an editor's diagnostics come from, as its one
//! [`fe_editor::diagnostics::DiagnosticSet`] keys them.
//!
//! An editor keeps its diagnostics keyed by one pair of types, and applying
//! under another pair replaces them all. So the language servers
//! ([`crate::lsp`]) and the plugins ([`crate::plugin_diagnostics`]) share one
//! pair: each server and each plugin is a [`DiagnosticOwner`], and what its
//! diagnostics came from a [`DiagnosticIdentity`]. A server's identity is the
//! buffer's `(session, incarnation)` on it; a plugin's is its load, so a
//! reload's diagnostics never mix with the load before.

use fe_lsp::{IncarnationId, ServerId, SessionId};

/// One source of an editor's diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DiagnosticOwner {
    /// A language server.
    Server(ServerId),
    /// A plugin, by the key [`crate::plugin_diagnostics`] gives its id.
    Plugin(u64),
}

/// What an owner's diagnostics came from, which ends when it does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DiagnosticIdentity {
    /// The buffer's incarnation on a server's session.
    Lsp(SessionId, IncarnationId),
    /// A plugin's load.
    Plugin(u64),
}
