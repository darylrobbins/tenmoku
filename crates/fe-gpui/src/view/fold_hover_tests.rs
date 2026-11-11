//! The gutter's fold chevrons in a headless window: folded rows' chevrons
//! always show, and the rest only while the pointer is in the gutter. A move
//! that changes whether it is, or which chevron it's over, notifies the
//! view, whose window a test app then draws when effects flush.

use std::{cell::Cell, rc::Rc};

use fe_buffer::Buffer;
use fe_display::layout::{Frame, Rect};
use fe_editor::EditorModel;
use gpui::{
    AppContext as _, Bounds, Context, Entity, Focusable as _, InteractiveElement as _, IntoElement,
    Modifiers, MouseExitEvent, ParentElement as _, Pixels, Point, Render, ScrollDelta,
    ScrollWheelEvent, Styled as _, TestAppContext, TouchPhase, VisualTestContext, Window, div,
    point, px,
};

use crate::{element::Line, *};

/// Rows 0 and 4 head fold candidates; the caret is on the last function.
const TEXT: &str = "fn f() {\n    one();\n    two();\n}\nfn g() {\n    three();\n}\nfn h() {}\n";

/// A focused Rust editor on [`TEXT`], drawn once its tree is parsed.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    editor_on(TEXT.to_string(), cx)
}

/// A focused Rust editor on `text`, which starts with [`TEXT`].
fn editor_on(
    text: String,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        model.set_cursor(TEXT.find("fn h").unwrap());
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    let parsed = view.update(cx, |view, _| {
        view.syntax().and_then(fe_syntax::Syntax::tree).is_some()
    });
    assert!(parsed);
    (view, cx)
}

/// The window bounds of `rect` in the last frame.
fn bounds_of(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    rect: impl FnOnce(&Frame<Line>) -> Rect,
) -> Bounds<Pixels> {
    view.update(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.bounds(rect(&layout.frame))
    })
}

/// The chevrons the last frame painted, as `(buffer row, folded)`.
fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<(u32, bool)> {
    view.update(cx, |view, _| {
        let chevrons = view.painted_chevrons.iter();
        chevrons.map(|&(row, folded, _)| (row, folded)).collect()
    })
}

/// Counts the view's notifications from now on.
fn count_notifications(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Rc<Cell<usize>> {
    let count = Rc::new(Cell::new(0));
    cx.update(|_, cx| {
        let count = count.clone();
        cx.observe(view, move |_, _| count.set(count.get() + 1))
            .detach();
    });
    count
}

/// Moves the pointer to `position` and says whether that notified the view.
fn move_to(position: Point<Pixels>, count: &Cell<usize>, cx: &mut VisualTestContext) -> bool {
    let before = count.get();
    cx.simulate_mouse_move(position, None, Modifiers::none());
    cx.run_until_parked();
    count.get() > before
}

fn exit_window(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseExitEvent {
        position,
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    cx.run_until_parked();
}

/// The middle of the fold column on display row `row`.
fn column_at(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
    let column = bounds_of(view, cx, |frame| frame.fold_column);
    let line = bounds_of(view, cx, |frame| Rect {
        x: 0.,
        y: frame.row_top(row),
        width: 1.,
        height: frame.line_height,
    });
    point(column.center().x, line.center().y)
}

#[gpui::test]
fn open_fold_chevrons_show_only_while_the_pointer_is_in_the_gutter(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let candidates = [(0, false), (4, false)];
    assert_eq!(painted(&view, cx), [], "the pointer hasn't moved");
    let text_left = bounds_of(&view, cx, |frame| frame.text_bounds()).left();
    let numbers = bounds_of(&view, cx, |frame| frame.problem_column).left() - px(3.);
    let numbers = point(numbers, column_at(&view, 0, cx).y);
    let text = point(text_left + px(20.), column_at(&view, 0, cx).y);
    let count = count_notifications(&view, cx);

    move_to(text, &count, cx);
    assert_eq!(painted(&view, cx), [], "over the text");
    // Into the gutter over the line numbers.
    assert!(move_to(numbers, &count, cx));
    assert_eq!(painted(&view, cx), candidates, "over the line numbers");
    // Within it, onto the fold column of a row that can't fold: nothing
    // changes.
    assert!(!move_to(column_at(&view, 2, cx), &count, cx));
    assert_eq!(painted(&view, cx), candidates);
    // Onto a candidate's chevron, which is drawn hovered.
    assert!(move_to(column_at(&view, 4, cx), &count, cx));
    assert_eq!(painted(&view, cx), candidates);
    // Just left of the text, in the gap after the change bar.
    let gap = point(text_left - px(2.), column_at(&view, 0, cx).y);
    assert!(move_to(gap, &count, cx));
    assert_eq!(painted(&view, cx), candidates, "in the gap before the code");
    assert!(move_to(text, &count, cx));
    assert_eq!(painted(&view, cx), [], "over the text");

    assert!(move_to(column_at(&view, 0, cx), &count, cx));
    exit_window(column_at(&view, 0, cx), cx);
    assert_eq!(painted(&view, cx), [], "the pointer left the window");
}

#[gpui::test]
fn the_gutter_moving_under_a_still_pointer_shows_the_chevrons(cx: &mut TestAppContext) {
    // 9,999 rows: one more widens the line numbers' column by a digit.
    let (view, cx) = editor_on(format!("{TEXT}{}", "\n".repeat(9990)), cx);
    let candidates = [(0, false), (4, false)];
    let text_left = bounds_of(&view, cx, |frame| frame.text_bounds()).left();
    let pointer = point(text_left + px(2.), column_at(&view, 0, cx).y);
    let count = count_notifications(&view, cx);
    move_to(pointer, &count, cx);
    assert_eq!(painted(&view, cx), [], "over the text");

    let edit = |range: std::ops::Range<usize>, new: &str, cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| {
            let mut buffer = view.model_mut().buffer_mut();
            buffer.edit([(range, new.to_string())], fe_buffer::Origin::User);
            drop(buffer);
            cx.notify();
        });
        cx.run_until_parked();
    };
    let len = TEXT.len() + 9990;
    edit(len..len, "\n", cx);
    assert_eq!(
        painted(&view, cx),
        candidates,
        "the gutter widened under the pointer"
    );
    edit(len..len + 1, "", cx);
    assert_eq!(painted(&view, cx), [], "and narrowed from under it");
}

#[gpui::test]
fn folded_chevrons_always_show(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    view.update(cx, |view, cx| {
        view.model_mut().fold_rows(0..=3);
        cx.notify();
    });
    cx.run_until_parked();
    // Row 4 shows on display row 1.
    assert_eq!(painted(&view, cx), [(0, true)], "the pointer hasn't moved");
    let count = count_notifications(&view, cx);

    let text = bounds_of(&view, cx, |frame| frame.text_bounds()).center();
    move_to(text, &count, cx);
    assert_eq!(painted(&view, cx), [(0, true)], "over the text");
    assert!(move_to(column_at(&view, 1, cx), &count, cx));
    assert_eq!(painted(&view, cx), [(0, true), (4, false)]);
    exit_window(column_at(&view, 1, cx), cx);
    assert_eq!(
        painted(&view, cx),
        [(0, true)],
        "the pointer left the window"
    );
}

#[gpui::test]
fn chevrons_paint_in_their_boxes(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    view.update(cx, |view, cx| {
        view.model_mut().fold_rows(0..=3);
        cx.notify();
    });
    cx.run_until_parked();
    let count = count_notifications(&view, cx);
    move_to(column_at(&view, 1, cx), &count, cx);

    let (chevrons, boxes) = view.update(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let frame = &layout.frame;
        let boxes: Vec<_> = frame
            .fold_toggles
            .iter()
            .map(|toggle| {
                let chevron = layout.bounds(frame.chevron_box(toggle));
                (toggle.buffer_row, toggle.folded, chevron)
            })
            .collect();
        (view.painted_chevrons.clone(), boxes)
    });
    assert_eq!(chevrons.len(), 2);
    assert_eq!(chevrons, boxes);
    for (_, folded, chevron) in chevrons {
        assert_eq!(chevron.size.width, chevron.size.height);
        let [start, tip, end] = crate::element::chevron_points(chevron, folded);
        assert!(chevron.contains(&start) && chevron.contains(&tip) && chevron.contains(&end));
        if folded {
            // Points right.
            assert_eq!(start.x, end.x);
            assert!(tip.x > start.x);
            assert_eq!(tip.y, chevron.center().y);
        } else {
            // Points down.
            assert_eq!(start.y, end.y);
            assert!(tip.y > start.y);
            assert_eq!(tip.x, chevron.center().x);
        }
    }
}

#[gpui::test]
fn the_whole_column_toggles(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let column = bounds_of(&view, cx, |frame| frame.fold_column);
    let row = column_at(&view, 0, cx).y;
    let folds = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            let folds = view.model().display().folds();
            folds
                .iter()
                .map(|fold| (fold.start, fold.end))
                .collect::<Vec<_>>()
        })
    };

    cx.simulate_click(point(column.left() + px(1.), row), Modifiers::none());
    assert_eq!(folds(cx), [(0, 2)]);
    cx.simulate_click(point(column.right() - px(1.), row), Modifiers::none());
    assert_eq!(folds(cx), []);
}

#[gpui::test]
fn a_click_or_scroll_after_leaving_shows_them_again(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let candidates = [(0, false), (4, false)];
    let at = column_at(&view, 2, cx);
    let count = count_notifications(&view, cx);
    move_to(at, &count, cx);
    exit_window(at, cx);
    assert_eq!(painted(&view, cx), []);

    // The window moved under the still pointer, which then clicks.
    cx.simulate_click(at, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), candidates, "clicked");

    exit_window(at, cx);
    assert_eq!(painted(&view, cx), []);
    cx.simulate_event(ScrollWheelEvent {
        position: at,
        delta: ScrollDelta::Pixels(point(px(0.), px(0.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), candidates, "scrolled");
}

/// An editor with, while `covered`, an overlay over its left 200 px.
struct Covered {
    editor: Entity<EditorView>,
    covered: bool,
}

impl Render for Covered {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let overlay = self.covered.then(|| {
            div()
                .id("overlay")
                .absolute()
                .top_0()
                .left_0()
                .w(px(200.))
                .h_full()
                .occlude()
        });
        div()
            .relative()
            .size_full()
            .child(self.editor.clone())
            .children(overlay)
    }
}

#[gpui::test]
fn a_covered_column_stays_unhovered_through_key_presses(cx: &mut TestAppContext) {
    cx.update(init);
    let (host, cx) = cx.add_window_view(|_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), Some("main.rs".into()));
        model.set_cursor(TEXT.find("fn h").unwrap());
        let editor = cx.new(|cx| EditorView::new(model, EditorTheme::default_dark(cx), cx));
        Covered {
            editor,
            covered: true,
        }
    });
    let view = host.read_with(cx, |host, _| host.editor.clone());
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    let column = bounds_of(&view, cx, |frame| frame.fold_column);
    assert!(column.right() < px(200.), "the overlay covers the column");
    let at = column_at(&view, 2, cx);
    let count = count_notifications(&view, cx);

    move_to(at, &count, cx);
    assert_eq!(painted(&view, cx), [], "covered");
    // A key press redraws the editor, and suppresses hover in GPUI.
    cx.simulate_keystrokes("x");
    cx.run_until_parked();
    let typed = view.update(cx, |view, _| view.model().text().to_string());
    assert_ne!(typed, TEXT, "the key reached the editor");
    assert_eq!(painted(&view, cx), [], "still covered");

    // Uncovered under the still pointer.
    host.update(cx, |host, cx| {
        host.covered = false;
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(painted(&view, cx), [(0, false), (4, false)], "uncovered");
}

#[path = "gutter_glyph_tests.rs"]
mod gutter_glyph;
