//! Input method composition ([`super::ime`]): composed text stays out of the
//! buffer, drawn as an underlined inlay at the caret, until it commits as
//! typing.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::{Buffer, Origin, ResolvedSelection};
use fe_command::{ContextFrame, Invocation, Value};
use fe_display::layout::{MAX_SHAPED_ROW_BYTES, MeasuredLine};
use fe_editor::EditorModel;
use gpui::{
    Bounds, Entity, EntityInputHandler as _, Focusable as _, Modifiers, Pixels, TestAppContext,
    VisualTestContext, point, px,
};

use crate::{
    commands::{RunCommand, observe_invocations},
    *,
};

/// A focused editor showing `text` with `selections`, and every invocation
/// run from then on.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
    selections: &[(usize, usize)],
) -> (
    Entity<EditorView>,
    &'a mut VisualTestContext,
    Rc<RefCell<Vec<Invocation>>>,
) {
    cx.update(init);
    let seen = Rc::new(RefCell::new(Vec::new()));
    cx.update(|cx| {
        let sink = seen.clone();
        observe_invocations(cx, move |invocation| {
            sink.borrow_mut().push(invocation.clone())
        });
    });
    let text = text.to_owned();
    let selections = selections.to_vec();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_selections(
            selections
                .into_iter()
                .map(|(start, end)| ResolvedSelection {
                    range: start..end,
                    reversed: false,
                }),
        );
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| {
        // Active, so losing focus is seen.
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx)
    });
    cx.run_until_parked();
    (view, cx, seen)
}

/// Runs `f` on the view, as the platform's input handler does.
fn ime<R>(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    f: impl FnOnce(&mut EditorView, &mut gpui::Window, &mut gpui::Context<EditorView>) -> R,
) -> R {
    cx.update(|window, cx| view.update(cx, |view, cx| f(view, window, cx)))
}

fn mark(view: &Entity<EditorView>, cx: &mut VisualTestContext, text: &str, selected: Range<usize>) {
    ime(view, cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, text, Some(selected), window, cx)
    });
}

fn marked(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<Range<usize>> {
    ime(view, cx, |view, window, cx| {
        view.marked_text_range(window, cx)
    })
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model().text().to_string())
}

fn heads(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<usize> {
    view.read_with(cx, |view, _| {
        (view.model().selections().iter())
            .map(|selection| selection.head())
            .collect()
    })
}

/// The buffer's version and the bytes of its undo history.
fn history(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (fe_buffer::Version, usize) {
    view.read_with(cx, |view, _| {
        let model = view.model();
        let bytes = model.buffer().history_bytes(&Origin::User);
        (model.snapshot().version(), bytes)
    })
}

fn inserts(seen: &Rc<RefCell<Vec<Invocation>>>) -> Vec<Value> {
    (seen.borrow().iter())
        .filter(|invocation| invocation.command.as_str() == "editor.insert")
        .map(|invocation| invocation.args.clone())
        .collect()
}

fn run(invocation: Invocation, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.dispatch_action(Box::new(RunCommand(invocation)), cx));
    cx.run_until_parked();
}

fn undo(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-z");
}

/// Whether two positions are the same, but for rounding.
#[track_caller]
fn near(a: Pixels, b: Pixels) {
    assert!((f32::from(a) - f32::from(b)).abs() < 0.01, "{a:?} != {b:?}");
}

fn draw(cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

#[gpui::test]
fn composing_leaves_the_buffer(cx: &mut TestAppContext) {
    let (view, cx, seen) = editor(cx, "ab", &[(2, 2)]);
    let events = view.update(cx, |view, _| view.model_mut().buffer_mut().subscribe());
    let before = history(&view, cx);

    // Typing "ka" in a Japanese input method composes "か".
    mark(&view, cx, "k", 1..1);
    assert_eq!(marked(&view, cx), Some(2..3));
    mark(&view, cx, "か", 0..1);
    assert_eq!(text(&view, cx), "ab");
    assert_eq!(marked(&view, cx), Some(2..3));
    ime(&view, cx, |view, window, cx| {
        // The input method sees the composition in the text.
        let mut actual = None;
        let composed = view.text_for_range(0..3, &mut actual, window, cx);
        assert_eq!(composed.as_deref(), Some("abか"));
        assert_eq!(actual, Some(0..3));
        let mut actual = None;
        let composed = view.text_for_range(1..3, &mut actual, window, cx);
        assert_eq!(composed.as_deref(), Some("bか"));
        // The selection is where the input method asked, relative to the
        // composition.
        let selection = view.selected_text_range(false, window, cx).unwrap();
        assert_eq!(selection.range, 2..3);
    });

    assert_eq!(history(&view, cx), before);
    assert!(events.drain().is_empty());
    assert!(inserts(&seen).is_empty());
    assert!(!view.read_with(cx, |view, _| view.model().is_dirty()));
}

#[gpui::test]
fn commit_types_once(cx: &mut TestAppContext) {
    // Choosing a candidate types it at every caret, once.
    let (view, cx, seen) = editor(cx, "a\nb", &[(1, 1), (3, 3)]);
    mark(&view, cx, "k", 1..1);
    mark(&view, cx, "か", 1..1);
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(None, "加", window, cx);
        assert_eq!(view.marked_text_range(window, cx), None);
    });
    assert_eq!(text(&view, cx), "a加\nb加");
    assert_eq!(heads(&view, cx), [4, 9]);
    assert_eq!(inserts(&seen), [Value::from_iter([("text", "加")])]);
    undo(cx);
    assert_eq!(text(&view, cx), "a\nb");
    assert_eq!(heads(&view, cx), [1, 3]);

    // Committing in place of the marked range does the same.
    mark(&view, cx, "か", 1..1);
    ime(&view, cx, |view, window, cx| {
        let marked = view.marked_text_range(window, cx);
        view.replace_text_in_range(marked, "加", window, cx)
    });
    assert_eq!(text(&view, cx), "a加\nb加");
    undo(cx);

    // Unmarking commits what's composed.
    seen.borrow_mut().clear();
    mark(&view, cx, "か", 1..1);
    ime(&view, cx, |view, window, cx| view.unmark_text(window, cx));
    assert_eq!(text(&view, cx), "aか\nbか");
    assert_eq!(marked(&view, cx), None);
    assert_eq!(inserts(&seen), [Value::from_iter([("text", "か")])]);
    undo(cx);
    assert_eq!(text(&view, cx), "a\nb");

    // An empty mark ends the composition, typing nothing.
    let before = history(&view, cx);
    mark(&view, cx, "か", 1..1);
    mark(&view, cx, "", 0..0);
    assert_eq!(marked(&view, cx), None);
    ime(&view, cx, |view, window, cx| view.unmark_text(window, cx));
    assert_eq!(text(&view, cx), "a\nb");
    assert_eq!(history(&view, cx), before);

    // Composing over a selection deletes it first.
    let (view, cx, seen) = editor(cx, "abcd", &[(1, 3)]);
    let before = history(&view, cx);
    mark(&view, cx, "か", 1..1);
    assert_eq!(text(&view, cx), "ad");
    assert_eq!(marked(&view, cx), Some(1..2));
    assert_eq!(inserts(&seen), [Value::from_iter([("text", "")])]);
    // Composing on edits nothing more.
    let deleted = history(&view, cx);
    assert_ne!(deleted, before);
    mark(&view, cx, "かな", 2..2);
    assert_eq!(history(&view, cx), deleted);
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(None, "仮名", window, cx)
    });
    assert_eq!(text(&view, cx), "a仮名d");

    // The deletion was one undo step.
    undo(cx);
    assert_eq!(text(&view, cx), "ad");
    undo(cx);
    assert_eq!(text(&view, cx), "abcd");

    // An input method reconverting buffer text deletes it the same way,
    // then composes there.
    let (view, cx, _) = editor(cx, "ab漢字", &[(8, 8)]);
    ime(&view, cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(Some(2..4), "かんじ", Some(3..3), window, cx)
    });
    assert_eq!(text(&view, cx), "ab");
    assert_eq!(marked(&view, cx), Some(2..5));
    undo(cx);
    assert_eq!(text(&view, cx), "ab漢字");
}

/// The bounds of UTF-16 `range` reported to the input method.
fn ime_bounds(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    range: Range<usize>,
) -> Bounds<Pixels> {
    ime(view, cx, |view, window, cx| {
        view.bounds_for_range(range, Default::default(), window, cx)
            .unwrap()
    })
}

fn caret_at(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    offset: usize,
) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        view.layout.as_mut().unwrap().caret_bounds(offset).unwrap()
    })
}

#[gpui::test]
fn painted_as_inlay(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx, "abcde\nf", &[(2, 2)]);
    draw(cx);
    let at = caret_at(&view, cx, 2);
    let after = caret_at(&view, cx, 3);
    let next_row = caret_at(&view, cx, 7);
    assert_eq!(view.read_with(cx, |view, _| view.painted_composition), None);

    // The input method's caret after the first char.
    mark(&view, cx, "かな", 1..1);
    draw(cx);
    let (inlay, underline, carets) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let inlay = layout.bounds(layout.frame.inlay.expect("laid out"));
        (inlay, view.painted_composition, view.painted_carets.clone())
    });
    // The composition is at the caret, and the rest of its row past it.
    near(inlay.origin.x, at.origin.x);
    assert_eq!(inlay.origin.y, at.origin.y);
    assert!(inlay.size.width > px(0.));
    let shifted = caret_at(&view, cx, 3);
    near(shifted.origin.x, after.origin.x + inlay.size.width);
    assert_eq!(caret_at(&view, cx, 7), next_row);

    // Underlined across it, in the text's colour.
    let underline = underline.expect("underlined");
    near(underline.origin.x, inlay.origin.x);
    near(underline.size.width, inlay.size.width);
    near(underline.bottom(), inlay.bottom());
    assert!(underline.size.height < inlay.size.height);

    // The one caret is inside it, after "か".
    let first = ime_bounds(&view, cx, 2..3);
    assert_eq!(carets.len(), 1);
    near(carets[0].origin.x, first.right());
    assert_eq!(carets[0].origin.y, inlay.origin.y);
    assert!(inlay.origin.x < carets[0].origin.x && carets[0].origin.x < inlay.right());
    // With "か" selected, at the selection's end, as the input method is
    // told.
    mark(&view, cx, "かな", 0..1);
    draw(cx);
    let selected = ime(&view, cx, |view, window, cx| {
        view.selected_text_range(false, window, cx).unwrap()
    });
    assert_eq!((selected.range, selected.reversed), (2..3, false));
    let carets = view.read_with(cx, |view, _| view.painted_carets.clone());
    assert_eq!(carets.len(), 1);
    near(carets[0].origin.x, first.right());

    // The input method finds the composition inside it.
    let whole = ime_bounds(&view, cx, 2..4);
    near(whole.origin.x, inlay.origin.x);
    near(whole.size.width, inlay.size.width);
    assert_eq!(whole.origin.y, inlay.origin.y);
    assert_eq!(whole.size.height, inlay.size.height);
    near(first.origin.x, inlay.origin.x);
    assert!(first.right() < inlay.right());
    let second = ime_bounds(&view, cx, 3..4);
    near(second.right(), inlay.right());
    // Past it, what's after it on the row.
    assert_eq!(ime_bounds(&view, cx, 5..5), shifted);
    let index = ime(&view, cx, |view, window, cx| {
        let over_second = point(second.center().x, second.center().y);
        view.character_index_for_point(over_second, window, cx)
    });
    assert!(matches!(index, Some(3 | 4)));
    let index = ime(&view, cx, |view, window, cx| {
        view.character_index_for_point(shifted.center(), window, cx)
    });
    assert_eq!(index, Some(5));
    // At its right edge, and on the start of the char after it, its end.
    for x in [inlay.right(), inlay.right() + px(1.)] {
        let index = ime(&view, cx, |view, window, cx| {
            view.character_index_for_point(point(x, inlay.center().y), window, cx)
        });
        assert_eq!(index, Some(4));
    }

    // With the commit it's buffer text, drawn as any other.
    ime(&view, cx, |view, window, cx| view.unmark_text(window, cx));
    draw(cx);
    view.read_with(cx, |view, _| {
        assert_eq!(view.layout.as_ref().unwrap().frame.inlay, None);
        assert_eq!(view.painted_composition, None);
    });
    near(caret_at(&view, cx, 9).origin.x, shifted.origin.x);
}

#[gpui::test]
fn geometry_is_of_the_drawn_composition(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx, "ab", &[(2, 2)]);
    mark(&view, cx, "k", 1..1);
    draw(cx);
    let at = caret_at(&view, cx, 2);
    let k = ime_bounds(&view, cx, 2..3);
    assert!(k.size.width > px(0.));

    // Until a frame draws it as it is now, the input method gets the caret
    // it's at, not the inlay of what it was.
    ime(&view, cx, |view, window, cx| {
        view.replace_and_mark_text_in_range(None, "かな", Some(2..2), window, cx);
        for range in [2..4, 3..4] {
            let bounds = view.bounds_for_range(range, Default::default(), window, cx);
            assert_eq!(bounds, Some(at));
        }
        let index = view.character_index_for_point(k.center(), window, cx);
        assert_eq!(index, Some(2));
    });
    draw(cx);
    let whole = ime_bounds(&view, cx, 2..4);
    assert!(whole.size.width > k.size.width);
    // Past the end of the row, its end.
    let index = ime(&view, cx, |view, window, cx| {
        let past = point(whole.right() + px(20.), whole.center().y);
        view.character_index_for_point(past, window, cx)
    });
    assert_eq!(index, Some(4));
}

/// Composes a long text at the caret, at UTF-16 offset `at`, and checks
/// its caret, at its end, is painted inside the text area, frame after
/// frame.
fn compose_past_the_edge(view: &Entity<EditorView>, cx: &mut VisualTestContext, at: usize) {
    let text_bounds = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.text_bounds())
    });
    let long = "あいうえおかきくけこさしすせそ";
    mark(view, cx, long, 15..15);
    // The second frame doesn't autoscroll.
    for _ in 0..2 {
        draw(cx);
        let carets = view.read_with(cx, |view, _| view.painted_carets.clone());
        assert_eq!(carets.len(), 1);
        near(
            carets[0].origin.x,
            ime_bounds(view, cx, at + 15..at + 15).origin.x,
        );
        assert!(carets[0].right() <= text_bounds.right());
        assert!(carets[0].origin.x > text_bounds.origin.x);
    }
    // The inlay starts where the buffer caret is drawn, before it.
    let inlay = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.inlay.unwrap())
    });
    near(ime_bounds(view, cx, at..at).origin.x, inlay.origin.x);
}

#[gpui::test]
fn the_composition_caret_stays_in_view(cx: &mut TestAppContext) {
    let row = "x".repeat(2000);
    let (view, cx, _) = editor(cx, &row, &[(2000, 2000)]);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(false));
    draw(cx);
    let scrolled = view.read_with(cx, |view, _| view.scroll.x);
    assert!(scrolled > px(0.));
    compose_past_the_edge(&view, cx, 2000);
    assert!(view.read_with(cx, |view, _| view.scroll.x) > scrolled);
}

#[gpui::test]
fn the_caret_of_a_wrapped_composition_stays_in_view(cx: &mut TestAppContext) {
    let row = "x".repeat(5000);
    let (view, cx, _) = editor(cx, &row, &[(0, 0)]);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(true));
    draw(cx);
    // On the last char of the first display row, by the right edge.
    let end = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.rows[0].end
    });
    assert!(end < 5000);
    view.update(cx, |view, _| view.model_mut().set_cursor(end - 1));
    draw(cx);
    assert_eq!(view.read_with(cx, |view, _| view.scroll.x), px(0.));
    compose_past_the_edge(&view, cx, end - 1);

    // Once it's committed, rows wrap as before.
    ime(&view, cx, |view, window, cx| view.unmark_text(window, cx));
    draw(cx);
    assert_eq!(view.read_with(cx, |view, _| view.scroll.x), px(0.));
}

#[gpui::test]
fn the_caret_stays_in_a_text_area_narrower_than_the_margin(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(cx, &"x".repeat(100), &[(0, 0)]);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(true));
    draw(cx);
    let char_width = caret_at(&view, cx, 1).origin.x - caret_at(&view, cx, 0).origin.x;
    let text_left = view.read_with(cx, |view, _| view.layout.as_ref().unwrap().frame.text_left);
    // Two chars wide, half the four-em margin.
    let height = cx.update(|window, _| window.viewport_size().height);
    cx.simulate_resize(gpui::size(px(text_left) + char_width * 2., height));
    draw(cx);
    let text_bounds = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.text_bounds())
    });
    assert!(text_bounds.size.width < char_width * 4.);

    for (composing, end) in [("か", 1), ("あいうえお", 5)] {
        mark(&view, cx, composing, end..end);
        draw(cx);
        let carets = view.read_with(cx, |view, _| view.painted_carets.clone());
        assert_eq!(carets.len(), 1);
        near(carets[0].origin.x, ime_bounds(&view, cx, end..end).origin.x);
        assert!(carets[0].origin.x >= text_bounds.origin.x);
        assert!(carets[0].right() <= text_bounds.right());
    }
}

#[gpui::test]
fn the_caret_of_a_composition_past_a_long_row_stays_in_view(cx: &mut TestAppContext) {
    let len = MAX_SHAPED_ROW_BYTES + 100;
    let (view, cx, _) = editor(cx, &"x".repeat(len), &[(len, len)]);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(false));
    draw(cx);
    compose_past_the_edge(&view, cx, len);
}

#[gpui::test]
fn cancelling_an_overflowing_composition_shows_the_caret(cx: &mut TestAppContext) {
    let row = "x".repeat(2000);
    let (view, cx, _) = editor(cx, &row, &[(100, 100)]);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(false));
    draw(cx);
    let long = "あいうえおかきくけこさしすせそ".repeat(30);
    let end = long.chars().count();
    for how in 0..2 {
        // Composed and drawn, so the frame scrolls to its caret.
        mark(&view, cx, &long, end..end);
        draw(cx);
        draw(cx);
        assert!(view.read_with(cx, |view, _| view.scroll.x) > px(0.));
        match how {
            0 => mark(&view, cx, "", 0..0),
            1 => ime(&view, cx, |view, window, cx| {
                view.replace_text_in_range(None, "", window, cx)
            }),
            _ => unreachable!(),
        }
        draw(cx);
        draw(cx);
        assert_eq!(marked(&view, cx), None, "{how}");
        let text_bounds = view.read_with(cx, |view, _| {
            let layout = view.layout.as_ref().unwrap();
            layout.bounds(layout.frame.text_bounds())
        });
        let caret = caret_at(&view, cx, 100);
        assert!(caret.origin.x >= text_bounds.origin.x, "{how}");
        assert!(caret.right() <= text_bounds.right(), "{how}");
    }
}

#[gpui::test]
fn a_movement_that_goes_nowhere_ends_the_composition_and_shows_the_caret(cx: &mut TestAppContext) {
    let row = "x".repeat(2000);
    let (view, cx, _) = editor(cx, &row, &[(2000, 2000)]);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(false));
    draw(cx);
    let long = "あいうえおかきくけこさしすせそ".repeat(30);
    let end = long.chars().count();
    mark(&view, cx, &long, end..end);
    draw(cx);
    draw(cx);
    let scrolled = view.read_with(cx, |view, _| view.scroll.x);
    // Right of the row's end, a move right goes nowhere.
    run(Invocation::of("editor.move_right").unwrap(), cx);
    assert_eq!(marked(&view, cx), None);
    assert_eq!(heads(&view, cx), [2000]);
    draw(cx);
    draw(cx);
    assert!(view.read_with(cx, |view, _| view.scroll.x) <= scrolled);
    let text_bounds = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.text_bounds())
    });
    let caret = caret_at(&view, cx, 2000);
    assert!(caret.origin.x >= text_bounds.origin.x);
    assert!(caret.right() <= text_bounds.right());
}

#[gpui::test]
fn another_long_row_stays_drawn_beside_a_wide_composition(cx: &mut TestAppContext) {
    let len = MAX_SHAPED_ROW_BYTES + 100;
    let text = format!("{}\n{}", "x".repeat(len), "y".repeat(len));
    let (view, cx, _) = editor(cx, &text, &[(100, 100)]);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(false));
    draw(cx);
    let long = "あいうえおかきくけこさしすせそ".repeat(60);
    let end = long.chars().count();
    mark(&view, cx, &long, end..end);
    for _ in 0..3 {
        draw(cx);
        view.read_with(cx, |view, _| {
            let layout = view.layout.as_ref().unwrap();
            let frame = &layout.frame;
            assert!(frame.inlay.is_some());
            assert_eq!(frame.rows.len(), 2);
            let (left, right) = (
                frame.scroll.x as f64,
                (frame.scroll.x + frame.text_bounds().width) as f64,
            );
            let other = &frame.rows[1];
            let slice = other.slice.as_ref().expect("a long row");
            let shaped_end = other.start_x() + f64::from(other.line.width());
            assert!(slice.start_x <= left, "{slice:?} vs {left}");
            assert!(shaped_end >= right, "{shaped_end} vs {right}");
        });
    }
}

#[gpui::test]
fn a_wide_composition_mid_way_along_a_long_row_stays_drawn(cx: &mut TestAppContext) {
    let len = MAX_SHAPED_ROW_BYTES + 100;
    let (view, cx, _) = editor(cx, &"x".repeat(len), &[(100, 100)]);
    view.update(cx, |view, _| view.model_mut().set_soft_wrap(false));
    draw(cx);
    let text_bounds = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout.bounds(layout.frame.text_bounds())
    });
    let long = "あいうえおかきくけこさしすせそ".repeat(60);
    let end = long.chars().count();
    mark(&view, cx, &long, end..end);
    for _ in 0..3 {
        draw(cx);
        assert!(view.read_with(cx, |view, _| {
            view.layout.as_ref().unwrap().frame.inlay.is_some()
        }));
        assert!(view.read_with(cx, |view, _| view.painted_composition.is_some()));
        let carets = view.read_with(cx, |view, _| view.painted_carets.clone());
        assert_eq!(carets.len(), 1);
        near(
            carets[0].origin.x,
            ime_bounds(&view, cx, 100 + end..100 + end).origin.x,
        );
        assert!(carets[0].right() <= text_bounds.right());
        assert!(carets[0].origin.x > text_bounds.origin.x);
    }
    // Cancelled, the buffer caret is back in view.
    mark(&view, cx, "", 0..0);
    draw(cx);
    draw(cx);
    let caret = caret_at(&view, cx, 100);
    assert!(caret.origin.x >= text_bounds.origin.x);
    assert!(caret.right() <= text_bounds.right());
}

#[gpui::test]
fn dropped(cx: &mut TestAppContext) {
    let (view, cx, seen) = editor(cx, "ab", &[(2, 2)]);
    let before = history(&view, cx);
    draw(cx);
    let start = caret_at(&view, cx, 0).center();

    // A click.
    mark(&view, cx, "か", 1..1);
    cx.simulate_click(start, Modifiers::none());
    assert_eq!(marked(&view, cx), None);
    assert_eq!(heads(&view, cx), [0]);

    // Losing focus.
    mark(&view, cx, "か", 1..1);
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    assert_eq!(marked(&view, cx), None);
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));

    // A move, and one that can't go further.
    mark(&view, cx, "か", 1..1);
    cx.simulate_keystrokes("left");
    assert_eq!(marked(&view, cx), None);
    assert_eq!(heads(&view, cx), [0]);
    mark(&view, cx, "か", 1..1);
    cx.simulate_keystrokes("right");
    assert_eq!(marked(&view, cx), None);
    assert_eq!(heads(&view, cx), [1]);
    view.update(cx, |view, _| view.model_mut().set_cursor(2));
    mark(&view, cx, "か", 1..1);
    cx.simulate_keystrokes("right");
    assert_eq!(marked(&view, cx), None);
    assert_eq!(heads(&view, cx), [2]);

    // Commands that neither move nor edit leave it, and so does one the
    // registry refuses.
    mark(&view, cx, "か", 1..1);
    cx.simulate_keystrokes("cmd-c");
    let refused = Invocation::of("editor.insert")
        .unwrap()
        .args(Value::from_iter([("unexpected", "x")]));
    run(refused, cx);
    run(Invocation::of("editor.toggle_soft_wrap").unwrap(), cx);
    run(Invocation::of("editor.show_hover").unwrap(), cx);
    assert_eq!(marked(&view, cx), Some(2..3));
    // So do vim's commands with vim off, and accepting a completion outside
    // the menu's context or with args.
    run(Invocation::of("vim.normal_mode").unwrap(), cx);
    run(Invocation::of("completion.accept").unwrap(), cx);
    let menu = fe_editor::commands::context().with(ContextFrame::new(completion_menu::KEY_CONTEXT));
    let accept = Invocation::of("completion.accept").unwrap().context(menu);
    run(
        accept.clone().args(Value::from_iter([("unexpected", "x")])),
        cx,
    );
    assert_eq!(marked(&view, cx), Some(2..3));
    // Accepting one in the menu ends it.
    run(accept, cx);
    assert_eq!(marked(&view, cx), None);
    // A fold command, which may move carets, ends it.
    mark(&view, cx, "か", 1..1);
    run(Invocation::of("editor.unfold_all").unwrap(), cx);
    assert_eq!(marked(&view, cx), None);

    // Committing nothing, typing nothing: it's no longer drawn.
    for marked_range in [false, true] {
        mark(&view, cx, "か", 1..1);
        draw(cx);
        assert!(view.read_with(cx, |view, _| view.painted_composition.is_some()));
        ime(&view, cx, |view, window, cx| {
            let range = marked_range
                .then(|| view.marked_text_range(window, cx))
                .flatten();
            view.replace_text_in_range(range, "", window, cx)
        });
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.painted_composition.is_none()));
    }

    assert_eq!(text(&view, cx), "ab");
    assert_eq!(history(&view, cx), before);
    assert!(inserts(&seen).is_empty());

    // Vim coming on mid-composition, in normal mode, drops it: unmarking
    // then types nothing.
    mark(&view, cx, "か", 1..1);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    assert_eq!(marked(&view, cx), None);
    ime(&view, cx, |view, window, cx| view.unmark_text(window, cx));
    assert_eq!(text(&view, cx), "ab");
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    mark(&view, cx, "か", 1..1);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    cx.update(|_, cx| set_vim_enabled(cx, false));
    cx.run_until_parked();
    // Back out of vim, it's still gone.
    assert_eq!(marked(&view, cx), None);
    ime(&view, cx, |view, window, cx| view.unmark_text(window, cx));
    assert_eq!(text(&view, cx), "ab");
    // So it is with vim on and off again before anything looks.
    mark(&view, cx, "か", 1..1);
    cx.update(|window, cx| {
        set_vim_enabled(cx, true);
        set_vim_enabled(cx, false);
        view.update(cx, |view, cx| view.unmark_text(window, cx));
    });
    assert_eq!(text(&view, cx), "ab");
    assert_eq!(history(&view, cx), before);

    // In vim's normal mode an input method changes nothing.
    cx.update(|_, cx| set_vim_enabled(cx, true));
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("normal")
    );
    mark(&view, cx, "か", 1..1);
    assert_eq!(marked(&view, cx), None);
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(None, "加", window, cx);
        view.unmark_text(window, cx);
    });
    assert_eq!(text(&view, cx), "ab");
    assert_eq!(history(&view, cx), before);

    // In insert mode a vim command its context refuses leaves it, and one
    // it takes ends it.
    cx.simulate_keystrokes("i");
    mark(&view, cx, "か", 1..1);
    let composing = marked(&view, cx);
    assert!(composing.is_some());
    let down = Value::from_iter([("motion", "down")]);
    run(Invocation::of("vim.move").unwrap().args(down), cx);
    assert_eq!(marked(&view, cx), composing);
    run(Invocation::of("vim.normal_mode").unwrap(), cx);
    assert_eq!(marked(&view, cx), None);
    assert_eq!(text(&view, cx), "ab");

    // In insert mode it composes, and the commit is typed into the session.
    view.update(cx, |view, _| view.model_mut().set_cursor(1));
    cx.simulate_keystrokes("i");
    mark(&view, cx, "か", 1..1);
    assert_eq!(text(&view, cx), "ab");
    assert_eq!(marked(&view, cx), Some(1..2));
    ime(&view, cx, |view, window, cx| {
        view.replace_text_in_range(None, "加", window, cx)
    });
    cx.simulate_keystrokes("escape");
    assert_eq!(text(&view, cx), "a加b");
    cx.simulate_keystrokes("u");
    assert_eq!(text(&view, cx), "ab");
}
