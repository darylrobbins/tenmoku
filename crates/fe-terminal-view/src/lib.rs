//! The GPUI view of a terminal session: [`TerminalView`] paints
//! `fe-terminal`'s grid in a [`TerminalStyle`]'s font and colours, keeps the
//! pty sized to the view, writes to it what is typed, composed and pasted,
//! and selects, scrolls back and reports the mouse; ctrl-shift-space puts it
//! in vi copy mode ([`TerminalView::toggle_vi_mode`]). Which cell paints what is
//! worked out in [`layout`], and what a mouse report writes in [`mouse`],
//! without a window. Depends on no fastedit editor crate, so it can move into
//! kiln; the workbench builds the style from its theme.

mod element;
mod find;
mod input;
pub mod layout;
pub mod mouse;
mod style;
mod vi;
mod view;

#[doc(hidden)]
pub use element::cell_size;
#[doc(hidden)]
pub use element::{PaintReport, PaintedCursor, PaintedText};
pub use fe_terminal::{QueryError, TerminalMatch, TerminalOptions, TerminalQuery, ViMove};
pub use style::TerminalStyle;
pub use view::{KEY_CONTEXT, TerminalView, TerminalViewEvent};
