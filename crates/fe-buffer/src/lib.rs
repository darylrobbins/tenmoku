//! Editable buffers for fastedit.
//!
//! - [`Buffer`] applies batches of edits atomically and records each as a
//!   transaction tagged with an [`Origin`] (user, AI, LSP, …). Undo and redo can
//!   be scoped by origin and work even when later edits exist.
//! - [`Anchor`]s are positions that follow the text through edits.
//!   [`Buffer::edit_from`] applies edits computed against an older snapshot,
//!   which is how AI proposals land while the user keeps typing.
//! - [`BufferSnapshot`] is an O(1), thread-safe copy for background work.
//!
//! This crate must never depend on GPUI.

mod buffer;
mod edit;
mod log;
mod selection;
mod snapshot;

pub use buffer::{
    Buffer, BufferEvent, EditError, EditKind, Origin, Subscription, TransactionId, UndoError,
};
pub use edit::Edit;
pub use fe_text::{Bias, LineEnding, Point, PointUtf16, Text};
pub use selection::{ResolvedSelection, Selection, SelectionSet};
pub use snapshot::{Anchor, BufferId, BufferSnapshot, Version};

#[cfg(test)]
mod tests;
