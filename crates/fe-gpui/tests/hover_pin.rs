//! Pinning the hover card: a second `editor.show_hover` keeps it until
//! `escape` or another `editor.show_hover`; the pointer leaving, scrolling
//! and edits elsewhere no longer hide it, the wheel scrolls its content
//! and a part of its text selects and copies.

use std::{path::PathBuf, rc::Rc};

use fe_buffer::{Buffer, BufferId, ResolvedSelection, Version};
use fe_config::{EditorPalette, themes};
use fe_editor::EditorModel;
use fe_gpui::{
    EditorSettings, EditorTheme, EditorView, HoverContent, HoverFuture, HoverProvider,
    PaintedHoverCard, init,
};
use gpui::{
    ClipboardItem, Entity, Focusable as _, Modifiers, MouseButton, MouseDownEvent, MouseExitEvent,
    ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase, VisualTestContext, point, px,
};
use kiln_core::theme::ThemeData;

const TEXT: &str = "fn main() {\n    let map = HashMap::new();\n    let x = 1;\n}\n";
const DOCS: &str = "Creates an empty `HashMap`.";

struct Provider;

impl HoverProvider for Provider {
    fn hover(&self, _: BufferId, _: Version, _: usize) -> HoverFuture {
        Box::pin(std::future::ready(Some(HoverContent {
            text: DOCS.into(),
            range: None,
        })))
    }
}

fn editor(cx: &mut TestAppContext) -> (Entity<EditorView>, &mut VisualTestContext) {
    cx.update(|cx| {
        init(cx);
        PaintedHoverCard::set(cx);
    });
    let theme = ThemeData::from_file(&themes::find("yohen").expect("Yohen ships").file());
    let (palette, _) = EditorPalette::for_theme(&theme);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(TEXT), Some(PathBuf::from("/w/main.rs")));
        let caret = TEXT.find("new").unwrap() + 1;
        model.set_selections(vec![ResolvedSelection {
            range: caret..caret,
            reversed: false,
        }]);
        let theme = EditorTheme::from_palette(&palette, &EditorSettings::default(), cx);
        let mut view = EditorView::new(model, theme, cx);
        view.set_hover_provider(Some(Rc::new(Provider)));
        view
    });
    cx.update(|window, cx| {
        window.activate_window();
        window.focus(&view.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (view, cx)
}

fn k(cx: &mut VisualTestContext) {
    cx.simulate_keystrokes("cmd-k cmd-i");
    cx.run_until_parked();
}

fn shown(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    // Draw a frame: a stale card is dropped by it.
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    view.read_with(cx, |view, _| view.popover_text()).is_some()
}

fn pinned(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> bool {
    // The pin glyph is the footer's.
    cx.update(|_, cx| PaintedHoverCard::take(cx));
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let glyph = kiln_l10n::tr!("gpui-hover-pinned").to_string();
    cx.update(|_, cx| PaintedHoverCard::take(cx))
        .iter()
        .any(|text| text.text == glyph)
}

#[gpui::test]
fn a_second_show_hover_pins_the_card(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    k(cx);
    assert!(shown(&view, cx));
    assert!(!pinned(&view, cx));
    // Unpinned, the caret moving off the line hides it.
    k(cx);
    assert!(shown(&view, cx));
    assert!(pinned(&view, cx));
}

#[gpui::test]
fn a_pinned_card_survives_pointer_leave_and_edits_elsewhere(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    k(cx);
    k(cx);
    cx.simulate_event(MouseExitEvent {
        position: point(px(900.), px(500.)),
        pressed_button: None,
        modifiers: Modifiers::none(),
    });
    cx.run_until_parked();
    assert!(shown(&view, cx));
    let caret = TEXT.find("new").unwrap() + 1;
    view.update(cx, |view, _| {
        view.model_mut().set_cursor(TEXT.len());
        view.model_mut().insert("x");
        view.model_mut().set_cursor(caret);
    });
    cx.run_until_parked();
    assert!(shown(&view, cx));
    // The caret leaving its line closes it.
    view.update(cx, |view, _| view.model_mut().set_cursor(0));
    assert!(!shown(&view, cx));
}

#[gpui::test]
fn escape_closes_it(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    k(cx);
    k(cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!shown(&view, cx));
}

#[gpui::test]
fn k_again_closes_it(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    k(cx);
    k(cx);
    k(cx);
    assert!(!shown(&view, cx));
}

#[gpui::test]
fn the_wheel_scrolls_and_text_selects_in_a_pinned_card(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx);
    k(cx);
    k(cx);
    let docs = cx.debug_bounds("hover-card-docs").expect("the docs");
    let over = docs.center();
    // The wheel over the card doesn't hide it.
    cx.simulate_event(ScrollWheelEvent {
        position: over,
        delta: ScrollDelta::Pixels(point(px(0.), px(-40.))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert!(shown(&view, cx));
    // A press selects the part, and copy copies it.
    cx.simulate_event(MouseDownEvent {
        position: over,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 1,
        first_mouse: false,
    });
    cx.run_until_parked();
    cx.write_to_clipboard(ClipboardItem::new_string(String::new()));
    cx.simulate_keystrokes("secondary-c");
    cx.run_until_parked();
    let copied = cx.read_from_clipboard().and_then(|item| item.text());
    assert_eq!(copied.as_deref(), Some(DOCS));
}
