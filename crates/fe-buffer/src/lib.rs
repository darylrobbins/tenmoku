//! Editable buffers for fastedit.
//!
//! - [`Buffer`] applies batches of edits atomically and records each as a
//!   transaction tagged with an [`Origin`] (user, AI, LSP, …). Undo and redo can
//!   be scoped by origin and work even when later edits exist.
//! - [`Anchor`]s are positions that follow the text through edits.
//!   [`Buffer::edit_from`] applies edits computed against an older snapshot,
//!   which is how AI proposals land while the user keeps typing.
//! - [`BufferSnapshot`] is an O(1), thread-safe copy for background work.
//! - [`ProposedChange`] holds edits made against a snapshot, such as an AI's,
//!   as anchored hunks the user accepts or rejects one by one; a hunk goes
//!   stale once the user edits the text under it. Each accepted hunk is a
//!   transaction under the accepting origin (`Origin::User` from the editor,
//!   so cmd-z undoes it, as in VS Code and Cursor; decided 2026-10-09,
//!   replacing T240's AI-transaction accept) or the proposal's own. The
//!   proposal's origin records who proposed it, and undo reverts the text but
//!   leaves the hunk accepted. [`ProposedChange::dry_run`] reports, without
//!   changing anything, which hunks would apply and why the rest can't.
//! - [`EditStream`] writes an AI's output into a target range as it arrives,
//!   chunk by chunk, while the user keeps typing. Streaming: the whole stream
//!   is one transaction under the AI's origin, grown with
//!   [`Buffer::edit_in`], so the user's undo never touches it and one undo of
//!   AI origins reverts all of it and restores the old target text; redo
//!   brings it back. Text the user types at the region's edges stays outside
//!   it. Cancelling, by [`EditStream::finish`] or by dropping the stream,
//!   keeps what landed.
//! - [`Provenance`] holds who wrote which bytes of a buffer's text, an
//!   [`Author`] per span, following edits, undo and redo; text accepted from a
//!   proposal is its agent's, whatever origin it's applied under.
//! - [`BackupStore`] keeps backups of unsaved buffers on disk, privately and
//!   atomically, for hot exit and crash recovery; [`Buffer::from_backup`]
//!   restores one, dirty over the file's current text.
//! - [`LocalHistory`] keeps past texts of each file on disk, by path, newest
//!   first, bounded by count, age and total size; see [`local_history`].
//!
//! This crate must never depend on GPUI.

mod backup;
mod buffer;
mod edit;
pub mod local_history;
mod log;
mod marker;
mod proposed;
mod provenance;
mod selection;
mod snapshot;
mod stream;

pub use backup::{Backup, BackupId, BackupMeta, BackupStore, ParseBackupIdError};
pub use buffer::{
    Buffer, BufferEvent, DiskStamp, EditError, EditKind, HistoryTime, HistoryWrite, Origin,
    SaveSnapshot, Subscription, TransactionId, UndoError,
};
pub use edit::Edit;
pub use fe_text::{Bias, DecodedFile, Encoding, FileEncoding, LineEnding, Point, PointUtf16, Text};
pub use local_history::LocalHistory;
pub use marker::{MarkerError, MarkerId, MarkerSet};
pub use proposed::{
    Conflict, DryRun, HunkId, HunkOutcome, HunkStatus, HunkView, Outcome, ProposalError,
    ProposedChange,
};
pub use provenance::{Author, Provenance};
pub use selection::{ResolvedSelection, Selection, SelectionSet};
pub use snapshot::{Anchor, BufferId, BufferSnapshot, Version};
pub use stream::{EditStream, StreamError};

#[cfg(test)]
mod tests;
