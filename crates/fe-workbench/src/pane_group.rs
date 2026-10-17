//! The editor area: kiln-shell's tree of panes over fastedit's [`Pane`]s,
//! split side by side or one above another
//! ([`kiln_shell::pane_group`]).
//!
//! Its commands (`pane.split_right`, `pane.focus_left`, `pane.move_tab_left`,
//! `pane.close` and so on) are the workbench's typed commands
//! (`workbench_commands.rs`).

pub use kiln_shell::pane_group::{Axis, Direction};
#[cfg(test)]
pub(crate) use kiln_shell::pane_group::{MIN_PANE_HEIGHT, MIN_PANE_WIDTH};

use crate::Pane;

/// A member of the tree: one of fastedit's panes, or an axis of members.
pub type Member = kiln_shell::Member<Pane>;

/// An axis's members, each with its share of the axis.
pub type Members = kiln_shell::Members<Pane>;

/// Where a tab moves to.
pub(crate) type Destination = kiln_shell::Destination<Pane>;

/// The tree of panes, kiln-shell's over fastedit's [`Pane`]s, which the
/// workbench's workspace holds and draws. It always has one at least.
pub type PaneGroup = kiln_shell::PaneGroup<Pane>;

/// How far a resize command moves a border.
pub(crate) const RESIZE_STEP: gpui::Pixels = gpui::px(32.);

#[cfg(test)]
#[path = "pane_resize_tests.rs"]
mod resize_tests;

#[cfg(test)]
#[path = "vim_window_keys_tests.rs"]
mod vim_window_keys_tests;
