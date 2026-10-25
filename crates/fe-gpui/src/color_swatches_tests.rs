//! Colour swatches in a headless window: a gap before each offset with a
//! square painted in it, beside inlay hints, following edits, the caret
//! stepping around the gap, and wrapped rows.

use fe_buffer::{Buffer, Origin};
use fe_command::Invocation;
use fe_editor::EditorModel;
use gpui::{
    Bounds, Entity, Focusable as _, Hsla, Modifiers, Pixels, Rgba, TestAppContext,
    VisualTestContext, point, px,
};

use super::{ColorSwatch, Painted};
use crate::{commands::RunCommand, inlay_hints::InlayHint, *};

const TEXT: &str = "a { color: #ff0000; }\n";
const RED: [f32; 4] = [1., 0., 0., 1.];

fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    draw(cx);
    (view, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Painted> {
    view.read_with(cx, |view, _| view.painted_swatches.clone())
}

fn set(view: &Entity<EditorView>, swatches: Vec<ColorSwatch>, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| view.set_color_swatches(swatches));
    draw(cx);
}

fn swatch(offset: usize, rgba: [f32; 4]) -> ColorSwatch {
    ColorSwatch { offset, rgba }
}

fn caret(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().caret_bounds(offset).unwrap()
    })
}

fn edit(
    view: &Entity<EditorView>,
    range: std::ops::Range<usize>,
    new: &str,
    cx: &mut VisualTestContext,
) {
    view.update(cx, |view, cx| {
        let mut buffer = view.model_mut().buffer_mut();
        buffer.edit([(range, new.to_string())], Origin::User);
        drop(buffer);
        cx.notify();
    });
    draw(cx);
}

/// Equal to within a hair of a pixel: boxes are sums of float widths.
fn assert_near(left: Pixels, right: Pixels) {
    assert!((left - right).abs() < px(0.01), "{left:?} != {right:?}");
}

fn hsla(rgba: [f32; 4]) -> Hsla {
    let [r, g, b, a] = rgba;
    Rgba { r, g, b, a }.into()
}

fn contains(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
    inner.left() >= outer.left()
        && inner.right() <= outer.right()
        && inner.top() >= outer.top()
        && inner.bottom() <= outer.bottom()
}

#[gpui::test]
fn a_swatch_reserves_a_gap_and_paints_in_it(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, TEXT);
    let hash = TEXT.find('#').unwrap();
    let before = caret(&view, hash, cx);
    let em = caret(&view, 1, cx).left() - caret(&view, 0, cx).left();
    let after = caret(&view, hash + 1, cx).left();
    assert!(painted(&view, cx).is_empty());

    set(&view, vec![swatch(hash, RED)], cx);
    let shown = painted(&view, cx);
    assert_eq!(shown.len(), 1);
    let shown = &shown[0];
    // One em, where the colour's text was, which moves an em right.
    assert_near(shown.gap.left(), before.left());
    assert_near(shown.gap.size.width, em);
    assert_near(caret(&view, hash + 1, cx).left(), after + em);
    assert_eq!(shown.gap.top(), before.top());
    // A square 0.85 em on a side, in the colour, bordered, centred in the
    // gap.
    assert!(contains(shown.gap, shown.square), "{shown:?}");
    assert_near(shown.square.size.width, (em * 0.85).round());
    assert_eq!(shown.square.size.width, shown.square.size.height);
    let across = shown.square.left() - shown.gap.left();
    assert_near(across, shown.gap.right() - shown.square.right());
    assert_eq!(shown.color, hsla(RED));
    let theme = view.read_with(cx, |view, _| view.theme.clone());
    assert_eq!(shown.border, theme.gutter_foreground);
    assert!(shown.checker.is_empty(), "an opaque colour has none");
    assert_eq!(
        view.read_with(cx, |view, _| view.color_swatches()),
        [swatch(hash, RED)]
    );

    // None set, none painted, and the text is back where it was.
    set(&view, Vec::new(), cx);
    assert!(painted(&view, cx).is_empty());
    assert_near(caret(&view, hash + 1, cx).left(), after);
}

#[gpui::test]
fn swatches_and_inlay_hints_coexist(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, TEXT);
    let hash = TEXT.find('#').unwrap();
    let colon = TEXT.find(':').unwrap();
    let hint = |offset: usize, label: &str| InlayHint {
        offset,
        label: label.to_string(),
        kind: None,
        padding_left: false,
        padding_right: false,
        data: None,
        tooltip: None,
        edits: Vec::new(),
    };
    set(&view, vec![swatch(hash, RED)], cx);
    // Hints set after the swatches, one at the swatch's offset, leave them.
    view.update(cx, |view, _| {
        let version = view.model.snapshot().version();
        view.set_inlay_hints(version, vec![hint(colon, "x"), hint(hash, "c:")]);
    });
    draw(cx);
    let (swatches, hints) = view.read_with(cx, |view, _| {
        (
            view.painted_swatches.clone(),
            view.painted_inlay_hints.clone(),
        )
    });
    assert_eq!(swatches.len(), 1);
    assert_eq!(hints.len(), 2);
    // At one offset the hint comes first, then the swatch, then the text.
    assert_near(swatches[0].gap.left(), hints[1].1.right());
    let em = caret(&view, 1, cx).left() - caret(&view, 0, cx).left();
    let past_hash = caret(&view, hash + 1, cx).left();
    assert_near(past_hash - swatches[0].gap.right(), em);
    assert!(swatches[0].gap.left() > hints[0].1.right());

    // Swatches set again leave the hints, and clearing the hints leaves the
    // swatches.
    let other = [0., 0.5, 1., 1.];
    set(&view, vec![swatch(hash, other), swatch(0, RED)], cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.inlay_hints().len()),
        2,
        "hints kept"
    );
    view.update(cx, |view, _| view.clear_inlay_hints());
    draw(cx);
    let swatches = painted(&view, cx);
    assert_eq!(swatches.len(), 2);
    assert_eq!(swatches[0].color, hsla(RED));
    assert_eq!(swatches[1].color, hsla(other));
    assert!(view.read_with(cx, |view, _| view.painted_inlay_hints.is_empty()));
}

#[gpui::test]
fn swatches_follow_edits(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "#fff #000\n");
    let black = [0., 0., 0., 1.];
    set(&view, vec![swatch(0, [1.; 4]), swatch(5, black)], cx);
    let shown = painted(&view, cx);
    assert_eq!(shown.len(), 2);

    // Typing at a swatch's offset puts the text before it: the swatch stays
    // with its colour.
    edit(&view, 0..0, "a ", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.color_swatches()),
        [swatch(2, [1.; 4]), swatch(7, black)]
    );
    let moved = painted(&view, cx);
    let em = caret(&view, 1, cx).left() - caret(&view, 0, cx).left();
    assert_near(moved[0].gap.left(), shown[0].gap.left() + em * 2.);
    assert_near(moved[1].gap.left(), shown[1].gap.left() + em * 2.);
    assert_near(moved[0].gap.left(), caret(&view, 2, cx).left());

    // Deleting text strictly around the second's offset drops it only.
    edit(&view, 5..9, "", cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.color_swatches()),
        [swatch(2, [1.; 4])]
    );
    assert_eq!(painted(&view, cx).len(), 1);
}

#[gpui::test]
fn swatch_alpha_paints_over_the_checkerboard(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, TEXT);
    let hash = TEXT.find('#').unwrap();
    let half = [0., 0., 1., 0.5];
    set(&view, vec![swatch(hash, half)], cx);
    let shown = painted(&view, cx);
    assert_eq!(shown.len(), 1);
    let shown = &shown[0];
    // The colour keeps its alpha, over a checkerboard of two greys filling
    // the square: the light under all of it, the dark in two quarters.
    assert_eq!(shown.color, hsla(half));
    assert!((shown.color.a - 0.5).abs() < 1e-6);
    let checker = &shown.checker;
    assert_eq!(checker.len(), 3, "{checker:?}");
    assert_eq!(checker[0].0, shown.square);
    let (light, dark) = (checker[0].1, checker[1].1);
    assert_eq!(checker[2].1, dark);
    assert_ne!(light, dark);
    assert_eq!((light.a, dark.a), (1., 1.), "opaque greys");
    assert!(
        light.s < 0.2 && dark.s < 0.2,
        "{light:?} {dark:?} are greys"
    );
    for (cell, _) in &checker[1..] {
        assert!(contains(shown.square, *cell));
        assert_near(cell.size.width, shown.square.size.width / 2.);
    }
    assert_ne!(checker[1].0.origin, checker[2].0.origin);
    // The same greys on the light theme, from its own colours.
    view.update(cx, |view, cx| {
        view.set_colors(EditorPalette::fastedit_light(), cx)
    });
    draw(cx);
    let light_theme = painted(&view, cx);
    assert_eq!(light_theme[0].checker.len(), 3);
    assert_ne!(light_theme[0].checker[1].1, dark);
    assert_ne!(light_theme[0].border, shown.border);
}

#[gpui::test]
fn the_caret_skips_the_gap(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, TEXT);
    let hash = TEXT.find('#').unwrap();
    let without = walk(&view, cx);
    set(&view, vec![swatch(hash, RED), swatch(0, RED)], cx);
    let shown = painted(&view, cx);
    assert_eq!(shown.len(), 2);

    // A click inside a gap puts the caret at its offset.
    for (ix, offset) in [0, hash].into_iter().enumerate() {
        let gap = shown[ix].gap;
        cx.simulate_click(point(gap.center().x, gap.center().y), Modifiers::none());
        let head = view.read_with(cx, |view, _| view.model.primary_selection().head());
        assert_eq!(head, offset);
    }
    assert_eq!(walk(&view, cx), without);
}

/// The offsets `editor.move_right` visits across the row from its start,
/// checking that each painted caret is outside every gap.
fn walk(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<usize> {
    view.update(cx, |view, _| view.model.set_cursor(0));
    draw(cx);
    let mut visited = Vec::new();
    for _ in 0..TEXT.len() {
        let head = view.read_with(cx, |view, _| view.model.primary_selection().head());
        visited.push(head);
        let (carets, swatches) = view.read_with(cx, |view, _| {
            (view.painted_carets.clone(), view.painted_swatches.clone())
        });
        assert!(!carets.is_empty(), "no caret painted at {head}");
        for caret in &carets {
            for swatch in &swatches {
                let gap = swatch.gap;
                assert!(
                    caret.left() <= gap.left() || caret.left() >= gap.right(),
                    "caret {caret:?} inside {gap:?} at {head}"
                );
            }
        }
        let invocation = Invocation::of("editor.move_right").unwrap();
        cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
        draw(cx);
        let next = view.read_with(cx, |view, _| view.model.primary_selection().head());
        if next == head {
            break;
        }
    }
    visited
}

#[gpui::test]
fn wrapped_rows_keep_swatches(cx: &mut TestAppContext) {
    // One buffer row of colours, wrapped into several screen rows.
    let text = "#abcdef ".repeat(40) + "\n";
    let (view, cx) = editor(cx, &text);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(true));
    draw(cx);
    let em = caret(&view, 1, cx).left() - caret(&view, 0, cx).left();
    let text_left = view.read_with(cx, |view, _| view.layout.as_ref().unwrap().frame.text_left);
    let height = cx.update(|window, _| window.viewport_size().height);
    cx.simulate_resize(gpui::size(px(text_left) + em * 40., height));
    draw(cx);
    let rows = |view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        let mut tops: Vec<Pixels> = (0..text.len())
            .map(|offset| caret(view, offset, cx).top())
            .collect();
        tops.dedup();
        tops.len()
    };
    let wrapped = rows(&view, cx);
    assert!(wrapped > 2, "{wrapped} rows");

    let offsets: Vec<usize> = (0..40).map(|ix| ix * 8).collect();
    set(
        &view,
        offsets.iter().map(|&offset| swatch(offset, RED)).collect(),
        cx,
    );
    let shown = painted(&view, cx);
    assert_eq!(shown.len(), offsets.len());
    // Each in the row its colour is now in, just before it.
    let mut tops = Vec::new();
    for (swatch, &offset) in shown.iter().zip(&offsets) {
        let colour = caret(&view, offset + 1, cx);
        assert_eq!(swatch.gap.top(), colour.top(), "at {offset}");
        assert!(
            swatch.gap.right() <= colour.left() + px(0.01),
            "at {offset}"
        );
        assert!(contains(swatch.gap, swatch.square));
        tops.push(swatch.gap.top());
    }
    tops.dedup();
    assert!(tops.len() > 2, "swatches on {} rows", tops.len());
    // Still wrapped, the buffer row whole.
    assert!(rows(&view, cx) >= wrapped);
    let text_now = view.read_with(cx, |view, _| view.model.text().to_string());
    assert_eq!(text_now, text);
}

/// With the gutter's annotation column (blame) shown, swatches still take
/// their gaps and paint (review px81xb F1).
#[gpui::test]
fn swatches_show_beside_the_annotation_column(cx: &mut TestAppContext) {
    cx.update(init);
    let (view, cx) = editor(cx, TEXT);
    let hash = TEXT.find('#').unwrap();
    view.update(cx, |view, cx| {
        let snapshot = view.model().snapshot().clone();
        let entry = GutterAnnotation {
            rows: snapshot.anchor_before(0)..snapshot.anchor_before(TEXT.len()),
            text: "2026-10-11 Ann Example".into(),
            heat: None,
        };
        let annotations = GutterAnnotations {
            version: snapshot.version(),
            entries: vec![entry],
            hover: std::rc::Rc::new(|_, _| None),
            on_click: std::rc::Rc::new(|_, _, _, _| {}),
        };
        view.set_gutter_annotations(Some(annotations), cx);
    });
    draw(cx);
    let column = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.annotation_column.width
    });
    assert!(column > 0., "the annotation column shows");
    let em = caret(&view, 1, cx).left() - caret(&view, 0, cx).left();
    let (before, after) = (caret(&view, hash, cx), caret(&view, hash + 1, cx).left());

    set(&view, vec![swatch(hash, RED)], cx);
    let shown = painted(&view, cx);
    assert_eq!(shown.len(), 1);
    assert_near(shown[0].gap.left(), before.left());
    assert_near(shown[0].gap.size.width, em);
    assert!(contains(shown[0].gap, shown[0].square));
    assert_eq!(shown[0].color, hsla(RED));
    assert_near(caret(&view, hash + 1, cx).left(), after + em);
}
