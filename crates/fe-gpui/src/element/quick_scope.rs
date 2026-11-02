//! Quick-scope's underlines (`vim.quick_scope`), laid out on a frame and
//! painted: a bar under each char of the primary caret's row that `f` or `F`
//! reaches in one or two presses ([`fe_vim::quick_scope::targets`]), in the
//! theme's primary or secondary colour. Underlines alone, as recolouring a
//! glyph would shape the row again on every caret move.

use fe_config::settings::VimQuickScope;
use fe_display::layout::Rect;
use fe_vim::{Mode, VimState, quick_scope::targets};
use gpui::{Bounds, Hsla, Pixels, Window, fill};

use super::{FrameLayout, block_rect};
use crate::EditorView;

/// An underline's height.
const HEIGHT: f32 = 2.;

/// An underline under a target.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Mark {
    /// The buffer offset of the target's char.
    pub(crate) offset: usize,
    pub(crate) color: Hsla,
    /// The bar along the bottom of the char's cell, in window coordinates.
    pub(crate) bounds: Bounds<Pixels>,
}

/// Which ways to mark, backward and forward, if any: in normal or a visual
/// mode, the way of a pending find, else both with `always`.
fn directions(mode: VimQuickScope, vim: &VimState) -> Option<(bool, bool)> {
    if !matches!(vim.mode(), Mode::Normal | Mode::Visual(_)) {
        return None;
    }
    match (mode, vim.pending().find_direction()) {
        (VimQuickScope::Off, _) | (VimQuickScope::OnKeys, None) => None,
        (_, Some(forward)) => Some((!forward, forward)),
        (VimQuickScope::Always, None) => Some((true, true)),
    }
}

/// The underlines for the primary caret of a `focused` editor, in offset
/// order, on the display rows the frame shows and the view's width reaches.
/// None while a jump pending labels the text.
pub(super) fn layout(
    layout: &mut FrameLayout,
    view: &EditorView,
    mode: VimQuickScope,
    focused: bool,
) -> Vec<Mark> {
    let Some(vim) = view
        .vim
        .as_ref()
        .filter(|_| focused && layout.jump.is_none())
    else {
        return Vec::new();
    };
    let Some((backward, forward)) = directions(mode, vim) else {
        return Vec::new();
    };
    let model = &view.model;
    // A visual mode's block is the vim cursor, not the selection's head.
    let caret = (vim.visual_cursor(model)).unwrap_or_else(|| model.primary_selection().head());
    let text = model.text();
    let found = (backward
        .then(|| targets(text, caret, false))
        .into_iter()
        .flatten())
    .chain(
        forward
            .then(|| targets(text, caret, true))
            .into_iter()
            .flatten(),
    );
    let (primary, secondary) = (
        view.theme.quick_scope_primary,
        view.theme.quick_scope_secondary,
    );
    let mut marks = Vec::new();
    for target in found {
        let Some(cell) = block_rect(layout, text, target.offset) else {
            continue;
        };
        let visible = layout.frame.text_bounds();
        if cell.x + cell.width <= visible.x || cell.x >= visible.x + visible.width {
            continue;
        }
        let height = HEIGHT.min(cell.height);
        let bar = Rect {
            y: cell.y + cell.height - height,
            height,
            ..cell
        };
        marks.push(Mark {
            offset: target.offset,
            color: if target.primary { primary } else { secondary },
            bounds: layout.bounds(bar),
        });
    }
    marks
}

/// Paints the underlines.
pub(super) fn paint(marks: &[Mark], window: &mut Window) {
    for mark in marks {
        window.paint_quad(fill(mark.bounds, mark.color));
    }
    #[cfg(test)]
    PAINTED.with(|painted| *painted.borrow_mut() = marks.to_vec());
}

#[cfg(test)]
thread_local! {
    /// The underlines this thread's last frame painted.
    pub(crate) static PAINTED: std::cell::RefCell<Vec<Mark>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
#[path = "quick_scope_tests.rs"]
mod tests;
