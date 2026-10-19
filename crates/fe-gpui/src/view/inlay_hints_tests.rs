//! Inlay hints in a headless window: chips painted inside the text after
//! their offsets, following edits, and the caret and clicks stepping around
//! them.

use std::rc::Rc;

use fe_buffer::{Buffer, Origin};
use fe_command::Invocation;
use fe_editor::EditorModel;
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, Pixels, TestAppContext, VisualTestContext, point, px,
};

use crate::{commands::RunCommand, inlay_hints::InlayHint, *};

const TEXT: &str = "let a = f(1);\n";

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
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

fn hint(offset: usize, label: &str, left: bool, right: bool) -> InlayHint {
    InlayHint {
        offset,
        label: label.to_string(),
        kind: None,
        padding_left: left,
        padding_right: right,
        data: Some(Rc::new(offset)),
    }
}

/// `: u8` after `a` and `x:` before `1`, given at the buffer's version.
fn set_hints(view: &Entity<EditorView>, cx: &mut VisualTestContext) {
    view.update(cx, |view, _| {
        let version = view.model.snapshot().version();
        view.set_inlay_hints(
            version,
            vec![hint(5, ": u8", true, false), hint(10, "x:", false, true)],
        );
    });
    draw(cx);
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(String, Bounds<Pixels>)> {
    view.read_with(cx, |view, _| view.painted_inlay_hints.clone())
}

fn caret_x(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Pixels {
    view.update(cx, |view, _| {
        view.layout
            .as_mut()
            .unwrap()
            .caret_bounds(offset)
            .unwrap()
            .left()
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

fn labels(painted: &[(String, Bounds<Pixels>)]) -> Vec<&str> {
    painted.iter().map(|(text, _)| text.as_str()).collect()
}

#[gpui::test]
fn hints_paint_after_their_offset_and_follow_edits(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    assert!(painted(&view, cx).is_empty());
    let (a, one) = (caret_x(&view, 5, cx), caret_x(&view, 10, cx));

    set_hints(&view, cx);
    let shown = painted(&view, cx);
    assert_eq!(labels(&shown), [" : u8", "x: "]);
    assert_near(shown[0].1.left(), a);
    assert_near(shown[1].1.left(), one + shown[0].1.size.width);
    assert!(caret_x(&view, 8, cx) >= shown[0].1.right());
    assert_eq!(
        view.read_with(cx, |view, _| view.inlay_hints()),
        [(5, ": u8".to_string()), (10, "x:".to_string())]
    );

    // Typing before them moves them with their text.
    edit(&view, 0..0, "bc", cx);
    let moved = painted(&view, cx);
    assert_eq!(labels(&moved), [" : u8", "x: "]);
    let delta = caret_x(&view, 2, cx) - caret_x(&view, 0, cx);
    assert_near(moved[0].1.left(), shown[0].1.left() + delta);
    assert_near(moved[1].1.left(), shown[1].1.left() + delta);
    assert_eq!(
        view.read_with(cx, |view, _| view.inlay_hints()),
        [(7, ": u8".to_string()), (12, "x:".to_string())]
    );

    // Deleting `a =`, strictly around the first hint's offset, drops it only.
    edit(&view, 6..9, "", cx);
    let rest = painted(&view, cx);
    assert_eq!(labels(&rest), ["x: "]);
    assert_eq!(
        view.read_with(cx, |view, _| view.inlay_hints()),
        [(9, "x:".to_string())]
    );
    assert!(rest[0].1.left() < moved[1].1.left());

    // No hints, no chips.
    view.update(cx, |view, _| view.clear_inlay_hints());
    draw(cx);
    assert!(painted(&view, cx).is_empty());

    // A version the buffer never had is ignored.
    view.update(cx, |view, _| {
        let mut other = Buffer::new("");
        let version = {
            for _ in 0..50 {
                other.edit([(0..0, "x".to_string())], Origin::User);
            }
            other.version()
        };
        view.set_inlay_hints(version, vec![hint(1, "t", false, false)]);
    });
    draw(cx);
    assert!(painted(&view, cx).is_empty());
}

#[gpui::test]
fn clicks_and_arrows_skip_hints(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let offsets_without = walk(&view, cx);
    set_hints(&view, cx);
    let shown = painted(&view, cx);

    // A click inside a hint puts the caret at its offset.
    for (ix, offset) in [5, 10].into_iter().enumerate() {
        let bounds = shown[ix].1;
        cx.simulate_click(
            point(bounds.center().x, bounds.center().y),
            Modifiers::none(),
        );
        let head = view.read_with(cx, |view, _| view.model.primary_selection().head());
        assert_eq!(head, offset);
    }

    assert_eq!(walk(&view, cx), offsets_without);
}

/// The offsets `editor.move_right` visits across the row from its start,
/// checking that each painted caret is outside every hint.
fn walk(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<usize> {
    view.update(cx, |view, _| view.model.set_cursor(0));
    draw(cx);
    let mut visited = Vec::new();
    for _ in 0..TEXT.len() {
        let head = view.read_with(cx, |view, _| view.model.primary_selection().head());
        visited.push(head);
        let (carets, hints) = view.read_with(cx, |view, _| {
            (
                view.painted_carets.clone(),
                view.painted_inlay_hints.clone(),
            )
        });
        assert!(!carets.is_empty(), "no caret painted at {head}");
        for caret in &carets {
            for (_, hint) in &hints {
                assert!(
                    caret.left() <= hint.left() || caret.left() >= hint.right(),
                    "caret {caret:?} inside {hint:?} at {head}"
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

/// A box selection from inside a tab keeps the pointer's column on the rows
/// below (T45 F1), with and without hints in the way.
#[gpui::test]
fn box_anchor_inside_a_tab_keeps_its_column(cx: &mut TestAppContext) {
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new("\tab\nabcdefgh\n"), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    draw(cx);
    let bounds = |offset: usize, cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            view.layout.as_mut().unwrap().caret_bounds(offset).unwrap()
        })
    };
    let (tab, after) = (bounds(0, cx), bounds(1, cx));
    let space = (after.left() - tab.left()) / 4.;
    let anchor = point(tab.left() + space * 2. + px(1.), tab.center().y);
    let row_1 = bounds(4, cx);
    let head = point(row_1.left() + space * 5. + px(1.), row_1.center().y);
    let modifiers = Modifiers {
        alt: true,
        shift: true,
        ..Modifiers::none()
    };
    cx.simulate_mouse_down(anchor, gpui::MouseButton::Left, modifiers);
    cx.simulate_mouse_move(head, Some(gpui::MouseButton::Left), modifiers);
    cx.simulate_mouse_up(head, gpui::MouseButton::Left, modifiers);
    let selected: Vec<_> = view.read_with(cx, |view, _| {
        (view.model.selections().iter())
            .map(|selection| selection.range.clone())
            .collect()
    });
    assert!(selected.contains(&(6..9)), "{selected:?}");
}

/// Hints from a version the buffer's log has compacted away are dropped,
/// not followed (T169 F1).
#[gpui::test]
fn hints_from_a_compacted_version_are_dropped(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    set_hints(&view, cx);
    // Edited and compacted with no frame between.
    view.update(cx, |view, _| {
        let mut buffer = view.model_mut().buffer_mut();
        buffer.edit([(0..0, "x".to_string())], Origin::User);
        drop(buffer);
        // The caret's own anchors rebased, as a host compacting would.
        view.model.set_cursor(0);
        let now = view.model.snapshot().version();
        let mut buffer = view.model_mut().buffer_mut();
        buffer.compact_log(now);
        buffer.edit([(0..0, "y".to_string())], Origin::User);
        drop(buffer);
        view.model.sync_display();
    });
    assert!(view.read_with(cx, |view, _| view.inlay_hints()).is_empty());
    draw(cx);
    assert!(painted(&view, cx).is_empty());
}
