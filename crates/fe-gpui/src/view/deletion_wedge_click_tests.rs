//! Clicks on a deletion's red wedge in a headless window: anywhere along
//! the wedge's half-line height, across the edge it sits on.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{
    Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels, Point,
    TestAppContext, VisualTestContext, point, px,
};

use crate::{EditorTheme, EditorView, init, view::EditorViewEvent};

const BASE: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\nfn f() {}\n";
const DELETED: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn f() {}\n";
const EOF_DELETED: &str = "fn a() {}\nfn b() {}\nfn c() {}\nfn d() {}\nfn e() {}\n";

type Events = Rc<RefCell<Vec<EditorViewEvent>>>;

fn editor<'a>(
    text: &str,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext, Events) {
    cx.update(init);
    let text = text.to_string();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        model.set_diff_base(Some(BASE.into()));
        model.set_soft_wrap(true);
        model.set_cursor(text.len());
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    let events: Events = Rc::new(RefCell::new(Vec::new()));
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

/// Clicks a quarter line less a pixel above and below the top edge of the
/// last frame's display row `row_at`, which emit `rows` and put the caret
/// at `caret_row`; the middle of that row emits nothing.
fn assert_wedge(text: &str, last: bool, rows: std::ops::Range<u32>, cx: &mut TestAppContext) {
    let (view, cx, events) = editor(text, cx);
    let (x, edge, line_height) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let row = match last {
            true => frame.rows.last().unwrap(),
            false => frame
                .rows
                .iter()
                .find(|row| row.buffer_row == rows.start)
                .unwrap(),
        };
        let lane = layout.bounds(frame.change_lane);
        let top = layout.origin.y + px(frame.row_top(row.row));
        (lane.center().x, top, px(frame.line_height))
    });
    let reach = line_height / 4. - px(1.);
    for dy in [-reach, reach] {
        events.borrow_mut().clear();
        click(point(x, edge + dy), cx);
        assert_eq!(
            *events.borrow(),
            [EditorViewEvent::ChangeBarClicked { rows: rows.clone() }],
            "{dy:?}"
        );
        let caret = view.read_with(cx, |view, _| {
            let text = view.model.text();
            text.offset_to_point(view.model.primary_selection().range.end)
                .row
        });
        assert_eq!(caret, rows.start);
    }
    events.borrow_mut().clear();
    click(point(x, edge + line_height / 2.), cx);
    assert_eq!(*events.borrow(), []);
}

#[gpui::test]
fn a_click_along_the_wedge_hits_it(cx: &mut TestAppContext) {
    assert_wedge(DELETED, false, 3..3, cx);
}

#[gpui::test]
fn the_wedge_at_the_end_of_the_text_hits_too(cx: &mut TestAppContext) {
    assert_wedge(EOF_DELETED, true, 5..5, cx);
}
