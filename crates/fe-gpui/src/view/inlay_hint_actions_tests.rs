//! An inlay hint's tooltip on hover and its edits on double-click, resolved
//! through the host.

use std::{cell::Cell, rc::Rc};

use fe_buffer::Buffer;
use fe_editor::EditorModel;
use gpui::{
    Bounds, Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    TestAppContext, VisualTestContext, px,
};

use crate::{
    hover::HOVER_DELAY,
    inlay_hints::{InlayHint, InlayHintFuture, InlayHintHost},
    *,
};

const TEXT: &str = "let a = 1;\n";

/// Resolves the hint at 5 to a tooltip and its edits, after `delay`.
struct Host {
    asked: Rc<Cell<usize>>,
    edits: Vec<(std::ops::Range<usize>, String)>,
    delay: std::time::Duration,
    executor: gpui::BackgroundExecutor,
}

impl InlayHintHost for Host {
    fn resolve(&self, mut hint: InlayHint) -> InlayHintFuture {
        self.asked.set(self.asked.get() + 1);
        let (edits, delay, executor) = (self.edits.clone(), self.delay, self.executor.clone());
        Box::pin(async move {
            if !delay.is_zero() {
                executor.timer(delay).await;
            }
            if hint.offset == 5 {
                hint.tooltip = Some("the type: u8".to_string());
                hint.edits = edits;
            }
            hint
        })
    }
}

fn hint(offset: usize, label: &str) -> InlayHint {
    InlayHint {
        offset,
        label: label.to_string(),
        kind: None,
        padding_left: false,
        padding_right: false,
        data: None,
        tooltip: None,
        edits: Vec::new(),
    }
}

fn editor(
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, Rc<Cell<usize>>, &mut VisualTestContext) {
    let insert = vec![(5..5, ": u8".to_string())];
    editor_with(insert, std::time::Duration::ZERO, cx)
}

fn editor_with(
    edits: Vec<(std::ops::Range<usize>, String)>,
    delay: std::time::Duration,
    cx: &mut TestAppContext,
) -> (Entity<EditorView>, Rc<Cell<usize>>, &mut VisualTestContext) {
    let asked = Rc::new(Cell::new(0));
    let host = Rc::new(Host {
        asked: asked.clone(),
        edits,
        delay,
        executor: cx.executor(),
    });
    let (view, cx) = cx.add_window_view(|_, cx| {
        let model = EditorModel::new(Buffer::new(TEXT), None);
        let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
        view.set_inlay_hint_host(Some(host));
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    draw(cx);
    view.update(cx, |view, _| {
        let version = view.model.snapshot().version();
        view.set_inlay_hints(version, vec![hint(5, ": u8"), hint(8, "x:")]);
    });
    draw(cx);
    (view, asked, cx)
}

fn draw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn painted(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> Vec<Bounds<Pixels>> {
    view.read_with(cx, |view, _| {
        view.painted_inlay_hints.iter().map(|(_, b)| *b).collect()
    })
}

fn text(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.read_with(cx, |view, _| view.model.text().to_string())
}

fn double_click(at: gpui::Point<Pixels>, cx: &mut VisualTestContext) {
    for click_count in [1, 2] {
        cx.simulate_event(MouseDownEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: at,
            modifiers: Modifiers::none(),
            button: MouseButton::Left,
            click_count,
        });
    }
    draw(cx);
}

#[gpui::test]
fn hovering_shows_the_tooltip_and_double_click_applies_edits(cx: &mut TestAppContext) {
    let (view, asked, cx) = editor(cx);
    let shown = painted(&view, cx);
    let (first, second) = (shown[0].center(), shown[1].center());

    // Resting on a hint shows its resolved tooltip once the delay is up.
    cx.simulate_mouse_move(first, None, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.popover_text()), None);
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
    assert_eq!(
        view.read_with(cx, |view, _| view.popover_text()).as_deref(),
        Some("the type: u8")
    );
    assert_eq!(asked.get(), 1);

    // Moves within it don't ask again.
    cx.simulate_mouse_move(
        gpui::point(first.x + px(1.), first.y),
        None,
        Modifiers::none(),
    );
    cx.run_until_parked();
    assert_eq!(asked.get(), 1);

    // A hint with no tooltip shows nothing.
    cx.simulate_mouse_move(second, None, Modifiers::none());
    cx.run_until_parked();
    cx.executor().advance_clock(HOVER_DELAY * 2);
    cx.run_until_parked();
    assert_eq!(view.read_with(cx, |view, _| view.popover_text()), None);

    // A double-click on it, with no edits, selects the word as before.
    double_click(second, cx);
    assert_eq!(text(&view, cx), TEXT);
    assert_eq!(painted(&view, cx).len(), 2);
    let selected = view.read_with(cx, |view, _| view.model.primary_selection().range);
    assert_eq!(selected, 8..9);

    // A double-click on the other applies its edits in one transaction.
    double_click(first, cx);
    assert_eq!(text(&view, cx), "let a: u8 = 1;\n");
    assert_eq!(painted(&view, cx).len(), 1);
    assert_eq!(
        view.read_with(cx, |view, _| view.inlay_hints())
            .iter()
            .map(|(_, label)| label.as_str())
            .collect::<Vec<_>>(),
        ["x:"]
    );
    view.update(cx, |view, _| {
        view.model.undo().unwrap();
    });
    assert_eq!(text(&view, cx), TEXT);
}

const SLOW: std::time::Duration = std::time::Duration::from_secs(1);

fn rest_on(at: gpui::Point<Pixels>, cx: &mut VisualTestContext) {
    cx.simulate_mouse_move(at, None, Modifiers::none());
    cx.run_until_parked();
    cx.executor().advance_clock(HOVER_DELAY);
    cx.run_until_parked();
}

#[gpui::test]
fn hover_and_double_click_share_one_resolve(cx: &mut TestAppContext) {
    let (view, asked, cx) = editor_with(vec![(5..5, ": u8".to_string())], SLOW, cx);
    let at = painted(&view, cx)[0].center();
    rest_on(at, cx);
    double_click(at, cx);
    double_click(at, cx);
    assert_eq!(asked.get(), 1);
    assert_eq!(text(&view, cx), TEXT);

    cx.executor().advance_clock(SLOW);
    cx.run_until_parked();
    assert_eq!(text(&view, cx), "let a: u8 = 1;\n");
    view.update(cx, |view, _| {
        view.model.undo().unwrap();
    });
    assert_eq!(text(&view, cx), TEXT);
}

#[gpui::test]
fn a_late_answer_for_replaced_hints_is_dropped(cx: &mut TestAppContext) {
    let (view, asked, cx) = editor_with(vec![(5..5, ": u8".to_string())], SLOW, cx);
    let at = painted(&view, cx)[0].center();
    rest_on(at, cx);
    double_click(at, cx);
    assert_eq!(asked.get(), 1);

    // A refresh gives the same label at the same place.
    view.update(cx, |view, _| {
        let version = view.model.snapshot().version();
        view.set_inlay_hints(version, vec![hint(5, ": u8"), hint(8, "x:")]);
    });
    draw(cx);
    cx.executor().advance_clock(SLOW);
    cx.run_until_parked();
    assert_eq!(text(&view, cx), TEXT);
    assert_eq!(view.read_with(cx, |view, _| view.popover_text()), None);
}

#[gpui::test]
fn edits_over_changed_text_are_not_applied(cx: &mut TestAppContext) {
    let (view, _, cx) = editor_with(vec![(4..5, "b".to_string())], std::time::Duration::ZERO, cx);
    let at = painted(&view, cx)[0].center();
    rest_on(at, cx);
    // The text the edit replaces is changed before the double-click.
    view.update(cx, |view, cx| {
        (view.model.buffer_mut()).edit([(4..5, "q")], fe_buffer::Origin::User);
        cx.notify();
    });
    draw(cx);
    let at = painted(&view, cx)[0].center();
    double_click(at, cx);
    assert_eq!(text(&view, cx), "let q = 1;\n");
}
