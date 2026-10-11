//! Dragging a tab is kiln-shell's ([`kiln_shell::pane`]): what's dragged,
//! the ghost following the pointer, and where in a pane a drop lands,
//! which a pane shows while hovered. Here, fastedit's names for them over
//! its [`Pane`].

use crate::Pane;

#[cfg(test)]
pub(crate) use kiln_shell::pane::{DropTarget, DropZone, marker};

/// A tab being dragged: its pane, and the tab, by ID.
pub(crate) type DraggedTab = kiln_shell::pane::DraggedTab<Pane>;

/// A tab dropped on a pane, for the workbench to move.
pub(crate) type TabDropped = kiln_shell::pane::TabDropped<Pane>;

#[cfg(test)]
#[path = "pane_drag_followup_tests.rs"]
mod followup_tests;
