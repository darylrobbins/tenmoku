//! The minimap on the left in a headless window: it takes the editor's left
//! edge, and the gutter, the text and everything placed by them move right
//! by its width, so clicks on the text, the change bar and fold chevrons
//! land as they did, and switching the side lays the frame out again,
//! hiding popovers the text moved out from under, as a scroll does.

use std::{cell::RefCell, rc::Rc};

use fe_buffer::{Buffer, BufferId, Version};
use fe_display::{
    layout::Rect,
    minimap::{MinimapOptions, MinimapSide},
};
use fe_editor::EditorModel;
use gpui::{
    Bounds, Entity, EntityInputHandler as _, Focusable as _, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, Pixels, Point, TestAppContext, VisualTestContext, point, px,
    size,
};

use crate::{
    EditorTheme, EditorView, HoverContent, HoverFuture, HoverProvider, hover::HOVER_DELAY, init,
    view::EditorViewEvent,
};

const BASE: &str = "fn a() {\n    b();\n}\nfn c() {}\n";
/// Rows 3 and 4 added.
const ADDED: &str = "fn a() {\n    b();\n}\nnew1\nnew2\nfn c() {}\n";

type Events = Rc<RefCell<Vec<EditorViewEvent>>>;

/// An editor on `text`, changed from [`BASE`], in an 800 × 400 window with
/// the minimap on `side`, drawn, and the events it emits.
fn editor<'a>(
    text: &str,
    side: MinimapSide,
    cx: &'a mut TestAppContext,
) -> (Entity<EditorView>, &'a mut VisualTestContext, Events) {
    cx.update(init);
    let text = text.to_owned();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        model.set_diff_base(Some(BASE.into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.simulate_resize(size(px(800.), px(400.)));
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    let events = Events::default();
    let sink = events.clone();
    cx.update(|_, cx| {
        cx.subscribe(&view, move |_, event: &EditorViewEvent, _| {
            sink.borrow_mut().push(event.clone())
        })
        .detach();
    });
    set_side(&view, side, cx);
    (view, cx, events)
}

fn set_side(view: &Entity<EditorView>, side: MinimapSide, cx: &mut VisualTestContext) {
    let options = MinimapOptions {
        side,
        ..MinimapOptions::default()
    };
    view.update(cx, |view, cx| view.set_minimap(Some(options), cx));
    draw(cx);
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn minimap(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.read_with(cx, |view, _| view.minimap_bounds().expect("shown"))
}

/// What's placed by the gutter and the text, in window coordinates: the
/// text area, the change lane, the first fold chevron, the caret at the
/// start of row 1's `b` (where a popover by it is anchored) and the bounds
/// the input method is given for that `b`.
fn placed(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> [Bounds<Pixels>; 5] {
    let b = BASE.find("b()").expect("b");
    let [text, lane, chevron, caret] = view.update(cx, |view, _| {
        let layout = view.layout.as_mut().expect("drawn");
        let frame = &layout.frame;
        let toggle = frame.fold_toggles.first().expect("a fold");
        let rects = [
            frame.text_bounds(),
            frame.change_lane,
            frame.chevron_box(toggle),
        ];
        let [text, lane, chevron] = rects.map(|rect| layout.bounds(rect));
        [text, lane, chevron, layout.caret_bounds(b).expect("shown")]
    });
    let ime = cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.bounds_for_range(b..b + 1, Default::default(), window, cx)
        })
    });
    [text, lane, chevron, caret, ime.expect("bounds")]
}

#[gpui::test]
fn switching_to_the_left_moves_the_minimap_to_the_edge_and_the_rest_right_by_its_width(
    cx: &mut TestAppContext,
) {
    let (view, cx, _) = editor(BASE, MinimapSide::Right, cx);
    let right = minimap(&view, cx);
    assert_eq!(right.right(), px(800.));
    let before = placed(&view, cx);

    // Live: the next frame is laid out again.
    set_side(&view, MinimapSide::Left, cx);
    let left = minimap(&view, cx);
    assert_eq!(left.origin, point(px(0.), px(0.)));
    assert_eq!(left.size, right.size);
    let after = placed(&view, cx);
    for (before, after) in before.iter().zip(&after) {
        assert_eq!(after.origin, before.origin + point(left.size.width, px(0.)));
        assert_eq!(after.size, before.size);
    }
    // The gutter starts at the minimap's right edge, the text area ends at
    // the editor's.
    assert!(after[1].left() >= left.right());
    assert_eq!(after[0].right(), px(800.));

    set_side(&view, MinimapSide::Right, cx);
    assert_eq!(minimap(&view, cx), right);
    assert_eq!(placed(&view, cx), before);
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
    draw(cx);
}

fn caret(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (u32, u32) {
    view.read_with(cx, |view, _| {
        let text = view.model.text();
        let point = text.offset_to_point(view.model.primary_selection().range.end);
        (point.row, point.column)
    })
}

/// The middle of the left half of the char at `offset`.
fn on_char(view: &Entity<EditorView>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.update(cx, |view, _| {
        let layout = view.layout.as_mut().expect("drawn");
        let (start, end) = (layout.caret_bounds(offset), layout.caret_bounds(offset + 1));
        let (start, end) = (start.expect("shown"), end.expect("shown"));
        point(
            start.left() + (end.left() - start.left()) / 4.,
            start.center().y,
        )
    })
}

#[gpui::test]
fn a_click_at_the_first_text_column_lands_there(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(BASE, MinimapSide::Left, cx);
    for (row, column) in [(3, 0), (1, 0), (1, 4), (0, 3)] {
        let offset = view.read_with(cx, |view, _| view.model.text().row_start(row)) + column;
        click(on_char(&view, offset, cx), cx);
        assert_eq!(caret(&view, cx), (row, column as u32));
    }
}

#[gpui::test]
fn a_click_on_the_minimap_scrolls_and_leaves_the_caret(cx: &mut TestAppContext) {
    let text: String = (0..300).map(|n| format!("let v{n} = {n};\n")).collect();
    let (view, cx, _) = editor(&text, MinimapSide::Left, cx);
    let at = caret(&view, cx);
    let bounds = minimap(&view, cx);
    click(point(bounds.center().x, bounds.bottom() - px(20.)), cx);
    let scrolled = view.read_with(cx, |view, _| view.scroll.y);
    assert!(scrolled > px(0.), "{scrolled:?}");
    assert_eq!(caret(&view, cx), at);
}

#[gpui::test]
fn change_bar_clicks_and_fold_chevrons_still_hit(cx: &mut TestAppContext) {
    let (view, cx, events) = editor(ADDED, MinimapSide::Left, cx);
    let row_middle = |view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext| {
        view.read_with(cx, |view, _| {
            let layout = view.layout.as_ref().expect("drawn");
            let frame = &layout.frame;
            let display = frame
                .rows
                .iter()
                .find(|r| r.buffer_row == row)
                .expect("row");
            let lane = layout.bounds(frame.change_lane);
            let line = layout.bounds(Rect {
                x: 0.,
                y: frame.row_top(display.row),
                width: 1.,
                height: frame.line_height,
            });
            point(lane.center().x, line.center().y)
        })
    };
    click(row_middle(&view, 4, cx), cx);
    assert_eq!(
        *events.borrow(),
        [EditorViewEvent::ChangeBarClicked { rows: 3..5 }]
    );
    assert_eq!(caret(&view, cx), (3, 0));

    // The chevron of `fn a`'s fold folds it.
    let chevron = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().expect("drawn");
        let toggle = layout.frame.fold_toggles.first().expect("a fold");
        layout.bounds(layout.frame.chevron_box(toggle)).center()
    });
    click(chevron, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.shows_buffer_row(1)),
        Some(false)
    );
}

fn content(text: &str) -> HoverContent {
    HoverContent {
        text: text.into(),
        range: None,
    }
}

/// Answers every request at once.
struct Provider;

impl HoverProvider for Provider {
    fn hover(&self, _: BufferId, _: Version, _: usize) -> HoverFuture {
        Box::pin(async { Some(content("about b")) })
    }
}

fn popover(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Option<String> {
    view.read_with(cx, |view, _| view.painted_popover.clone())
}

fn rest_on_b(view: &Entity<EditorView>, delay: bool, cx: &mut VisualTestContext) {
    let b = BASE.find("b()").expect("b");
    let at = on_char(view, b, cx);
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    if delay {
        cx.executor().advance_clock(HOVER_DELAY);
    }
    draw(cx);
}

#[gpui::test]
fn switching_sides_hides_popovers_at_the_caret_and_the_pointer(cx: &mut TestAppContext) {
    let (view, cx, _) = editor(BASE, MinimapSide::Right, cx);
    view.update(cx, |view, _| {
        view.set_hover_provider(Some(Rc::new(Provider)))
    });
    cx.update(|window, _| window.activate_window());
    for side in [MinimapSide::Left, MinimapSide::Right] {
        // At the caret.
        view.update(cx, |view, cx| {
            view.show_popover(content("at the caret"), cx)
        });
        draw(cx);
        assert_eq!(popover(&view, cx).as_deref(), Some("at the caret"));
        set_side(&view, side, cx);
        assert_eq!(popover(&view, cx), None, "{side:?}");

        // At the pointer, shown.
        let back = match side {
            MinimapSide::Left => MinimapSide::Right,
            MinimapSide::Right => MinimapSide::Left,
        };
        set_side(&view, back, cx);
        rest_on_b(&view, true, cx);
        assert_eq!(popover(&view, cx).as_deref(), Some("about b"));
        set_side(&view, side, cx);
        assert_eq!(popover(&view, cx), None, "{side:?}");

        // At the pointer, waiting for the delay: nothing comes of it.
        set_side(&view, back, cx);
        cx.simulate_mouse_move(point(px(400.), px(390.)), None, Modifiers::none());
        draw(cx);
        rest_on_b(&view, false, cx);
        set_side(&view, side, cx);
        cx.executor().advance_clock(HOVER_DELAY);
        draw(cx);
        assert_eq!(popover(&view, cx), None, "{side:?}");
    }
}
