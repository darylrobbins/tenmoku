//! The host's lightbulb in the gutter's mark slot: it takes its row's
//! diagnostic dot, follows edits, and calls back when clicked.

use std::{cell::Cell, rc::Rc};

use fe_buffer::Buffer;
use fe_editor::{
    EditorModel,
    diagnostics::{Diagnostic, DiagnosticTag, Severity},
};
use gpui::{
    Entity, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext,
    VisualTestContext, point,
};

use crate::{
    view::{GlyphKind, GutterGlyph},
    *,
};

const TEXT: &str = "aa\nbb\ncc\ndd\n";

/// A drawn editor on [`TEXT`] with an error on row 2 and the caret at the
/// start.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, _| window.activate_window());
    view.update(cx, |view, cx| {
        let version = view.model().snapshot().version();
        let at = TEXT.find("cc").unwrap();
        let items = vec![(at..at + 2, Diagnostic::new(Severity::Error, "bad"))];
        assert!(
            view.model_mut()
                .apply_diagnostics(1u32, (1, 1), version, items, Some((1, 1)))
        );
        cx.notify();
    });
    cx.run_until_parked();
    (view, cx)
}

fn glyph(row: u32) -> Option<GutterGlyph> {
    Some(GutterGlyph {
        row,
        kind: GlyphKind::Lightbulb,
    })
}

fn painted(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
) -> (Option<(u32, gpui::Bounds<Pixels>)>, Vec<u32>) {
    view.read_with(cx, |view, _| {
        let rows = view.painted_marks.iter().map(|&(row, _)| row).collect();
        (view.painted_glyph, rows)
    })
}

fn redraw(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
}

fn click(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    cx.run_until_parked();
}

#[gpui::test]
fn the_glyph_replaces_the_rows_dot_and_follows_edits(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    assert_eq!(painted(&view, cx), (None, vec![2]), "the dot, no glyph");

    view.update(cx, |view, _| view.set_gutter_glyph(glyph(2), None));
    redraw(&view, cx);
    let (shown, dots) = painted(&view, cx);
    let (row, bounds) = shown.expect("glyph painted");
    assert_eq!(row, 2);
    assert_in_slot(&view, bounds, cx);
    assert!(dots.is_empty(), "no dot under the glyph");

    // A row inserted above moves it, and the error, to row 3; one inserted
    // exactly at its row's start does too.
    for (insert_row, glyph_row) in [(0, 3), (3, 4)] {
        view.update(cx, |view, cx| {
            let at = view.model().text().row_start(insert_row);
            view.model_mut().set_cursor(at);
            view.model_mut().insert("new\n");
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |view, _| view.gutter_glyph()),
            glyph(glyph_row)
        );
    }
    let (shown, dots) = painted(&view, cx);
    assert_eq!(shown.map(|(row, _)| row), Some(4));
    assert!(dots.is_empty());

    view.update(cx, |view, _| view.set_gutter_glyph(None, None));
    redraw(&view, cx);
    assert_eq!(painted(&view, cx), (None, vec![4]), "the dot returns");

    // On row zero, a row inserted at the very start goes above it.
    view.update(cx, |view, cx| {
        view.set_gutter_glyph(glyph(0), None);
        view.model_mut().set_cursor(0);
        view.model_mut().insert("top\n");
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.gutter_glyph()), glyph(1));
}

/// A tall line height doesn't push the glyph out of the mark slot.
#[gpui::test]
fn a_tall_line_height_keeps_the_glyph_in_its_slot(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    view.update(cx, |view, _| {
        view.theme.line_height *= 3.;
        view.set_gutter_glyph(glyph(2), None);
    });
    redraw(&view, cx);
    let (_, bounds) = painted(&view, cx).0.expect("glyph painted");
    assert_in_slot(&view, bounds, cx);
}

/// Both of `bounds`' horizontal edges are in the 2 em left of the line
/// numbers, which start at the editor's left edge.
fn assert_in_slot(
    view: &Entity<EditorView>,
    bounds: gpui::Bounds<Pixels>,
    cx: &mut VisualTestContext,
) {
    let (left, em) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let rows = view.model().text().row_count();
        // The gutter is a whole number of ems wide.
        let em = (frame.text_left - frame.annotation_column.width)
            / fe_display::layout::gutter_width(rows, 1.);
        (
            layout.origin.x + gpui::px(frame.annotation_column.width),
            em,
        )
    });
    assert!(bounds.left() >= left, "{bounds:?} left of {left:?}");
    assert!(
        bounds.right() <= left + gpui::px(2. * em),
        "{bounds:?} past the slot's {em} em"
    );
}

#[gpui::test]
fn clicking_the_glyph_calls_back(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let calls = Rc::new(Cell::new(0));
    let counter = calls.clone();
    let view_again = view.clone();
    view.update(cx, |view, _| {
        view.set_gutter_glyph(
            glyph(2),
            Some(Rc::new(move |_, cx| {
                counter.set(counter.get() + 1);
                // The host may update the view from it.
                view_again.update(cx, |_, cx| cx.notify());
            })),
        )
    });
    redraw(&view, cx);
    let caret = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| view.model.primary_selection().range.clone())
    };
    let (_, bounds) = painted(&view, cx).0.expect("glyph painted");

    // Row 1's slot, a line above.
    click(
        point(
            bounds.center().x,
            bounds.center().y - bounds.size.height * 1.25,
        ),
        cx,
    );
    assert_eq!(calls.get(), 0);

    // The click elsewhere moved the caret; the glyph's leaves it.
    let before = caret(&view, cx);
    click(bounds.center(), cx);
    assert_eq!(calls.get(), 1);
    assert_eq!(caret(&view, cx), before);
}

/// A Deprecated range that starts before a scrolled long row's shaped slice
/// paints without a strike colour from that slice, instead of underflowing
/// looking it up.
#[gpui::test]
fn a_deprecated_range_before_the_shaped_slice_paints(cx: &mut TestAppContext) {
    let text = "x".repeat(100_000);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, _| window.activate_window());
    view.update(cx, |view, _| {
        let version = view.model().snapshot().version();
        let mut diagnostic = Diagnostic::new(Severity::Warning, "old");
        diagnostic.tags.push(DiagnosticTag::Deprecated);
        assert!(view.model_mut().apply_diagnostics(
            1u32,
            (1, 1),
            version,
            vec![(0..60_000, diagnostic)],
            Some((1, 1))
        ));
        view.scroll.x = gpui::px(25_000.);
        view.autoscroll = false;
    });
    redraw(&view, cx);
    let start = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.rows[0].shown().start
    });
    assert!(start > 0, "the slice starts after the range does");
}
