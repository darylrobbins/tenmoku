//! The GPUI view of a terminal session: [`TerminalView`] paints
//! `fe-terminal`'s grid in a [`TerminalStyle`]'s font and colours, keeps the
//! pty sized to the view, and writes to it what is typed, composed and
//! pasted. Which cell paints what is worked out in [`layout`], without a
//! window. Depends on no fastedit editor crate, so it
//! can move into kiln; the workbench builds the style from its theme.

mod element;
mod input;
pub mod layout;
mod style;
mod view;

pub use fe_terminal::TerminalOptions;
pub use style::TerminalStyle;
pub use view::{KEY_CONTEXT, TerminalView, TerminalViewEvent};
