//! Clicks on the git change bar in a headless window: a click on a change's
//! mark emits its rows and moves the caret there; the rest of the gutter
//! behaves as it did.

use std::{cell::RefCell, ops::Range, rc::Rc};

use fe_buffer::Buffer;
use fe_display::layout::Rect;
use fe_editor::EditorModel;
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    Point, TestAppContext, VisualTestContext, point, px,
};

use crate::{EditorTheme, EditorView, init, view::EditorViewEvent};

const BASE: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\n";
/// Rows 4 and 5 added, and `fn e` of the base deleted below them.
const ADDED: &str =
    "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nnew1\nnew2\nfn e() {}\nfn f() {}\n";
const EOF_DELETED: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n";
const DELETED: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn f() {}\n";

fn editor<'a>(
    text: &str,
    cx: &'a mut TestAppContext,
) -> (
    Entity<EditorView>,
    &'a mut VisualTestContext,
    Rc<RefCell<Vec<EditorViewEvent>>>,
) {
    editor_on(BASE, text, cx)
}

fn editor_on<'a>(
    base: &str,
    text: &str,
    cx: &'a mut TestAppContext,
) -> (
    Entity<EditorView>,
    &'a mut VisualTestContext,
    Rc<RefCell<Vec<EditorViewEvent>>>,
) {
    cx.update(init);
    let (text, base) = (text.to_string(), base.to_string());
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        model.set_diff_base(Some(base.into()));
        model.set_soft_wrap(true);
        model.set_cursor(text.len());
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    let events = Rc::new(RefCell::new(Vec::new()));
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &EditorViewEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
    (view, cx, events)
}

fn bounds_of(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    rect: impl Fn(&fe_display::layout::Frame<crate::element::Line>) -> Rect,
) -> Bounds<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        layout.bounds(rect(&layout.frame))
    })
}

/// The middle of `column` on buffer row `row`.
fn at(
    view: &Entity<EditorView>,
    row: u32,
    column: impl Fn(&fe_display::layout::Frame<crate::element::Line>) -> Rect,
    cx: &mut VisualTestContext,
) -> Point<Pixels> {
    let column = bounds_of(view, cx, column);
    let line = bounds_of(view, cx, |frame| {
        let display = frame.rows.iter().find(|r| r.buffer_row == row).unwrap();
        Rect {
            x: 0.,
            y: frame.row_top(display.row),
            width: 1.,
            height: frame.line_height,
        }
    });
    point(column.center().x, line.center().y)
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

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    view.read_with(cx, |view, _| {
        let text = view.model.text();
        let point = text.offset_to_point(view.model.primary_selection().range.end);
        (point.row, point.column)
    })
}

#[gpui::test]
fn emits_rows(cx: &mut TestAppContext) {
    {
        let (view, cx, events) = editor(ADDED, cx);
        click(at(&view, 5, |f| f.change_lane, cx), cx);
        assert_eq!(
            *events.borrow(),
            [EditorViewEvent::ChangeBarClicked { rows: 4..6 }]
        );
        assert_eq!(caret(&view, cx), (4, 0));
    }
    {
        // Both halves of a deletion mark, which sits across the top edge of
        // its row; the middle of that row, away from it, isn't the mark.
        let (view, cx, events) = editor(DELETED, cx);
        let edge = at(&view, 3, |f| f.change_lane, cx);
        let top = bounds_of(&view, cx, |f| Rect {
            x: 0.,
            y: f.row_top(f.rows.iter().find(|r| r.buffer_row == 3).unwrap().row),
            width: 1.,
            height: 1.,
        })
        .origin
        .y;
        click(point(edge.x, edge.y), cx);
        assert_eq!(*events.borrow(), []);
        for dy in [-2., 2.] {
            events.borrow_mut().clear();
            click(point(edge.x, top + px(dy)), cx);
            let rows: Range<u32> = 3..3;
            assert_eq!(
                *events.borrow(),
                [EditorViewEvent::ChangeBarClicked { rows }]
            );
            assert_eq!(caret(&view, cx), (3, 0));
        }
    }
    // A deletion at the end of the text: the top of the empty last row.
    let (view, cx, events) = editor(EOF_DELETED, cx);
    let last = bounds_of(&view, cx, |f| {
        let row = f.rows.last().unwrap();
        Rect {
            x: 0.,
            y: f.row_top(row.row),
            width: 1.,
            height: f.line_height,
        }
    });
    let lane = bounds_of(&view, cx, |f| f.change_lane);
    click(point(lane.center().x, last.top() + px(2.)), cx);
    let rows: Range<u32> = 5..5;
    assert_eq!(
        *events.borrow(),
        [EditorViewEvent::ChangeBarClicked { rows }]
    );
}

#[gpui::test]
fn elsewhere_unchanged(cx: &mut TestAppContext) {
    let (view, cx, events) = editor(ADDED, cx);
    let before = caret(&view, cx);
    click(at(&view, 1, |f| f.change_lane, cx), cx);
    assert_eq!(*events.borrow(), []);
    let numbers = bounds_of(&view, cx, |f| f.fold_column);
    let row = at(&view, 1, |f| f.fold_column, cx);
    click(point(numbers.left() - px(3.), row.y), cx);
    assert_eq!(*events.borrow(), []);
    let _ = before;
    // A chevron still toggles its fold, with no event.
    let (view, cx, events) = editor("fn a() {\n    x();\n}\n", cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    click(at(&view, 0, |f| f.fold_column, cx), cx);
    cx.update(|window, cx| window.draw(cx).clear(cx));
    assert_eq!(
        view.read_with(cx, |view, _| view.shows_buffer_row(1)),
        Some(false)
    );
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn a_click_leaves_vim_visual_mode(cx: &mut TestAppContext) {
    let (view, cx, events) = editor(ADDED, cx);
    cx.update(|_, cx| crate::vim::set_vim_enabled(cx, true));
    cx.simulate_keystrokes("escape v l");
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("visual")
    );
    click(at(&view, 5, |f| f.change_lane, cx), cx);
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(
        view.read_with(cx, |view, _| view.vim_mode()),
        Some("normal")
    );
    assert_eq!(caret(&view, cx), (4, 0));
}

#[gpui::test]
fn a_wrapped_rows_continuation_has_no_deletion_mark(cx: &mut TestAppContext) {
    let long = "word ".repeat(400);
    let base = format!("a\nb\nc\ngone\n{long}\n");
    let text = format!("a\nb\nc\n{long}\n");
    let (view, cx, events) = editor_on(&base, &text, cx);
    let fragments = view.read_with(cx, |view, _| {
        let frame = &view.layout.as_ref().unwrap().frame;
        frame.rows.iter().filter(|row| row.buffer_row == 3).count()
    });
    assert!(fragments > 1, "the row wraps");
    let lane = bounds_of(&view, cx, |f| f.change_lane);
    let top_of = |nth: usize, view: &Entity<EditorView>, cx: &mut VisualTestContext| {
        bounds_of(view, cx, |f| {
            let row = f
                .rows
                .iter()
                .filter(|row| row.buffer_row == 3)
                .nth(nth)
                .unwrap();
            Rect {
                x: 0.,
                y: f.row_top(row.row),
                width: 1.,
                height: 1.,
            }
        })
        .origin
        .y
    };
    let second = top_of(1, &view, cx);
    click(point(lane.center().x, second + px(1.)), cx);
    assert_eq!(*events.borrow(), []);
    let first = top_of(0, &view, cx);
    click(point(lane.center().x, first + px(1.)), cx);
    assert_eq!(
        *events.borrow(),
        [EditorViewEvent::ChangeBarClicked { rows: 3..3 }]
    );
}
