//! Indent guides with a large block folded: computed over the shown buffer
//! rows alone, and drawn as they are with the block unfolded.

use std::collections::BTreeSet;

use fe_buffer::Buffer;
use fe_editor::{EditorModel, IndentStyle};
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorTheme, EditorView, init};

const HIDDEN: u32 = 100_000;

/// A header, a block of `HIDDEN` rows, a blank row, a row after it, closers.
fn text() -> String {
    let block = "        b();\n".repeat(HIDDEN as usize);
    format!("fn main() {{\n    if x {{\n{block}\n        c();\n    }}\n}}\n")
}

fn folded(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let text = text();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_indent_style(Some(IndentStyle::Spaces(4)));
        model.fold_rows(1..=HIDDEN + 1);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    VISITED.with(|visited| visited.borrow_mut().clear());
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx)
}

#[gpui::test]
fn a_folded_block_is_not_visited(cx: &mut TestAppContext) {
    let (view, cx) = folded(cx);
    let shown: BTreeSet<u32> = view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        frame.rows.iter().map(|row| row.buffer_row).collect()
    });
    assert!(shown.contains(&1) && shown.contains(&(HIDDEN + 2)));
    assert!(!shown.contains(&2), "the block is folded");
    let visited = VISITED.with(|visited| visited.borrow().clone());
    assert!(visited.len() >= 2, "{visited:?}");
    let rows: u32 = visited.iter().map(|rows| rows.end - rows.start).sum();
    assert_eq!(rows as usize, shown.len(), "{visited:?}");
    for row in visited.iter().flat_map(|rows| rows.clone()) {
        assert!(shown.contains(&row), "visited hidden row {row}");
    }
}

#[gpui::test]
fn guides_around_a_fold_are_those_of_the_unfolded_text(cx: &mut TestAppContext) {
    let (view, cx) = folded(cx);
    let placed = view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").indent_guides.clone()
    });
    // Each shown row's guide cells, from the guides of the whole unfolded
    // text, as window (x, y).
    let (want, line_height) = view.update(cx, |view, _| {
        let text = view.model().text().clone();
        let layout = view.layout.as_mut().expect("drawn");
        let space = f64::from(layout.glyph_widths.char_width(' '));
        let guides = indent_guides(&text, 0..text.row_count(), 4, 4);
        let mut want = BTreeSet::new();
        for row in layout.frame.rows.clone() {
            for guide in guides.iter().filter(|g| g.rows.contains(&row.buffer_row)) {
                let frame = &layout.frame;
                let rect = Rect {
                    x: frame.view_x(f64::from(guide.column) * space),
                    y: frame.row_top(row.row),
                    width: 1.,
                    height: frame.line_height,
                };
                let origin = layout.bounds(rect).origin;
                want.insert((
                    f32::from(origin.x).round() as i64,
                    f32::from(origin.y).round() as i64,
                ));
            }
        }
        (want, layout.frame.line_height)
    });
    let mut have = BTreeSet::new();
    for guide in &placed {
        let origin = guide.bounds.origin;
        for i in 0..(f32::from(guide.bounds.size.height) / line_height).round() as i64 {
            let y = f32::from(origin.y) + i as f32 * line_height;
            have.insert((f32::from(origin.x).round() as i64, y.round() as i64));
        }
    }
    assert!(!want.is_empty());
    assert_eq!(have, want);
}
