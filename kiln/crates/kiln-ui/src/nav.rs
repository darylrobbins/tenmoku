// SPDX-License-Identifier: LicenseRef-Proprietary

//! The keys [`crate::List`] and [`crate::Tree`] share: moving a selection
//! through rows, a page at a time, and confirming it.

use gpui::{KeyDownEvent, UniformListScrollHandle};

/// What a key asks a list of rows for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NavKey {
    /// Select this row.
    To(usize),
    /// Confirm the selected row.
    Confirm,
    /// `left`: collapse, or go to the parent, in a tree.
    Left,
    /// `right`: expand, or go to the first child, in a tree.
    Right,
}

/// What `event` asks of `count` rows with `selected` selected, `page`
/// rows to a screen; `None` for a key the rows don't take, or a key with
/// modifiers, which go on to the app's bindings.
pub(crate) fn nav_key(
    event: &KeyDownEvent,
    selected: Option<usize>,
    count: usize,
    page: usize,
) -> Option<NavKey> {
    if event.keystroke.modifiers.modified() {
        return None;
    }
    let last = count.checked_sub(1);
    let page = page.max(1);
    let to = |ix: Option<usize>| ix.map(NavKey::To);
    match event.keystroke.key.as_str() {
        "up" => to(match selected {
            Some(ix) => Some(ix.saturating_sub(1)),
            None => last,
        }),
        "down" => to(match selected {
            Some(ix) => last.map(|last| (ix + 1).min(last)),
            None => last.map(|_| 0),
        }),
        "home" => to(last.map(|_| 0)),
        "end" => to(last),
        "pageup" => to(last.map(|_| selected.unwrap_or(0).saturating_sub(page))),
        "pagedown" => to(last.map(|last| (selected.unwrap_or(0) + page).min(last))),
        "enter" => selected.map(|_| NavKey::Confirm),
        "left" => Some(NavKey::Left),
        "right" => Some(NavKey::Right),
        _ => None,
    }
}

/// How many rows of `count` the list behind `scroll` showed in its last
/// frame, or 10 before it's been drawn.
pub(crate) fn page_rows(scroll: &UniformListScrollHandle, count: usize) -> usize {
    let state = scroll.0.borrow();
    // `item` is the list's own size, `contents` all its rows'.
    match state.last_item_size {
        Some(size) if count > 0 && size.contents.height > gpui::px(0.) => {
            let row = size.contents.height / count as f32;
            ((size.item.height / row).floor() as usize).max(1)
        }
        _ => 10,
    }
}
