// SPDX-License-Identifier: LicenseRef-Proprietary

//! TextInput takes IME composition: it stays out of `text()` and raises no
//! event until it commits, and its bounds follow the committed text.

use std::{cell::RefCell, rc::Rc};

use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler as _, IntoElement, ParentElement as _,
    Render, Styled as _, TestAppContext, Window, div,
};

use crate::{TextInput, TextInputEvent, UiTheme};

struct Host(Entity<TextInput>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().text_size(gpui::px(30.)).child(self.0.clone())
    }
}

type Events = Rc<RefCell<Vec<TextInputEvent>>>;

fn input_with<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<TextInput>, Events, &'a mut gpui::VisualTestContext) {
    let events: Events = Rc::default();
    let log = events.clone();
    let (host, cx) = cx.add_window_view(|_, cx| {
        let input = cx.new(|cx| TextInput::new("Find", &UiTheme::default(), cx));
        cx.subscribe(&input, move |_, _, event: &TextInputEvent, _| {
            log.borrow_mut().push(*event)
        })
        .detach();
        Host(input)
    });
    cx.run_until_parked();
    let input = host.read_with(cx, |host, _| host.0.clone());
    input.update(cx, |input, cx| input.set_text(text, cx));
    (input, events, cx)
}

#[gpui::test]
fn composition_is_not_text(cx: &mut TestAppContext) {
    let (input, events, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "に", None, window, cx);
        input.replace_and_mark_text_in_range(None, "日本", None, window, cx);
    });
    input.update_in(cx, |input, window, cx| {
        assert_eq!(input.text(), "ab");
        assert_eq!(input.marked_text_range(window, cx), Some(2..4));
        let mut actual = None;
        assert_eq!(
            input.text_for_range(0..4, &mut actual, window, cx),
            Some("ab日本".to_string())
        );
        assert_eq!(
            input.selected_text_range(false, window, cx).unwrap().range,
            4..4
        );
    });
    assert!(events.borrow().is_empty());
}

#[gpui::test]
fn commit_emits_once(cx: &mut TestAppContext) {
    let (input, events, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "にほんご", None, window, cx);
        input.replace_text_in_range(None, "日本語", window, cx);
        assert_eq!(input.text(), "ab日本語");
        assert_eq!(input.marked_text_range(window, cx), None);
    });
    assert_eq!(*events.borrow(), [TextInputEvent::Changed]);

    // unmark_text commits.
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "x", None, window, cx);
        input.unmark_text(window, cx);
        assert_eq!(input.text(), "ab日本語x");
        assert_eq!(input.marked_text_range(window, cx), None);
    });
    assert_eq!(events.borrow().len(), 2);

    // An empty mark ends the composition without an event.
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "y", None, window, cx);
        input.replace_and_mark_text_in_range(None, "", None, window, cx);
        assert_eq!(input.marked_text_range(window, cx), None);
        assert_eq!(input.text(), "ab日本語x");
    });
    assert_eq!(events.borrow().len(), 2);

    // set_text and clear drop it.
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "y", None, window, cx);
        input.set_text("q", cx);
        assert_eq!(input.marked_text_range(window, cx), None);
        input.replace_and_mark_text_in_range(None, "y", None, window, cx);
        input.clear(cx);
        assert_eq!(input.marked_text_range(window, cx), None);
        assert_eq!(input.text(), "");
    });
}

#[gpui::test]
fn bounds_follow_the_text(cx: &mut TestAppContext) {
    let (input, _, cx) = input_with(cx, "ab");
    let bounds = gpui::Bounds::new(
        gpui::point(gpui::px(10.), gpui::px(5.)),
        gpui::size(gpui::px(300.), gpui::px(30.)),
    );
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "日本", None, window, cx);
        let committed = input.bounds_for_range(0..2, bounds, window, cx).unwrap();
        let marked = input.bounds_for_range(2..4, bounds, window, cx).unwrap();
        assert!(marked.left() > bounds.left(), "{marked:?}");
        assert!(marked.left() > committed.left(), "{marked:?} {committed:?}");
        assert_eq!(marked.left(), committed.right());
    });
}

#[gpui::test]
fn bounds_use_the_inherited_font_size(cx: &mut TestAppContext) {
    let (input, _, cx) = input_with(cx, "ab");
    cx.run_until_parked();
    let bounds = gpui::Bounds::new(
        gpui::point(gpui::px(0.), gpui::px(0.)),
        gpui::size(gpui::px(300.), gpui::px(30.)),
    );
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "日本", None, window, cx);
        let marked = input.bounds_for_range(2..4, bounds, window, cx).unwrap();
        let style = window.text_style();
        let run = gpui::TextRun {
            len: 2,
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let width = |size| {
            window
                .text_system()
                .shape_line(
                    "ab".into(),
                    gpui::px(size),
                    std::slice::from_ref(&run),
                    None,
                )
                .width()
        };
        let padding = window.rem_size() * 0.75;
        assert_eq!(marked.left(), padding + width(30.));
    });
}

#[gpui::test]
fn editing_cancels_the_composition(cx: &mut TestAppContext) {
    let (input, events, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "日本", None, window, cx);
        input.insert("X", cx);
        assert_eq!(input.text(), "abX");
        assert_eq!(input.marked_text_range(window, cx), None);
        input.replace_and_mark_text_in_range(None, "日本", None, window, cx);
        input.backspace(cx);
        assert_eq!(input.text(), "ab");
        assert_eq!(input.marked_text_range(window, cx), None);
    });
    assert_eq!(events.borrow().len(), 2);
}

#[gpui::test]
fn replacement_ranges_are_read_in_combined_text(cx: &mut TestAppContext) {
    let (input, events, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "日本語", None, window, cx);
        // A subrange of the composition.
        input.replace_and_mark_text_in_range(Some(3..4), "x", None, window, cx);
        let mut actual = None;
        assert_eq!(input.marked_text_range(window, cx), Some(2..5));
        assert_eq!(
            input.text_for_range(0..5, &mut actual, window, cx),
            Some("ab日x".to_string() + "語")
        );
        // A range reaching into committed text deletes that part of it.
        input.replace_and_mark_text_in_range(Some(1..5), "z", None, window, cx);
        assert_eq!(input.text(), "a");
        assert_eq!(input.marked_text_range(window, cx), Some(1..2));
    });
    // The deletion under the composition reports when it ends.
    assert!(events.borrow().is_empty());
    input.update_in(cx, |input, window, cx| input.unmark_text(window, cx));
    assert_eq!(events.borrow().len(), 1);
}

#[gpui::test]
fn an_empty_mark_with_a_range_ends_it(cx: &mut TestAppContext) {
    let (input, events, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "日本語", None, window, cx);
        input.replace_and_mark_text_in_range(Some(3..4), "", None, window, cx);
        assert_eq!(input.marked_text_range(window, cx), None);
        assert_eq!(input.text(), "ab");
        input.set_text("", cx);
        input.replace_and_mark_text_in_range(None, "x", None, window, cx);
        input.backspace(cx);
        assert_eq!(input.marked_text_range(window, cx), None);
    });
    assert!(events.borrow().is_empty());
}

#[gpui::test]
fn an_empty_mark_reaching_committed_text_edits_nothing(cx: &mut TestAppContext) {
    let (input, events, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(None, "日本", None, window, cx);
        input.replace_and_mark_text_in_range(Some(1..4), "", None, window, cx);
        assert_eq!(input.text(), "ab");
        assert_eq!(input.marked_text_range(window, cx), None);
    });
    assert!(events.borrow().is_empty());
}

#[gpui::test]
fn clear_reports_text_a_composition_deleted(cx: &mut TestAppContext) {
    let (input, events, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        input.replace_and_mark_text_in_range(Some(0..2), "x", None, window, cx);
        assert_eq!(input.text(), "");
        input.clear(cx);
        assert_eq!(input.marked_text_range(window, cx), None);
    });
    assert_eq!(*events.borrow(), [TextInputEvent::Changed]);
}

#[gpui::test]
fn one_edit_emits_one_change(cx: &mut TestAppContext) {
    // Each caller drops a composition that deleted committed text, then
    // edits: one `Changed` for both.
    let mark = |input: &mut TextInput, window: &mut Window, cx: &mut Context<TextInput>| {
        input.replace_and_mark_text_in_range(Some(1..2), "x", None, window, cx);
    };
    let (input, events, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        mark(input, window, cx);
        input.backspace(cx);
        assert_eq!(input.text(), "");
    });
    assert_eq!(events.borrow().len(), 1);

    let (input, events, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        mark(input, window, cx);
        input.delete_word_left(cx);
        assert_eq!(input.text(), "");
    });
    assert_eq!(events.borrow().len(), 1);

    let (input, events, cx) = input_with(cx, "abc");
    input.update_in(cx, |input, window, cx| {
        mark(input, window, cx);
        // Not the marked range: replaces committed text.
        input.replace_text_in_range(Some(0..1), "z", window, cx);
        assert_eq!(input.text(), "zc");
    });
    assert_eq!(events.borrow().len(), 1);
}
