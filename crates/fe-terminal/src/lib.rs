//! Terminal sessions for the terminal pane: a pty running a shell, the VT grid
//! its output draws (with scrollback and selection), and xterm key encoding.
//! Built on alacritty_terminal (Apache-2.0; docs/decisions/terminal.md), whose
//! types never appear in this crate's API, so the GPUI view (`fe-terminal-view`)
//! doesn't name it. Must not depend on GPUI.

pub mod keys;
mod links;
mod pty_loop;
mod search;
mod session;
mod snapshot;

pub use links::{Link, LinkTarget, link_at};
pub use search::{GridPoint, QueryError, TerminalMatch, TerminalQuery};
pub use session::{GridSize, SelectionKind, Side, Terminal, TerminalEvent, TerminalOptions};
pub use snapshot::{
    Cell, CellFlags, Color, Cursor, CursorShape, Modes, SelectionSpan, Snapshot, indexed_rgb,
};
