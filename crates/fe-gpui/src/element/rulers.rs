//! Rulers (`editor.rulers`) laid out on a frame and painted: a thin
//! vertical line down the text area at each column, under the text, in the
//! theme's `ruler` colour, as VS Code's `editor.rulers`.

use fe_display::{CharWidth as _, layout::Rect};
use gpui::{Bounds, Pixels, Window, fill};

use super::FrameLayout;
use crate::EditorView;

/// A line at each of the setting's columns above 0, `column` monospace
/// advances right of column 0, that the view's width reaches, in column
/// order.
pub(super) fn layout(
    layout: &mut FrameLayout,
    view: &EditorView,
    window: &Window,
) -> Vec<Bounds<Pixels>> {
    let mut columns: Vec<i32> = (view.theme.rulers.iter())
        .copied()
        .filter(|&column| column > 0)
        .collect();
    columns.extend(
        (view.max_line_length)
            .and_then(|n| i32::try_from(n).ok())
            .filter(|&n| n >= 1),
    );
    if columns.is_empty() {
        return Vec::new();
    }
    columns.sort_unstable();
    columns.dedup();
    let space = f64::from(layout.glyph_widths.char_width(' '));
    let scale = window.scale_factor();
    let line = (1. * scale).round().max(1.) / scale;
    let frame = &layout.frame;
    let text = frame.text_bounds();
    (columns.into_iter())
        .map(|column| frame.view_x(f64::from(column) * space))
        .filter(|&x| text.x <= x && x < text.x + text.width)
        .map(|x| {
            layout.bounds(Rect {
                x,
                y: text.y,
                width: line,
                height: text.height,
            })
        })
        .collect()
}

/// Paints the rulers in `color`.
pub(super) fn paint(rulers: &[Bounds<Pixels>], color: gpui::Hsla, window: &mut Window) {
    for &ruler in rulers {
        window.paint_quad(fill(ruler, color));
    }
    PAINTED.with(|painted| {
        // Reuses the vector, so a frame allocates nothing for this.
        let mut painted = painted.borrow_mut();
        painted.clear();
        painted.extend_from_slice(rulers);
    });
}

thread_local! {
    // The rulers this thread's last frame painted.
    pub(crate) static PAINTED: std::cell::RefCell<Vec<Bounds<Pixels>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The x of each ruler this thread's last frame painted, for tests of
/// what a configuration draws.
pub(crate) fn painted_xs() -> Vec<f32> {
    PAINTED.with(|painted| painted.borrow().iter().map(|r| r.origin.x.into()).collect())
}

#[cfg(test)]
#[path = "rulers_tests.rs"]
mod tests;
