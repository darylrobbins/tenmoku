// SPDX-License-Identifier: LicenseRef-Proprietary

//! TextField in a window: typed text through the platform input handler,
//! the editing keys through its actions, the mouse, the clipboard, IME
//! composition, scrolling and the events a host subscribes to.

use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, EntityInputHandler as _, FocusHandle,
    Focusable as _, InteractiveElement as _, IntoElement, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement as _, Pixels, Point, Render, Styled as _,
    Subscription, TestAppContext, VisualTestContext, Window, div, point, px,
};

use crate::{TextField, TextFieldEvent};

/// A window holding a field, recording its events.
struct Host {
    field: Entity<TextField>,
    other: FocusHandle,
    events: Vec<TextFieldEvent>,
    _events: Subscription,
}

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .w(px(120.))
            .child(self.field.clone())
            .child(div().track_focus(&self.other).h(px(20.)))
    }
}

/// A focused field `text` in a window, with the PC key bindings (or the
/// Mac's), its focus event already recorded.
fn open<'a>(
    text: &str,
    macos: bool,
    cx: &'a mut TestAppContext,
) -> (Entity<Host>, &'a mut VisualTestContext) {
    cx.update(|cx| cx.bind_keys(TextField::key_bindings(macos)));
    let text = text.to_string();
    let (host, cx) = cx.add_window_view(move |window, cx| {
        let field = cx.new(|cx| TextField::new(window, cx).placeholder("Find").text(&text));
        let events = cx.subscribe_in(&field, window, |host: &mut Host, _, event, _, _| {
            host.events.push(*event);
        });
        Host {
            field,
            other: cx.focus_handle(),
            events: Vec::new(),
            _events: events,
        }
    });
    cx.update(|window, cx| {
        // Focus events fire only in the active window.
        window.activate_window();
        let focus = host.read(cx).field.focus_handle(cx);
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    (host, cx)
}

fn field(host: &Entity<Host>, cx: &mut VisualTestContext) -> Entity<TextField> {
    host.read_with(cx, |host, _| host.field.clone())
}

fn value(host: &Entity<Host>, cx: &mut VisualTestContext) -> String {
    let field = field(host, cx);
    field.read_with(cx, |field, _| field.value().to_string())
}

fn selected(host: &Entity<Host>, cx: &mut VisualTestContext) -> String {
    let field = field(host, cx);
    field.read_with(cx, |field, _| {
        field.value()[field.selected_range()].to_string()
    })
}

fn caret(host: &Entity<Host>, cx: &mut VisualTestContext) -> usize {
    let field = field(host, cx);
    field.read_with(cx, |field, _| field.model().caret())
}

fn take_events(host: &Entity<Host>, cx: &mut VisualTestContext) -> Vec<TextFieldEvent> {
    host.update(cx, |host, _| std::mem::take(&mut host.events))
}

/// The window point of byte `offset`, mid-line, by the last frame.
fn point_at(host: &Entity<Host>, offset: usize, cx: &mut VisualTestContext) -> Point<Pixels> {
    let field = field(host, cx);
    field.read_with(cx, |field, _| {
        field.point_for_offset(offset).expect("drawn")
    })
}

fn mouse_down(position: Point<Pixels>, clicks: usize, modifiers: Modifiers) -> MouseDownEvent {
    MouseDownEvent {
        position,
        modifiers,
        button: MouseButton::Left,
        click_count: clicks,
        first_mouse: false,
    }
}

fn mouse_up(position: Point<Pixels>, clicks: usize) -> MouseUpEvent {
    MouseUpEvent {
        position,
        modifiers: Modifiers::none(),
        button: MouseButton::Left,
        click_count: clicks,
    }
}

#[gpui::test]
fn typing_goes_through_the_input_handler(cx: &mut TestAppContext) {
    let (host, cx) = open("", false, cx);
    assert_eq!(take_events(&host, cx), [TextFieldEvent::Focused]);
    cx.simulate_input("héllo");
    assert_eq!(value(&host, cx), "héllo");
    assert_eq!(take_events(&host, cx), [TextFieldEvent::Changed; 5]);
    cx.simulate_keystrokes("left left");
    cx.simulate_input("X");
    assert_eq!(value(&host, cx), "hélXlo");
    assert_eq!(caret(&host, cx), 5);
}

#[gpui::test]
fn the_editing_keys_move_select_and_delete(cx: &mut TestAppContext) {
    let (host, cx) = open("one two three", false, cx);
    cx.simulate_keystrokes("ctrl-left");
    assert_eq!(caret(&host, cx), 8);
    cx.simulate_keystrokes("ctrl-shift-left");
    assert_eq!(selected(&host, cx), "two ");
    // Typing replaces the selection.
    cx.simulate_input("2");
    assert_eq!(value(&host, cx), "one 2three");
    cx.simulate_keystrokes("ctrl-backspace");
    assert_eq!(value(&host, cx), "one three");
    cx.simulate_keystrokes("delete delete");
    assert_eq!(value(&host, cx), "one ree");
    cx.simulate_keystrokes("end shift-left shift-left");
    assert_eq!(selected(&host, cx), "ee");
    cx.simulate_keystrokes("home shift-end");
    assert_eq!(selected(&host, cx), "one ree");
    cx.simulate_keystrokes("backspace");
    assert_eq!(value(&host, cx), "");
    // Nothing to delete: no event.
    take_events(&host, cx);
    cx.simulate_keystrokes("backspace");
    assert_eq!(take_events(&host, cx), []);
}

#[gpui::test]
fn the_mac_bindings_use_alt_and_cmd(cx: &mut TestAppContext) {
    let (host, cx) = open("one two", true, cx);
    cx.simulate_keystrokes("alt-left");
    assert_eq!(caret(&host, cx), 4);
    cx.simulate_keystrokes("cmd-shift-right");
    assert_eq!(selected(&host, cx), "two");
    cx.simulate_keystrokes("cmd-left alt-backspace");
    assert_eq!(value(&host, cx), "one two");
    cx.simulate_keystrokes("cmd-right alt-backspace");
    assert_eq!(value(&host, cx), "one ");
}

#[gpui::test]
fn undo_coalesces_typing(cx: &mut TestAppContext) {
    let (host, cx) = open("", false, cx);
    cx.simulate_input("abc");
    cx.simulate_keystrokes("home");
    cx.simulate_input("x");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(value(&host, cx), "abc");
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(value(&host, cx), "");
    take_events(&host, cx);
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(take_events(&host, cx), []);
    cx.simulate_keystrokes("secondary-shift-z ctrl-y");
    assert_eq!(value(&host, cx), "xabc");
    assert_eq!(take_events(&host, cx), [TextFieldEvent::Changed; 2]);
}

#[gpui::test]
fn enter_submits(cx: &mut TestAppContext) {
    let (host, cx) = open("query", false, cx);
    take_events(&host, cx);
    cx.simulate_keystrokes("enter");
    assert_eq!(take_events(&host, cx), [TextFieldEvent::Submitted]);
    assert_eq!(value(&host, cx), "query");
}

#[gpui::test]
fn copy_cut_and_paste_use_the_clipboard(cx: &mut TestAppContext) {
    let (host, cx) = open("alpha beta", false, cx);
    cx.simulate_keystrokes("ctrl-shift-left secondary-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("beta".into())
    );
    cx.simulate_keystrokes("secondary-a secondary-x");
    assert_eq!(value(&host, cx), "");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("alpha beta".into())
    );
    cx.write_to_clipboard(ClipboardItem::new_string("one\ntwo".into()));
    cx.simulate_keystrokes("secondary-v");
    assert_eq!(value(&host, cx), "one two");
    // A paste is its own undo step.
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(value(&host, cx), "");
}

#[gpui::test]
fn focus_and_blur_are_events(cx: &mut TestAppContext) {
    let (host, cx) = open("", false, cx);
    cx.update(|window, cx| {
        let other = host.read(cx).other.clone();
        window.focus(&other, cx);
    });
    cx.run_until_parked();
    assert_eq!(
        take_events(&host, cx),
        [TextFieldEvent::Focused, TextFieldEvent::Blurred]
    );
    // Typing goes nowhere now.
    cx.simulate_input("x");
    assert_eq!(value(&host, cx), "");
}

#[gpui::test]
fn set_value_is_not_a_change(cx: &mut TestAppContext) {
    let (host, cx) = open("", false, cx);
    take_events(&host, cx);
    let field = field(&host, cx);
    field.update(cx, |field, cx| field.set_value("new", cx));
    assert_eq!(value(&host, cx), "new");
    assert_eq!(take_events(&host, cx), []);
    // Its history is gone.
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(value(&host, cx), "new");
}

#[gpui::test]
fn an_input_method_composes_through_the_input_handler(cx: &mut TestAppContext) {
    let (host, cx) = open("😀", false, cx);
    let field = field(&host, cx);
    field.update_in(cx, |field, window, cx| {
        // UTF-16: the emoji is two code units.
        field.replace_and_mark_text_in_range(None, "n", None, window, cx);
        assert_eq!(field.marked_text_range(window, cx), Some(2..3));
        field.replace_and_mark_text_in_range(None, "ni", Some(2..2), window, cx);
        assert_eq!(field.marked_text_range(window, cx), Some(2..4));
        let selection = field.selected_text_range(false, window, cx).unwrap();
        assert_eq!(selection.range, 4..4);
        let mut actual = None;
        assert_eq!(
            field.text_for_range(0..4, &mut actual, window, cx),
            Some("😀ni".into())
        );
        assert_eq!(actual, Some(0..4));
        // A range splitting the emoji's surrogate pair starts after it.
        assert_eq!(
            field.text_for_range(1..4, &mut actual, window, cx),
            Some("ni".into())
        );
        assert_eq!(actual, Some(2..4));
        // The candidate is chosen.
        field.replace_text_in_range(None, "你", window, cx);
        assert_eq!(field.marked_text_range(window, cx), None);
    });
    assert_eq!(value(&host, cx), "😀你");
    assert_eq!(
        take_events(&host, cx),
        [
            TextFieldEvent::Focused,
            TextFieldEvent::Changed,
            TextFieldEvent::Changed,
            TextFieldEvent::Changed
        ]
    );
    // The composition is one undo step.
    cx.simulate_keystrokes("secondary-z");
    assert_eq!(value(&host, cx), "😀");

    // Bounds for the composition, for the candidate window.
    field.update_in(cx, |field, window, cx| {
        field.replace_and_mark_text_in_range(None, "k", None, window, cx);
    });
    cx.run_until_parked();
    let start = point_at(&host, 4, cx);
    let end = point_at(&host, 5, cx);
    field.update_in(cx, |field, window, cx| {
        let element = field.last_bounds().unwrap();
        let bounds = field.bounds_for_range(2..3, element, window, cx).unwrap();
        assert_eq!((bounds.left(), bounds.right()), (start.x, end.x));
        // UTF-16 index 2: after the emoji's two code units.
        assert_eq!(field.character_index_for_point(start, window, cx), Some(2));
        // Past the text, no character.
        let past = point(end.x + px(50.), start.y);
        assert_eq!(field.character_index_for_point(past, window, cx), None);
    });
}

#[gpui::test]
fn clicks_and_drags_select(cx: &mut TestAppContext) {
    let (host, cx) = open("foo bar.baz", false, cx);
    let at = |offset, cx: &mut VisualTestContext| point_at(&host, offset, cx);

    let p = at(2, cx);
    cx.simulate_click(p, Modifiers::none());
    assert_eq!(caret(&host, cx), 2);

    let p = at(9, cx);
    cx.simulate_click(p, Modifiers::shift());
    assert_eq!(selected(&host, cx), "o bar.b");

    // A drag from 4 to 7.
    let (from, to) = (at(4, cx), at(7, cx));
    cx.simulate_event(mouse_down(from, 1, Modifiers::none()));
    cx.simulate_event(MouseMoveEvent {
        position: to,
        modifiers: Modifiers::none(),
        pressed_button: Some(MouseButton::Left),
    });
    cx.simulate_event(mouse_up(to, 1));
    assert_eq!(selected(&host, cx), "bar");
    // Moving after the release selects nothing more.
    let past = at(11, cx);
    cx.simulate_mouse_move(past, None, Modifiers::none());
    assert_eq!(selected(&host, cx), "bar");

    let p = at(9, cx);
    cx.simulate_event(mouse_down(p, 2, Modifiers::none()));
    cx.simulate_event(mouse_up(p, 2));
    assert_eq!(selected(&host, cx), "baz");

    cx.simulate_event(mouse_down(p, 3, Modifiers::none()));
    cx.simulate_event(mouse_up(p, 3));
    assert_eq!(selected(&host, cx), "foo bar.baz");
}

#[gpui::test]
fn a_click_focuses_the_field(cx: &mut TestAppContext) {
    let (host, cx) = open("abc", false, cx);
    cx.update(|window, cx| {
        let other = host.read(cx).other.clone();
        window.focus(&other, cx);
    });
    cx.run_until_parked();
    take_events(&host, cx);
    let p = point_at(&host, 1, cx);
    cx.simulate_click(p, Modifiers::none());
    assert_eq!(take_events(&host, cx), [TextFieldEvent::Focused]);
    cx.simulate_input("x");
    assert_eq!(value(&host, cx), "axbc");
}

#[gpui::test]
fn the_text_scrolls_to_keep_the_caret_in_view(cx: &mut TestAppContext) {
    let long = "abcdefghij".repeat(10);
    let (host, cx) = open(&long, false, cx);
    let field = field(&host, cx);
    let in_view = |cx: &mut VisualTestContext| {
        field.read_with(cx, |field, _| {
            let bounds = field.last_bounds().unwrap();
            let caret = field.point_for_offset(field.model().caret()).unwrap();
            bounds.left() <= caret.x && caret.x <= bounds.right()
        })
    };
    // The caret starts at the end of a line wider than the field.
    let scroll = field.read_with(cx, |field, _| field.scroll_x());
    assert!(scroll > px(0.), "scrolled {scroll:?}");
    assert!(in_view(cx));

    cx.simulate_keystrokes("home");
    assert_eq!(field.read_with(cx, |field, _| field.scroll_x()), px(0.));
    assert!(in_view(cx));

    cx.simulate_keystrokes("end");
    assert_eq!(field.read_with(cx, |field, _| field.scroll_x()), scroll);
    // Deleting the text scrolls back.
    cx.simulate_keystrokes("secondary-a backspace");
    assert_eq!(field.read_with(cx, |field, _| field.scroll_x()), px(0.));
}
