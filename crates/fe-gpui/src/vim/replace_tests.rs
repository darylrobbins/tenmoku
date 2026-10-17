//! Replace mode, the caret's shape in each mode, and the keys that work
//! through the reducer alone: `ctrl-a`, the fold keys and marks.

use fe_buffer::{Buffer, Point};
use fe_editor::EditorModel;
use gpui::{Bounds, Entity, Focusable as _, Pixels, TestAppContext, VisualTestContext, px, size};

use super::*;
use crate::{EditorTheme, EditorView, init};

/// A focused editor in a window of its own showing `text`, vim on, the
/// caret at `at`. [`init`], with the macOS keymap, has run.
fn vim_editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    at: (u32, u32),
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        let offset = model.text().point_to_offset(Point::new(at.0, at.1));
        model.set_cursor(offset);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    (view, cx)
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn mode(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<&'static str> {
    view.read_with(cx, |view, _| view.vim_mode())
}

/// The primary caret's head, as a row and a byte column.
fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let point = model
            .text()
            .offset_to_point(model.primary_selection().head());
        (point.row, point.column)
    })
}

/// Draws a frame, and returns the carets it painted.
fn draw(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    view.read_with(cx, |view, _| view.painted_carets.clone())
}

/// The bar caret at `offset` in the last frame.
fn bar_at(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().caret_bounds(offset).unwrap()
    })
}

/// The buffer rows the last frame showed.
fn shown_rows(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<u32> {
    view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        frame.rows.iter().map(|row| row.buffer_row).collect()
    })
}

#[gpui::test]
fn replace_mode_typing(cx: &mut TestAppContext) {
    cx.update(init);
    overwrite_typing(cx);
    virtual_tab(cx);
    caret_shapes(cx);
    increment(cx);
    folds(cx);
    mark_jump_scrolls(cx);
    mark_jump_scrolls_within_a_wrapped_row(cx);
}

/// `Tab` in `gR`, pressed or committed with text, types the spaces to the
/// next stop over the columns they take; in `R` it is as before.
fn virtual_tab(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "abcdefgh", (0, 0));
    cx.simulate_keystrokes("g shift-r tab");
    cx.simulate_input("x");
    cx.simulate_keystrokes("escape");
    assert_eq!(text(&view, cx), "    xfgh");
    cx.simulate_keystrokes("u g shift-r");
    cx.simulate_input("\t");
    cx.simulate_keystrokes("escape");
    assert_eq!(text(&view, cx), "    efgh");
}

/// `R` overwrites as typed, `<BS>` puts back, and `u` undoes it all.
fn overwrite_typing(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "abcd\nefgh", (0, 0));
    cx.simulate_keystrokes("shift-r");
    assert_eq!(mode(&view, cx), Some("replace"));
    cx.simulate_input("xy");
    assert_eq!(text(&view, cx), "xycd\nefgh");
    cx.simulate_keystrokes("backspace");
    assert_eq!(text(&view, cx), "xbcd\nefgh");
    assert_eq!(caret(&view, cx), (0, 1));
    // A line break overwrites nothing.
    cx.simulate_keystrokes("enter");
    cx.simulate_input("z");
    assert_eq!(text(&view, cx), "x\nzcd\nefgh");
    cx.simulate_keystrokes("escape");
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(caret(&view, cx), (1, 0));
    cx.simulate_keystrokes("u");
    assert_eq!(text(&view, cx), "abcd\nefgh");
    // `ctrl-[` leaves it too.
    cx.simulate_keystrokes("shift-r");
    cx.simulate_input("q");
    cx.simulate_keystrokes("ctrl-[");
    assert_eq!(mode(&view, cx), Some("normal"));
    assert_eq!(text(&view, cx), "qbcd\nefgh");
}

/// A block in normal mode, a bar in insert mode, and an underline, 2px
/// tall and one grapheme wide, in replace and operator-pending mode.
fn caret_shapes(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "abcd", (0, 0));
    draw(&view, cx);
    let (at_0, at_1) = (bar_at(&view, 0, cx), bar_at(&view, 1, cx));
    let width = at_1.origin.x - at_0.origin.x;
    assert!(width > px(2.));

    let block = draw(&view, cx);
    assert_eq!(block.len(), 1);
    assert_eq!(block[0].origin, at_0.origin);
    assert_eq!(block[0].size.width, width);
    assert_eq!(block[0].size.height, at_0.size.height);

    let underline = |cx: &mut VisualTestContext| {
        let painted = draw(&view, cx);
        assert_eq!(painted.len(), 1);
        let line = painted[0];
        assert_eq!(line.size.height, px(2.));
        assert_eq!(line.size.width, width);
        assert_eq!(line.origin.x, at_0.origin.x);
        assert_eq!(line.bottom(), at_0.bottom());
    };
    cx.simulate_keystrokes("shift-r");
    underline(cx);
    cx.simulate_keystrokes("escape");
    assert_eq!(draw(&view, cx), block);

    cx.simulate_keystrokes("d");
    assert_eq!(mode(&view, cx), Some("operator_pending"));
    underline(cx);
    cx.simulate_keystrokes("escape");

    cx.simulate_keystrokes("i");
    let bar = draw(&view, cx);
    assert_eq!(bar, [at_0]);
    assert_eq!(bar[0].size.width, px(2.));
}

/// `ctrl-a` reaches the reducer, where no `ctrl-` binding applies.
fn increment(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "x 9", (0, 0));
    cx.simulate_keystrokes("ctrl-a");
    assert_eq!(text(&view, cx), "x 10");
    cx.simulate_keystrokes("ctrl-x");
    assert_eq!(text(&view, cx), "x 9");
}

/// `za` folds the indented block under the caret, and the next frame
/// hides its rows and shows the header's chevron folded; `za` again shows
/// them.
fn folds(cx: &mut TestAppContext) {
    let (view, cx) = vim_editor(cx, "head\n    one\n    two\ntail", (0, 0));
    draw(&view, cx);
    assert_eq!(shown_rows(&view, cx), [0, 1, 2, 3]);
    cx.simulate_keystrokes("z a");
    draw(&view, cx);
    assert_eq!(shown_rows(&view, cx), [0, 3]);
    let chevrons = view.read_with(cx, |view, _| view.painted_chevrons.clone());
    assert_eq!(
        chevrons
            .iter()
            .map(|&(row, folded, _)| (row, folded))
            .collect::<Vec<_>>(),
        [(0, true)]
    );
    cx.simulate_keystrokes("z a");
    draw(&view, cx);
    assert_eq!(shown_rows(&view, cx), [0, 1, 2, 3]);
    cx.simulate_keystrokes("z c");
    draw(&view, cx);
    assert_eq!(shown_rows(&view, cx), [0, 3]);
    cx.simulate_keystrokes("z o");
    draw(&view, cx);
    assert_eq!(shown_rows(&view, cx), [0, 1, 2, 3]);
}

/// `'a` after `ma` and `G` brings the caret back and scrolls it into view.
fn mark_jump_scrolls(cx: &mut TestAppContext) {
    let rows: Vec<String> = (0..500).map(|row| format!("row {row}")).collect();
    let (view, cx) = vim_editor(cx, &rows.join("\n"), (3, 2));
    draw(&view, cx);
    // Ten rows in view.
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").frame.line_height
    });
    cx.simulate_resize(size(px(600.), px(line_height * 10.)));
    draw(&view, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.viewport_rows()),
        Some((0, 10))
    );
    cx.simulate_keystrokes("m a shift-g");
    draw(&view, cx);
    assert_eq!(caret(&view, cx).0, 499);
    // Whether row 3 is among the rows in view.
    let shows = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let (top, rows) = view.viewport_rows().expect("drawn");
            (top..top + rows).contains(&3)
        })
    };
    assert!(!shows(cx));
    cx.simulate_keystrokes("' a");
    assert_eq!(caret(&view, cx), (3, 0));
    draw(&view, cx);
    assert!(shows(cx));
}

/// `'a` back to the start of a soft-wrapped first row many screens long,
/// after `G` left the view partway down it, scrolls the caret into view.
fn mark_jump_scrolls_within_a_wrapped_row(cx: &mut TestAppContext) {
    let text = format!("{}\nend", "word ".repeat(3000));
    let (view, cx) = vim_editor(cx, &text, (0, 0));
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(true));
    draw(&view, cx);
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").frame.line_height
    });
    cx.simulate_resize(size(px(600.), px(line_height * 10.)));
    draw(&view, cx);
    // Whether the caret's display row is among the rows in view.
    let shows_caret = |cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let frame = &view.layout.as_ref().expect("drawn").frame;
            let model = view.model();
            let row = model
                .display()
                .row_for_offset(model.primary_selection().head());
            (frame.first_row..frame.first_row + frame.visible_rows()).contains(&row)
        })
    };
    assert!(shows_caret(cx));
    cx.simulate_keystrokes("m a shift-g");
    draw(&view, cx);
    assert_eq!(caret(&view, cx).0, 1);
    let first_row = view.read_with(cx, |view, _| view.layout.as_ref().unwrap().frame.first_row);
    assert!(first_row > 10, "the view starts partway down row 0");
    assert!(shows_caret(cx));
    cx.simulate_keystrokes("' a");
    assert_eq!(caret(&view, cx), (0, 0));
    draw(&view, cx);
    assert!(shows_caret(cx));

    // Several commands before a draw: a macro's `l`, which keeps the view
    // partway down row 0, then its `'a`.
    cx.simulate_keystrokes("q b l ' a q shift-g");
    draw(&view, cx);
    let first_row = view.read_with(cx, |view, _| view.layout.as_ref().unwrap().frame.first_row);
    assert!(first_row > 10, "the view starts partway down row 0");
    assert_eq!(caret(&view, cx).0, 1);
    cx.simulate_keystrokes("@ b");
    assert_eq!(caret(&view, cx), (0, 0));
    draw(&view, cx);
    assert!(shows_caret(cx));
}
