//! Whitespace marks drawn in a headless window: which spaces and tabs each
//! `editor.render_whitespace` mode marks, where, and only on the rows the
//! frame shows.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{Entity, TestAppContext, VisualTestContext};

use super::*;
use crate::{EditorSettings, EditorTheme, EditorView, init};

/// An editor on `text`, set up by `setup`, drawn.
fn editor<'a>(
    text: &str,
    setup: impl FnOnce(&mut EditorModel) + 'static,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let text = text.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        setup(&mut model);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Switches the mode, as a settings reload does, and draws.
fn set(mode: RenderWhitespace, view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    let settings = EditorSettings {
        render_whitespace: mode,
        ..EditorSettings::default()
    };
    view.update(cx, |view, cx| view.apply_settings(&settings, cx));
    draw(cx);
}

/// The last frame's marks as laid out, after checking it painted just
/// those.
fn marks(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Mark> {
    let placed = view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").whitespace.clone()
    });
    assert_eq!(PAINTED.with(|painted| painted.borrow().clone()), placed);
    placed
}

/// Where the char at `offset` is drawn in the last frame: from its start
/// to the next char's, on the display row holding it.
fn expected(
    view: &Entity<EditorView>,
    offset: usize,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().expect("drawn");
        let frame = &layout.frame;
        let row = (frame.rows.iter())
            .find(|row| row.start <= offset && offset < row.end)
            .expect("a shown row");
        let (long_rows, widths) = (&mut layout.long_rows, &mut layout.glyph_widths);
        let start = frame.view_x(row.x_after_hints(offset, long_rows, widths));
        let end = frame.view_x(row.x_for_offset(offset + 1, long_rows, widths));
        let rect = Rect {
            x: start,
            y: frame.row_top(row.row),
            width: end - start,
            height: frame.line_height,
        };
        layout.bounds(rect)
    })
}

fn offsets(marks: &[Mark]) -> Vec<usize> {
    marks.iter().map(|mark| mark.offset).collect()
}

#[gpui::test]
fn modes(cx: &mut TestAppContext) {
    // Row 0: `a`, two spaces, `b`, a tab, a space, `c`, three spaces.
    // Row 1, from offset 11: `x y  z`, a single space between words.
    let (view, cx) = editor(
        "a  b\t c   \nx y  z",
        |model| {
            model.set_tab_size(4);
            // Over `b\t`.
            model.select_range(3..5, false);
        },
        cx,
    );
    let space = view.update(cx, |view, _| {
        f32::from(view.layout.as_mut().unwrap().space_width())
    });
    let modes = [
        (RenderWhitespace::All, vec![1, 2, 4, 5, 7, 8, 9, 12, 14, 15]),
        // Only `x y`'s single space between words goes.
        (
            RenderWhitespace::Boundary,
            vec![1, 2, 4, 5, 7, 8, 9, 14, 15],
        ),
        (RenderWhitespace::Selection, vec![4]),
        (RenderWhitespace::Trailing, vec![7, 8, 9]),
        (RenderWhitespace::None, vec![]),
    ];
    for (mode, want) in modes {
        set(mode, &view, cx);
        assert_eq!(
            view.read_with(cx, |view, _| view.theme.render_whitespace),
            mode
        );
        let marks = marks(&view, cx);
        assert_eq!(offsets(&marks), want, "{mode:?}");
        for mark in &marks {
            let kind = match mark.offset {
                4 => Kind::Tab,
                _ => Kind::Space,
            };
            assert_eq!(mark.kind, kind, "{mode:?} at {}", mark.offset);
            assert_eq!(mark.bounds, expected(&view, mark.offset, cx), "{mode:?}");
            // A space is one advance; the tab, at column 4, reaches column 8.
            let width = f32::from(mark.bounds.size.width);
            let columns = if kind == Kind::Tab { 4. } else { 1. };
            assert!((width - columns * space).abs() < 0.01, "{width} vs {space}");
        }
    }
    // Row 1's marks are a row below row 0's.
    set(RenderWhitespace::All, &view, cx);
    let marks = marks(&view, cx);
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    });
    let (first, last) = (&marks[0], &marks[marks.len() - 1]);
    assert_eq!(
        last.bounds.origin.y - first.bounds.origin.y,
        gpui::px(line_height)
    );
    // The default is `selection`.
    assert_eq!(
        EditorSettings::default().render_whitespace,
        RenderWhitespace::Selection
    );
}

#[gpui::test]
fn wrap_and_fold(cx: &mut TestAppContext) {
    // Row 0 wraps; rows 2 to 4 fold under row 1; then far more rows than fit.
    let words = "word ".repeat(80);
    let tail = "p q\n".repeat(500);
    let text = format!("{words}\nfn f() {{\n  a b\n  c d\n  e f\n}}\n{tail}");
    let (view, cx) = editor(
        &text,
        |model| {
            model.set_soft_wrap(true);
            model.fold_rows(1..=4);
        },
        cx,
    );
    // Wrapping takes a frame to settle.
    draw(cx);
    set(RenderWhitespace::All, &view, cx);
    let marks = marks(&view, cx);
    let (pieces, shown, frame_end) = view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().unwrap().frame;
        let pieces = (frame.rows.iter())
            .filter(|row| row.buffer_row == 0)
            .count();
        let shown: Vec<u32> = frame.rows.iter().map(|row| row.buffer_row).collect();
        (pieces, shown, frame.rows.last().unwrap().end)
    });
    assert!(pieces > 1, "row 0 wraps: {pieces} pieces");
    assert!(!shown.contains(&2) && shown.contains(&5), "{shown:?}");

    // Every space of the wrapped row is marked, on the screen row of the
    // piece that holds it, and the pieces each get some.
    let on_row_0: Vec<&Mark> = marks
        .iter()
        .filter(|mark| mark.offset < words.len())
        .collect();
    assert_eq!(on_row_0.len(), 80);
    let mut tops = Vec::new();
    for mark in &marks {
        assert_eq!(
            mark.bounds,
            expected(&view, mark.offset, cx),
            "at {}",
            mark.offset
        );
        if mark.offset < words.len() && !tops.contains(&mark.bounds.origin.y) {
            tops.push(mark.bounds.origin.y);
        }
    }
    assert_eq!(tops.len(), pieces);

    // None in the folded rows, nor past the frame's last row.
    let (folded_start, folded_end) = view.read_with(cx, |view, _| {
        let text = view.model().text();
        (text.row_start(2), text.row_start(5))
    });
    assert!(
        marks
            .iter()
            .all(|mark| !(folded_start..folded_end).contains(&mark.offset))
    );
    assert!(marks.iter().all(|mark| mark.offset < frame_end));
    assert!(frame_end < text.len() - 100, "rows run past the frame");
    // The header row of the fold keeps its own.
    assert!(marks.iter().any(|mark| mark.offset == words.len() + 3));
}
