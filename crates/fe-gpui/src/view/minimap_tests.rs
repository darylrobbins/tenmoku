//! The mouse over the minimap in a headless window: a click centres the row
//! there, the slider drags wherever the pointer goes, the wheel scrolls, and
//! none of it moves the carets or vim's mode; the text keeps its own clicks
//! up to its right edge.

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::minimap::MinimapOptions;
use fe_editor::EditorModel;
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px, size,
};

use crate::{EditorTheme, EditorView, init, vim::set_vim_enabled};

/// A Rust editor on 300 numbered rows, the minimap on, in an 800 × 400
/// window, with rows 0's first 5 bytes selected.
fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(init);
    let text: String = (0..300)
        .map(|n| format!("let value_{n} = {n};\n"))
        .collect();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        model.set_selections([ResolvedSelection {
            range: 0..5,
            reversed: false,
        }]);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_minimap(Some(MinimapOptions::default()), cx);
        view
    });
    cx.simulate_resize(size(px(800.), px(400.)));
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

fn minimap(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    view.read_with(cx, |view, _| view.minimap_bounds().expect("shown"))
}

/// The display row at the editor's top, fractional, and how many it holds.
fn top(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> (f32, f32) {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let first = (frame.block_map).to_display_top(frame.scroll.y / frame.line_height);
        (first, layout.minimap.as_ref().unwrap().screen_rows)
    })
}

fn selection(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> std::ops::Range<usize> {
    view.read_with(cx, |view, _| view.model().primary_selection().range)
}

fn press(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseDownEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    draw(cx);
}

fn drag_to(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseMoveEvent {
        position,
        pressed_button: Some(MouseButton::Left),
        modifiers: Modifiers::none(),
    });
    draw(cx);
}

fn release(position: Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_event(MouseUpEvent {
        button: MouseButton::Left,
        position,
        modifiers: Modifiers::none(),
        click_count: 1,
    });
    draw(cx);
}

/// Where the minimap draws display row `row`, across its middle.
fn row_at(view: &Entity<EditorView>, row: u32, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let minimap = layout.minimap.as_ref().unwrap();
        let geometry = &minimap.geometry;
        let y = geometry.row_y(row) + geometry.row_height / 2.;
        let x = (geometry.x.start + geometry.x.end) / 2.;
        layout.to_window(fe_display::layout::Vec2 { x, y })
    })
}

#[gpui::test]
fn a_click_centres_its_row_and_leaves_the_selection(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let at = row_at(&view, 150, cx);
    press(at, cx);
    release(at, cx);
    let (first, visible) = top(&view, cx);
    let centre = first + visible / 2.;
    assert!((centre - 150.5).abs() < 0.01, "{first} + {visible} / 2");
    assert_eq!(selection(&view, cx), 0..5);
    assert!(!view.read_with(cx, |view, _| view.minimap_dragging()));
}

#[gpui::test]
fn dragging_the_slider_reaches_the_last_page_and_back_from_anywhere(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let minimap = minimap(&view, cx);
    let start = point(minimap.center().x, minimap.top() + px(5.));
    press(start, cx);
    assert_eq!(top(&view, cx).0, 0., "pressed on the slider: no jump");
    assert!(view.read_with(cx, |view, _| view.minimap_dragging()));
    // Over the text and past the window's bottom.
    drag_to(point(px(200.), px(2000.)), cx);
    let last = view.read_with(cx, |view, _| view.model.display().row_count() - 1);
    assert_eq!(top(&view, cx).0, last as f32);
    let shown = view.read_with(cx, |view, _| view.shows_buffer_row(last));
    assert_eq!(shown, Some(true), "the last page");
    // Past the window's right edge, back up.
    drag_to(point(px(900.), start.y), cx);
    assert_eq!(top(&view, cx).0, 0.);
    drag_to(point(px(900.), start.y + px(30.)), cx);
    let (first, _) = top(&view, cx);
    assert!(first > 0., "{first}");
    release(point(px(900.), start.y + px(30.)), cx);
    assert!(!view.read_with(cx, |view, _| view.minimap_dragging()));
    // Released: moving no longer scrolls.
    cx.simulate_mouse_move(start, None, Modifiers::none());
    draw(cx);
    assert_eq!(top(&view, cx).0, first);
    assert_eq!(selection(&view, cx), 0..5, "no text selection started");
}

#[gpui::test]
fn the_wheel_over_the_minimap_scrolls(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let minimap = minimap(&view, cx);
    cx.simulate_event(ScrollWheelEvent {
        position: minimap.center(),
        delta: ScrollDelta::Lines(point(0., -10.)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    draw(cx);
    let (first, _) = top(&view, cx);
    assert!(first >= 9., "{first}");
    assert_eq!(selection(&view, cx), 0..5);
}

#[gpui::test]
fn a_click_at_the_texts_right_edge_lands_in_the_text(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    view.update(cx, |view, cx| {
        let long = "x".repeat(400);
        (view.model.buffer_mut()).edit([(0..0, format!("{long}\n"))], fe_buffer::Origin::User);
        cx.notify();
    });
    draw(cx);
    let (right, y) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let text = layout.bounds(layout.frame.text_bounds());
        let row = &layout.frame.rows[0];
        let y = layout.origin.y + px(layout.frame.row_top(row.row) + 1.);
        (text.right(), y)
    });
    assert_eq!(right, minimap(&view, cx).left());
    let edge = point(right - px(1.), y);
    press(edge, cx);
    release(edge, cx);
    let caret = selection(&view, cx);
    assert!(caret.is_empty() && caret.start > 50, "{caret:?}");
    assert!(!view.read_with(cx, |view, _| view.minimap_dragging()));
}

#[gpui::test]
fn in_vim_a_click_on_the_minimap_leaves_the_mode_and_caret(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    cx.update(|_, cx| set_vim_enabled(cx, true));
    draw(cx);
    let mode = |cx: &mut VisualTestContext| {
        view.update(cx, |view, cx| {
            view.sync_vim(cx);
            view.vim
                .as_ref()
                .expect("vim is on")
                .mode_name()
                .to_string()
        })
    };
    assert_eq!(mode(cx), "normal");
    cx.simulate_keystrokes("j l");
    let caret = selection(&view, cx);
    let at = row_at(&view, 150, cx);
    press(at, cx);
    release(at, cx);
    assert_eq!(mode(cx), "normal");
    assert_eq!(selection(&view, cx), caret);
    assert!(top(&view, cx).0 > 100.);

    cx.simulate_keystrokes("v l");
    assert!(mode(cx).starts_with("visual"));
    let selected = selection(&view, cx);
    let at = row_at(&view, 10, cx);
    press(at, cx);
    drag_to(row_at(&view, 200, cx), cx);
    release(row_at(&view, 200, cx), cx);
    assert!(mode(cx).starts_with("visual"), "{}", mode(cx));
    assert_eq!(selection(&view, cx), selected);
}

#[gpui::test]
fn a_click_centres_its_row_on_screen_across_a_block_above_it(cx: &mut TestAppContext) {
    let (view, cx) = editor_with_block(cx);
    let at = row_at(&view, 105, cx);
    press(at, cx);
    release(at, cx);
    let (centre, middle, line_height) = view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let row = (frame.rows.iter()).find(|row| row.buffer_row == 105);
        let row = row.expect("the clicked row is shown");
        let centre = frame.row_top(row.row) + frame.line_height / 2.;
        (centre, frame.height / 2., frame.line_height)
    });
    assert!(
        (centre - middle).abs() < line_height / 2.,
        "{centre} vs {middle}"
    );
    assert_eq!(selection(&view, cx), 0..5);
}

/// [`editor`] with a block of 15 screen rows above row 100, as a large
/// proposal's hunk takes.
fn editor_with_block(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    editor_with_block_above(100, cx)
}

/// [`editor`] with a block of 15 screen rows above display row `above`;
/// after the last row at 301.
fn editor_with_block_above(
    above: u32,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx) = editor(cx);
    view.update(cx, |view, cx| {
        let block = crate::view::HostBlock {
            above,
            rows: 15,
            kind: crate::view::HostBlockKind::Filler,
            id: 1,
        };
        view.set_blocks(vec![block], cx);
    });
    draw(cx);
    (view, cx)
}

/// Scrolls to screen row `top` and draws; the line height.
fn scroll_to_screen(view: &Entity<EditorView>, top: f32, cx: &mut VisualTestContext) -> f32 {
    view.update(cx, |view, cx| view.set_scroll_top(top, cx));
    draw(cx);
    view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    })
}

/// How far the editor is scrolled, in pixels, as the last frame left it.
fn scroll(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    view.read_with(cx, |view, _| view.scroll.y.into())
}

/// The middle of the slider.
fn slider(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        layout
            .bounds(layout.minimap.as_ref().unwrap().slider())
            .center()
    })
}

#[gpui::test]
fn inside_a_block_a_drag_across_leaves_the_scroll(cx: &mut TestAppContext) {
    let (view, cx) = editor_with_block(cx);
    // A click whose centring puts the top inside the block, then a move
    // sideways.
    let at = row_at(&view, 105, cx);
    press(at, cx);
    let clicked = scroll(&view, cx);
    drag_to(point(at.x - px(30.), at.y), cx);
    assert_eq!(scroll(&view, cx), clicked);
    release(at, cx);

    // Scrolled into the block, a press on the slider and a move sideways.
    view.update(cx, |view, cx| view.set_scroll_top(105., cx));
    draw(cx);
    let inside = scroll(&view, cx);
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    });
    assert_eq!(inside, 105. * line_height);
    let at = slider(&view, cx);
    press(at, cx);
    assert_eq!(scroll(&view, cx), inside);
    drag_to(point(at.x + px(200.), at.y), cx);
    assert_eq!(scroll(&view, cx), inside);
    release(at, cx);
}

#[gpui::test]
fn a_drag_across_a_block_is_monotonic_and_repeatable(cx: &mut TestAppContext) {
    let (view, cx) = editor_with_block(cx);
    let start = slider(&view, cx);
    press(start, cx);
    let mut scrolls = Vec::new();
    for step in 0..60 {
        drag_to(point(start.x, start.y + px(step as f32 * 4.)), cx);
        scrolls.push(scroll(&view, cx));
    }
    assert!(
        scrolls.windows(2).all(|pair| pair[0] <= pair[1]),
        "{scrolls:?}"
    );
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    });
    assert!(
        scrolls[59] > 115. * line_height,
        "past the block: {}",
        scrolls[59]
    );
    // Back to a height already passed, then sideways there: where it was.
    let y = start.y + px(30. * 4.);
    drag_to(point(start.x, y), cx);
    assert_eq!(scroll(&view, cx), scrolls[30]);
    drag_to(point(start.x - px(50.), y), cx);
    assert_eq!(scroll(&view, cx), scrolls[30]);
    release(start, cx);
}

#[gpui::test]
fn inside_a_trailing_block_a_drag_across_leaves_the_scroll(cx: &mut TestAppContext) {
    let (view, cx) = editor_with_block_above(301, cx);
    let rows = view.read_with(cx, |view, _| view.model.display().row_count());
    assert_eq!(rows, 301);
    let line_height = scroll_to_screen(&view, 306., cx);
    assert_eq!(scroll(&view, cx), 306. * line_height, "inside the block");
    let at = slider(&view, cx);
    press(at, cx);
    assert_eq!(scroll(&view, cx), 306. * line_height);
    drag_to(point(at.x - px(40.), at.y), cx);
    assert_eq!(scroll(&view, cx), 306. * line_height);
    // Down from past the last row's screen row: never back up.
    drag_to(point(at.x, at.y + px(3.)), cx);
    assert!(scroll(&view, cx) >= 306. * line_height);
    drag_to(point(at.x, at.y - px(2000.)), cx);
    assert_eq!(scroll(&view, cx), 0.);
    release(at, cx);
}

#[gpui::test]
fn from_inside_a_block_a_drag_reaches_the_top_and_the_bottom(cx: &mut TestAppContext) {
    let (view, cx) = editor_with_block(cx);
    let line_height = scroll_to_screen(&view, 105., cx);
    assert_eq!(scroll(&view, cx), 105. * line_height, "inside the block");
    let at = slider(&view, cx);
    press(at, cx);
    // The last row, 300, is at screen row 315, past the block.
    drag_to(point(at.x, at.y + px(2000.)), cx);
    assert_eq!(scroll(&view, cx), 315. * line_height);
    drag_to(point(at.x, at.y - px(2000.)), cx);
    assert_eq!(scroll(&view, cx), 0.);
    // Back where it started: where it started.
    drag_to(at, cx);
    assert_eq!(scroll(&view, cx), 105. * line_height);
    release(at, cx);
}

/// The pointer's travel over the minimap that scrolls `rows` display rows.
fn travel(view: &Entity<EditorView>, rows: f32, cx: &mut VisualTestContext) -> Pixels {
    view.read_with(cx, |view, _| {
        let geometry = &view
            .layout
            .as_ref()
            .unwrap()
            .minimap
            .as_ref()
            .unwrap()
            .geometry;
        px(rows * 100. / geometry.scroll_for_drag(0., 100.))
    })
}

/// The screen row at the top, from the last frame's scroll.
fn screen_top(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> f32 {
    let line_height = view.read_with(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.line_height
    });
    scroll(view, cx) / line_height
}

/// Asserts the screen top is `expected`, to float noise.
fn assert_screen_top(view: &Entity<EditorView>, expected: f32, cx: &mut VisualTestContext) {
    let top = screen_top(view, cx);
    assert!(
        (top - expected).abs() < 0.01,
        "screen top {top}, not {expected}"
    );
}

#[gpui::test]
fn from_inside_a_block_a_small_drag_scrolls_a_little_either_way(cx: &mut TestAppContext) {
    let (view, cx) = editor_with_block(cx);
    // The block above row 100 is at screen rows 100 to 114.
    scroll_to_screen(&view, 105., cx);
    let at = slider(&view, cx);
    let row = travel(&view, 1., cx);
    press(at, cx);
    drag_to(point(at.x, at.y - row), cx);
    assert_screen_top(&view, 104., cx);
    drag_to(point(at.x, at.y + row), cx);
    assert_screen_top(&view, 106., cx);
    // Past the block's edge, the rows beyond it: row 99 above, row 101
    // below.
    drag_to(point(at.x, at.y - row * 6.), cx);
    assert_screen_top(&view, 99., cx);
    drag_to(point(at.x, at.y + row * 11.), cx);
    assert_screen_top(&view, 116., cx);
    release(at, cx);
}

#[gpui::test]
fn from_inside_a_trailing_block_a_small_drag_scrolls_a_little_up(cx: &mut TestAppContext) {
    let (view, cx) = editor_with_block_above(301, cx);
    // The block after the last row, 300, is at screen rows 301 to 315.
    scroll_to_screen(&view, 306., cx);
    let at = slider(&view, cx);
    let row = travel(&view, 1., cx);
    press(at, cx);
    drag_to(point(at.x, at.y - row), cx);
    assert_screen_top(&view, 305., cx);
    // Down from past the last row's screen row: where it started.
    drag_to(point(at.x, at.y + row), cx);
    assert_screen_top(&view, 306., cx);
    // Past the block's top, the last row.
    drag_to(point(at.x, at.y - row * 6.), cx);
    assert_screen_top(&view, 300., cx);
    release(at, cx);
}

#[gpui::test]
fn from_inside_a_leading_block_a_drag_up_stops_at_the_top(cx: &mut TestAppContext) {
    let (view, cx) = editor_with_block_above(0, cx);
    // The block above row 0 is at screen rows 0 to 14.
    scroll_to_screen(&view, 5., cx);
    assert_screen_top(&view, 5., cx);
    let at = slider(&view, cx);
    let row = travel(&view, 1., cx);
    press(at, cx);
    drag_to(point(at.x, at.y - row), cx);
    assert_screen_top(&view, 4., cx);
    drag_to(point(at.x, at.y - row * 5.), cx);
    assert_screen_top(&view, 0., cx);
    // Just across the top, then further up: still the top.
    for rows in [5.5, 6., 10., 40.] {
        drag_to(point(at.x, at.y - row * rows), cx);
        assert_screen_top(&view, 0., cx);
    }
    // Down past the block: row 0, then row 1.
    drag_to(point(at.x, at.y + row * 10.), cx);
    assert_screen_top(&view, 15., cx);
    drag_to(point(at.x, at.y + row * 11.), cx);
    assert_screen_top(&view, 16., cx);
    release(at, cx);
}
