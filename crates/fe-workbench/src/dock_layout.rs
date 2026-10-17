//! The docks' sizes and places from the keyboard: `dock.grow` and
//! `dock.shrink` change the size of the dock holding focus (the
//! terminal's while the terminal has focus), else the sidebar's, by
//! [`STEP`] within its limits ([`kiln_shell::dock`]), showing it if
//! hidden; `terminal.move_to_bottom`, `terminal.move_to_left` and
//! `terminal.move_to_right` move the terminal's dock to that edge, and
//! `dock.move_sidebar_left` and `dock.move_sidebar_right` the sidebar,
//! one dock per edge ([`displaced`]). They have no default keys: they're
//! for the palette and the user's keymap.
//!
//! They're typed commands in `workbench_commands.rs`.

use gpui::{App, Pixels, Window, px};
use kiln_shell::{DockPosition, DockState};

/// How much a command grows or shrinks a dock.
pub(crate) const STEP: Pixels = px(40.);

/// Grows `dock` by `by`, or shrinks it by less than nothing, within its
/// limits in `window`, and shows it.
pub(crate) fn resize(dock: &mut DockState, by: Pixels, window: &Window, cx: &App) {
    let size = dock.current_size() + by;
    dock.set_size(size.min(dock.max_size(window, cx)));
    dock.set_visible(true);
}

/// Where the other dock goes when a dock moves from `from` to `to`, the
/// other being at `other`: where it is, unless that's `to`, one dock per
/// edge; then to `from` if that's a side, else to the side opposite `to`.
pub(crate) fn displaced(from: DockPosition, to: DockPosition, other: DockPosition) -> DockPosition {
    use DockPosition::*;
    match (from, to) {
        _ if other != to => other,
        (Left | Right, _) => from,
        (Bottom, Left) => Right,
        (Bottom, Right | Bottom) => Left,
    }
}

/// A position's name in a saved layout.
pub(crate) fn position_name(position: DockPosition) -> &'static str {
    match position {
        DockPosition::Left => "left",
        DockPosition::Right => "right",
        DockPosition::Bottom => "bottom",
    }
}

/// The position named `name` in a saved layout.
pub(crate) fn position_named(name: &str) -> Option<DockPosition> {
    match name {
        "left" => Some(DockPosition::Left),
        "right" => Some(DockPosition::Right),
        "bottom" => Some(DockPosition::Bottom),
        _ => None,
    }
}
