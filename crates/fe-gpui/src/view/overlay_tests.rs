//! The host's overlay under an anchor range: drawn at the text area's left
//! under the anchor's last row, following edits, hidden while the row is
//! scrolled out, one per editor.

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{
    AnyView, AppContext as _, Bounds, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Pixels, Render, Styled as _, TestAppContext, VisualTestContext, div, px,
};

use crate::*;

/// An overlay with a selector to find it by.
struct Probe(&'static str);

impl Render for Probe {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let name = self.0;
        div()
            .w(px(120.))
            .h(px(24.))
            .debug_selector(move || name.into())
    }
}

fn text() -> String {
    (0..200).map(|row| format!("line {row}\n")).collect()
}

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(&text()), None);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    (view, cx)
}

fn probe(name: &'static str, cx: &mut VisualTestContext) -> AnyView {
    cx.new(|_| Probe(name)).into()
}

/// The start of `row`.
fn row_start(row: usize) -> usize {
    text().lines().take(row).map(|line| line.len() + 1).sum()
}

fn set(
    view: &Entity<EditorView>,
    overlay: Option<AnyView>,
    rows: std::ops::Range<usize>,
    cx: &mut VisualTestContext,
) {
    let range = row_start(rows.start)..row_start(rows.end) - 1;
    view.update(cx, |view, cx| {
        view.set_overlay(overlay, range);
        cx.notify();
    });
    cx.run_until_parked();
}

fn scroll(view: &Entity<EditorView>, top: f32, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| {
        view.scroll.y = px(top);
        view.autoscroll = false;
        cx.notify();
    });
    cx.run_until_parked();
}

fn bounds(name: &'static str, cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
    cx.debug_bounds(name)
}

/// The bottom of buffer `row` and the text area's left, in the last frame.
fn row_bottom_and_left(
    view: &Entity<EditorView>,
    row: usize,
    cx: &mut VisualTestContext,
) -> (Pixels, Pixels) {
    view.update(cx, |view, _| {
        let caret = (view.layout.as_mut().unwrap())
            .caret_bounds(row_start(row))
            .unwrap();
        let left = view
            .layout
            .as_ref()
            .unwrap()
            .bounds(fe_display::layout::Rect {
                x: view.layout.as_ref().unwrap().frame.text_left,
                y: 0.,
                width: 0.,
                height: 0.,
            });
        (caret.bottom(), left.origin.x)
    })
}

#[gpui::test]
fn overlay_is_drawn_under_the_anchor_row(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let overlay = probe("overlay", cx);
    set(&view, Some(overlay), 2..4, cx);
    assert_eq!(
        view.read_with(cx, |view, _| view.overlay_anchor_row()),
        Some(3)
    );
    let shown = bounds("overlay", cx).expect("drawn");
    let (bottom, left) = row_bottom_and_left(&view, 3, cx);
    assert!(
        shown.top() >= bottom - px(0.5),
        "{shown:?} under {bottom:?}"
    );
    assert!(shown.top() <= bottom + px(0.5));
    assert!(
        (shown.left() - left).abs() <= px(1.),
        "{shown:?} at {left:?}"
    );
}

#[gpui::test]
fn a_second_overlay_replaces_the_first(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let (first, second) = (probe("first", cx), probe("second", cx));
    set(&view, Some(first), 2..3, cx);
    assert!(bounds("first", cx).is_some());
    set(&view, Some(second), 5..6, cx);
    assert!(bounds("first", cx).is_none());
    assert!(bounds("second", cx).is_some());
}

#[gpui::test]
fn none_removes_the_overlay(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let overlay = probe("overlay", cx);
    set(&view, Some(overlay), 2..3, cx);
    assert!(bounds("overlay", cx).is_some());
    set(&view, None, 2..3, cx);
    assert!(bounds("overlay", cx).is_none());
    assert_eq!(
        view.read_with(cx, |view, _| view.overlay_anchor_row()),
        None
    );
}

#[gpui::test]
fn overlay_follows_edits_above_the_anchor(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let overlay = probe("overlay", cx);
    set(&view, Some(overlay), 5..6, cx);
    let before = bounds("overlay", cx).unwrap();
    view.update(cx, |view, cx| {
        view.model_mut().set_cursor(0);
        view.model_mut().insert("a\nb\n");
        cx.notify();
    });
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.overlay_anchor_row()),
        Some(7)
    );
    let after = bounds("overlay", cx).unwrap();
    let line = view.read_with(cx, |view, _| view.theme.font_size * view.theme.line_height);
    assert!(((after.top() - before.top()) - line * 2.).abs() <= px(1.));
}

#[gpui::test]
fn overlay_hides_when_scrolled_out_of_view(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    let overlay = probe("overlay", cx);
    set(&view, Some(overlay), 2..3, cx);
    assert!(bounds("overlay", cx).is_some());
    scroll(&view, 5_000., cx);
    cx.run_until_parked();
    assert!(bounds("overlay", cx).is_none());
    assert_eq!(
        view.read_with(cx, |view, _| view.overlay_anchor_row()),
        Some(2)
    );
    scroll(&view, 0., cx);
    cx.run_until_parked();
    assert!(bounds("overlay", cx).is_some());
}

/// An editor beside a neighbour that counts the clicks it gets.
struct Pair {
    editor: Entity<EditorView>,
    clicks: std::rc::Rc<std::cell::Cell<usize>>,
}

impl Render for Pair {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .flex()
            .size_full()
            .child(
                div()
                    .w(px(200.))
                    .h_full()
                    .debug_selector(|| "neighbour".into())
                    .on_mouse_down(gpui::MouseButton::Left, move |_, _, _| {
                        clicks.set(clicks.get() + 1)
                    }),
            )
            .child(
                div()
                    .w(px(500.))
                    .h(px(300.))
                    .debug_selector(|| "pane".into())
                    .child(self.editor.clone()),
            )
    }
}

/// An overlay far bigger than the editor is cut to it, and a click beside the
/// editor isn't taken by it.
#[gpui::test]
fn an_oversized_overlay_stays_inside_the_editor(cx: &mut TestAppContext) {
    let clicks = std::rc::Rc::new(std::cell::Cell::new(0));
    let (host, cx) = cx.add_window_view({
        let clicks = clicks.clone();
        move |_, cx| {
            let model = EditorModel::new(Buffer::new(&text()), None);
            let editor = cx.new(|cx| EditorView::new(model, EditorTheme::default_dark(cx), cx));
            Pair { editor, clicks }
        }
    });
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    let editor = host.read_with(cx, |host, _| host.editor.clone());
    let big: AnyView = cx.new(|_| Big).into();
    // Under the last visible rows, so the clamp has to move it too.
    set(&editor, Some(big), 2..3, cx);
    let pane = bounds("pane", cx).unwrap();
    assert!(bounds("big", cx).is_some(), "drawn");
    let shown = bounds("editor-overlay", cx).expect("drawn");
    assert!(
        shown.left() >= pane.left() && shown.right() <= pane.right(),
        "{shown:?} in {pane:?}"
    );
    assert!(
        shown.top() >= pane.top() && shown.bottom() <= pane.bottom(),
        "{shown:?} in {pane:?}"
    );

    let neighbour = bounds("neighbour", cx).unwrap();
    cx.simulate_click(neighbour.center(), gpui::Modifiers::none());
    assert_eq!(clicks.get(), 1, "the neighbour got the click");
}

struct Big;

impl Render for Big {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
            .w(px(5000.))
            .h(px(5000.))
            .flex_none()
            .debug_selector(|| "big".into())
    }
}
