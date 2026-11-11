//! Indent guides drawn in a headless window: one thin rect per run of each
//! guide's display rows, every 4 columns, broken by a display row whose code
//! starts left of it, in the active colour for the caret's block, and none
//! with `editor.indent_guides` off.

use fe_buffer::Buffer;
use fe_editor::{EditorModel, IndentStyle};
use gpui::{Bounds, Entity, TestAppContext, VisualTestContext, px};

use super::*;
use crate::{EditorSettings, EditorTheme, EditorView, init};

/// A nested block with a blank row inside it, indented by four spaces.
const SAMPLE: &str = "fn main() {\n    if x {\n        a();\n\n        b();\n    }\n}\n";

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

/// The last frame's guides as laid out, after checking it painted just
/// those.
fn guides(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Placed> {
    let placed = view.read_with(cx, |view, _| {
        view.layout.as_ref().expect("drawn").indent_guides.clone()
    });
    assert_eq!(PAINTED.with(|painted| painted.borrow().clone()), placed);
    placed
}

/// The bounds a guide at `column` over the buffer rows `rows` would have,
/// one per run of display rows, in the last frame.
fn expected(
    view: &Entity<EditorView>,
    column: u32,
    rows: std::ops::Range<u32>,
    cx: &mut VisualTestContext,
) -> Bounds<Pixels> {
    let scale = cx.update(|window, _| window.scale_factor());
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().expect("drawn");
        let space = f64::from(layout.glyph_widths.char_width(' '));
        let frame = &layout.frame;
        let shown: Vec<_> = (frame.rows.iter())
            .filter(|row| rows.contains(&row.buffer_row))
            .collect();
        assert!(!shown.is_empty());
        let rect = Rect {
            x: frame.view_x(f64::from(column) * space),
            y: frame.row_top(shown[0].row),
            width: (1. * scale).round().max(1.) / scale,
            height: shown.len() as f32 * frame.line_height,
        };
        layout.bounds(rect)
    })
}

/// The guides `fe_editor` computes over the whole text, as (column, rows).
fn computed(
    view: &Entity<EditorView>,
    width: u32,
    cx: &mut VisualTestContext,
) -> Vec<(u32, std::ops::Range<u32>)> {
    view.read_with(cx, |view, _| {
        let text = view.model().text();
        let tab = view.model().tab_size() as u32;
        (indent_guides(text, 0..text.row_count(), width, tab).into_iter())
            .map(|guide| (guide.column, guide.rows))
            .collect()
    })
}

fn bounds(guides: &[Placed]) -> Vec<Bounds<Pixels>> {
    guides.iter().map(|guide| guide.bounds).collect()
}

#[gpui::test]
fn four_spaces_get_a_guide_at_each_level_through_blank_rows(cx: &mut TestAppContext) {
    let (view, cx) = editor(
        SAMPLE,
        |model| model.set_indent_style(Some(IndentStyle::Spaces(4))),
        cx,
    );
    let computed = computed(&view, 4, cx);
    // Row 3 is blank, inside the `if`.
    assert_eq!(computed, [(0, 1..6), (4, 2..5)]);
    let want: Vec<_> = (computed.into_iter())
        .map(|(column, rows)| expected(&view, column, rows, cx))
        .collect();
    let placed = guides(&view, cx);
    assert_eq!(bounds(&placed), want);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    assert!(theme.indent_guides);
    // The caret is on row 0, whose next row opens the outer block.
    assert_eq!(placed[0].color, theme.indent_guide_active);
    assert_eq!(placed[1].color, theme.indent_guide);
}

#[gpui::test]
fn two_spaces_still_get_a_guide_every_four_columns(cx: &mut TestAppContext) {
    let (view, cx) = editor(
        "a:\n  b:\n    c:\n      d",
        |model| model.set_indent_style(Some(IndentStyle::Spaces(2))),
        cx,
    );
    let computed = computed(&view, 4, cx);
    assert_eq!(computed, [(0, 1..4), (4, 3..4)]);
    let want: Vec<_> = (computed.into_iter())
        .map(|(column, rows)| expected(&view, column, rows, cx))
        .collect();
    assert_eq!(bounds(&guides(&view, cx)), want);
}

#[gpui::test]
fn tabs_get_a_guide_every_tab_size(cx: &mut TestAppContext) {
    let (view, cx) = editor(
        "fn f() {\n\tif x {\n\t\ty();\n\t}\n}\n",
        |model| {
            model.set_indent_style(Some(IndentStyle::Tabs));
            model.set_tab_size(4);
        },
        cx,
    );
    let computed = computed(&view, 4, cx);
    assert_eq!(computed, [(0, 1..4), (4, 2..3)]);
    let want: Vec<_> = (computed.into_iter())
        .map(|(column, rows)| expected(&view, column, rows, cx))
        .collect();
    assert_eq!(bounds(&guides(&view, cx)), want);
}

#[gpui::test]
fn a_wrapped_row_s_continuation_breaks_the_guide(cx: &mut TestAppContext) {
    // Row 1's continuations start at column 0, with code: the guide at
    // column 0 stops above them and goes on below.
    let text = format!("fn f() {{\n    {}\n    x\n}}\n", "word ".repeat(80));
    let (view, cx) = editor(
        &text,
        |model| {
            model.set_indent_style(Some(IndentStyle::Spaces(4)));
            model.set_soft_wrap(true);
        },
        cx,
    );
    draw(cx);
    let (pieces, line_height) = view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        let pieces = (frame.rows.iter())
            .filter(|row| row.buffer_row == 1)
            .count();
        (pieces, frame.line_height)
    });
    assert!(pieces > 1, "row 1 wraps: {pieces} pieces");
    let placed = guides(&view, cx);
    let first = expected(&view, 0, 1..2, cx);
    let row_2 = expected(&view, 0, 2..3, cx);
    let one_row = |bounds: Bounds<Pixels>| Bounds {
        size: gpui::size(bounds.size.width, px(line_height)),
        ..bounds
    };
    assert_eq!(bounds(&placed), [one_row(first), row_2]);
    assert_eq!(row_2.top() - first.top(), px(pieces as f32 * line_height));
}

#[gpui::test]
fn the_caret_s_block_is_active_and_the_setting_turns_guides_off(cx: &mut TestAppContext) {
    let (view, cx) = editor(
        SAMPLE,
        |model| model.set_indent_style(Some(IndentStyle::Spaces(4))),
        cx,
    );
    // On `if x {`, which opens the block whose guide starts on row 2.
    view.update(cx, |view, cx| {
        let start = view.model().text().row_start(1);
        view.model_mut().set_cursor(start + 4);
        cx.notify();
    });
    draw(cx);
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    let placed = guides(&view, cx);
    assert_eq!(
        bounds(&placed),
        [expected(&view, 0, 1..6, cx), expected(&view, 4, 2..5, cx)]
    );
    let colors: Vec<_> = placed.iter().map(|guide| guide.color).collect();
    assert_eq!(colors, [theme.indent_guide, theme.indent_guide_active]);
    assert_ne!(theme.indent_guide, theme.indent_guide_active);

    let set = |on: bool, cx: &mut VisualTestContext| {
        let settings = EditorSettings {
            indent_guides: on,
            ..EditorSettings::default()
        };
        view.update(cx, |view, cx| view.apply_settings(&settings, cx));
        draw(cx);
    };
    set(false, cx);
    assert!(!view.read_with(cx, |view, _| view.theme.indent_guides));
    assert_eq!(guides(&view, cx), []);

    set(true, cx);
    assert_eq!(guides(&view, cx), placed);
}
