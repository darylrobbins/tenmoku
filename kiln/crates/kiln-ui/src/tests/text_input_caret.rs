// SPDX-License-Identifier: LicenseRef-Proprietary

//! TextInput's caret: it moves, it's drawn where text goes in, and edits
//! and an IME composition happen at it.

use gpui::{
    AppContext as _, Context, Entity, EntityInputHandler as _, Focusable as _, IntoElement,
    ParentElement as _, Render, Styled as _, TestAppContext, VisualTestContext, Window, div, px,
};

use crate::{TextInput, UiTheme};

struct Host(Entity<TextInput>);

impl Render for Host {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(px(300.)).child(self.0.clone())
    }
}

/// A focused input holding `text`, its caret at the end.
fn input_with<'a>(
    cx: &'a mut TestAppContext,
    text: &str,
) -> (Entity<TextInput>, &'a mut VisualTestContext) {
    let (host, cx) =
        cx.add_window_view(|_, cx| Host(cx.new(|cx| TextInput::new("", &UiTheme::default(), cx))));
    let input = host.read_with(cx, |host, _| host.0.clone());
    input.update_in(cx, |input, window, cx| {
        input.set_text(text, cx);
        window.focus(&input.focus_handle(cx), cx);
    });
    cx.run_until_parked();
    (input, cx)
}

/// The width of `text` shaped in the window's text style.
fn width(text: &str, cx: &mut VisualTestContext) -> gpui::Pixels {
    cx.update(|window, _| {
        let style = window.text_style();
        let run = gpui::TextRun {
            len: text.len(),
            font: style.font(),
            color: style.color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(text.to_owned().into(), size, &[run], None);
        line.width()
    })
}

#[gpui::test]
fn caret_is_drawn_at_the_insertion_point(cx: &mut TestAppContext) {
    let (input, cx) = input_with(cx, "ab");
    let text = |cx: &mut VisualTestContext| cx.debug_bounds("text-input-text").expect("drawn");
    let shown = text(cx);
    input.update(cx, |input, cx| input.move_left(cx));
    cx.run_until_parked();
    // Moving the caret leaves the text as it was laid out.
    assert_eq!(text(cx), shown);
    let (caret, bounds) = input.read_with(cx, |input, _| input.painted_caret.expect("painted"));
    let padding = cx.update(|window, _| window.rem_size() * 0.75);
    let (a, ab) = (width("a", cx), width("ab", cx));
    // Layout rounds to whole pixels.
    let start = bounds.left() + padding;
    let (after_a, before_b) = (start + a - px(1.), start + ab - px(1.));
    assert!(after_a < before_b);
    let at = caret.left();
    assert!(after_a <= at && at < before_b, "{at:?} between a and b");
    // Where the platform is told the caret is.
    let told = input.update_in(cx, |input, window, cx| {
        input.bounds_for_range(1..1, bounds, window, cx).unwrap()
    });
    assert!((told.left() - at).abs() < px(1.), "{told:?} {caret:?}");

    input.update(cx, |input, cx| {
        input.insert("X", cx);
        assert_eq!(input.text(), "aXb");
        assert_eq!(input.caret(), 2);
        // A selection gives way to Left, which goes to its start.
        input.select_all(cx);
        input.move_left(cx);
        assert!(!input.selected());
        assert_eq!(input.caret(), 0);
        input.insert("Y", cx);
        assert_eq!(input.text(), "YaXb");
        input.select_all(cx);
        input.move_right(cx);
        assert_eq!(input.caret(), 4);
    });
}

#[gpui::test]
fn edits_and_motions_follow_graphemes_and_words(cx: &mut TestAppContext) {
    let (input, cx) = input_with(cx, "e\u{301}x foo/bar");
    input.update(cx, |input, cx| {
        input.move_word_left(cx);
        assert_eq!(input.caret(), "e\u{301}x foo/".len());
        input.delete_word_left(cx);
        assert_eq!(input.text(), "e\u{301}x foobar");
        input.move_home(cx);
        input.move_right(cx);
        assert_eq!(input.caret(), "e\u{301}".len(), "one grapheme");
        input.backspace(cx);
        assert_eq!(input.text(), "x foobar");
        input.delete_forward(cx);
        assert_eq!(input.text(), " foobar");
        input.move_word_right(cx);
        assert_eq!(input.caret(), " foobar".len());
        input.move_end(cx);
        input.delete_forward(cx);
        assert_eq!(input.text(), " foobar", "nothing after the end");
    });
}

#[gpui::test]
fn ime_composition_at_the_caret(cx: &mut TestAppContext) {
    let (input, cx) = input_with(cx, "ab");
    input.update_in(cx, |input, window, cx| {
        input.move_left(cx);
        input.replace_and_mark_text_in_range(None, "に", None, window, cx);
        assert_eq!(input.marked_text_range(window, cx), Some(1..2));
        assert_eq!(
            input.selected_text_range(false, window, cx).unwrap().range,
            2..2
        );
        let mut actual = None;
        assert_eq!(
            input.text_for_range(0..3, &mut actual, window, cx),
            Some("aにb".to_owned())
        );
        input.replace_text_in_range(None, "日", window, cx);
        assert_eq!(input.text(), "a日b");
        assert_eq!(input.caret(), "a日".len());

        // A composition replacing the grapheme before the caret, as an
        // accent menu's does, and the text after it, as the platform may.
        input.replace_and_mark_text_in_range(Some(1..2), "本", None, window, cx);
        assert_eq!(input.text(), "ab");
        assert_eq!(input.marked_text_range(window, cx), Some(1..2));
        input.replace_and_mark_text_in_range(Some(1..3), "語", None, window, cx);
        assert_eq!(input.text(), "a");
        input.unmark_text(window, cx);
        assert_eq!(input.text(), "a語");

        // A platform replacement of committed text away from the caret.
        input.move_home(cx);
        input.replace_text_in_range(Some(1..2), "z", window, cx);
        assert_eq!(input.text(), "az");
        assert_eq!(input.caret(), 2);
    });
}

#[gpui::test]
fn edits_that_join_graphemes_leave_the_caret_on_a_boundary(cx: &mut TestAppContext) {
    let (woman, laptop, zwj) = ("\u{1F469}", "\u{1F4BB}", "\u{200D}");
    let (input, cx) = input_with(cx, &format!("{woman}{laptop}"));
    input.update_in(cx, |input, window, cx| {
        // A joiner typed between them makes one grapheme: the caret goes
        // after it, and Backspace deletes all of it.
        input.move_left(cx);
        input.insert(zwj, cx);
        assert_eq!(input.caret(), input.text().len());
        input.backspace(cx);
        assert_eq!(input.text(), "");

        // So does a composition committing one, and a deletion that leaves
        // a combining mark on the letter before it.
        input.set_text(format!("{woman}{laptop}"), cx);
        input.move_left(cx);
        input.replace_and_mark_text_in_range(None, zwj, None, window, cx);
        input.unmark_text(window, cx);
        assert_eq!(input.caret(), input.text().len());
        // A tab takes no combining mark; with it gone, the `e` does.
        input.set_text("e\t\u{301}", cx);
        input.move_left(cx);
        input.backspace(cx);
        assert_eq!(input.text(), "e\u{301}");
        assert_eq!(input.caret(), input.text().len());
    });
}

#[gpui::test]
fn a_composition_goes_where_its_range_starts(cx: &mut TestAppContext) {
    let (input, cx) = input_with(cx, "abcd");
    input.update_in(cx, |input, window, cx| {
        // Wholly after the caret.
        input.move_home(cx);
        input.move_right(cx);
        input.replace_and_mark_text_in_range(Some(3..4), "X", None, window, cx);
        assert_eq!(input.marked_text_range(window, cx), Some(3..4));
        input.unmark_text(window, cx);
        assert_eq!(input.text(), "abcX");

        // Empty, after the caret, and after an open composition, which
        // stays as text.
        input.move_home(cx);
        input.replace_and_mark_text_in_range(None, "Y", None, window, cx);
        input.replace_and_mark_text_in_range(Some(3..3), "Z", None, window, cx);
        input.unmark_text(window, cx);
        assert_eq!(input.text(), "YabZcX");
    });
}

#[gpui::test]
fn a_crlf_set_is_one_step(cx: &mut TestAppContext) {
    // A host's text keeps its line breaks, as a replacement's pasted
    // carriage return does; the caret steps over a CRLF at once.
    let (input, cx) = input_with(cx, "a\r\nb");
    input.update(cx, |input, cx| {
        assert_eq!(input.text(), "a\r\nb");
        input.move_left(cx);
        assert_eq!(input.caret(), 3);
        input.move_left(cx);
        assert_eq!(input.caret(), 1);
        input.move_right(cx);
        assert_eq!(input.caret(), 3);
        input.move_end(cx);
        assert_eq!(input.caret(), 4);
        input.move_left(cx);
        input.backspace(cx);
        assert_eq!(input.text(), "ab");
    });
}

#[gpui::test]
fn a_dropped_composition_leaves_the_caret_on_a_boundary(cx: &mut TestAppContext) {
    // A composition over the tab joins the `e` and the accent around it;
    // however it's dropped, Backspace then deletes the whole `é`.
    let (input, cx) = input_with(cx, "e\t\u{301}");
    let drops: [fn(&mut TextInput, &mut Window, &mut Context<TextInput>); 3] = [
        |input, window, cx| input.replace_and_mark_text_in_range(None, "", None, window, cx),
        |input, _, cx| input.set_typing(false, cx),
        |input, _, cx| input.move_end(cx),
    ];
    for (ix, drop) in drops.into_iter().enumerate() {
        input.update_in(cx, |input, window, cx| {
            input.set_typing(true, cx);
            input.set_text("e\t\u{301}", cx);
            input.replace_and_mark_text_in_range(Some(1..2), "X", None, window, cx);
            drop(input, window, cx);
            assert_eq!(input.text(), "e\u{301}", "{ix}");
            assert_eq!(input.caret(), input.text().len(), "{ix}");
            input.backspace(cx);
            assert_eq!(input.text(), "", "{ix}");
        });
    }
}

#[gpui::test]
fn a_composition_before_an_open_one_leaves_that_one_in_place(cx: &mut TestAppContext) {
    let (input, cx) = input_with(cx, "abcd");
    input.update_in(cx, |input, window, cx| {
        for (range, expected) in [(0..1, "XbcYd"), (1..1, "aXbcYd")] {
            input.set_text("abcd", cx);
            input.move_left(cx);
            input.replace_and_mark_text_in_range(None, "Y", None, window, cx);
            input.replace_and_mark_text_in_range(Some(range), "X", None, window, cx);
            input.unmark_text(window, cx);
            assert_eq!(input.text(), expected);
        }
    });
}

#[gpui::test]
fn a_combining_mark_after_a_line_break_is_its_own_step(cx: &mut TestAppContext) {
    let (input, cx) = input_with(cx, "");
    input.update(cx, |input, cx| {
        for brk in ["\n", "\r", "\r\n"] {
            let text = format!("a{brk}\u{301}b");
            input.set_text(text.clone(), cx);
            input.move_home(cx);
            input.move_right(cx);
            input.move_right(cx);
            assert_eq!(input.caret(), 1 + brk.len(), "{brk:?}");
            input.move_right(cx);
            assert_eq!(input.caret(), text.len() - 1, "{brk:?}");
            input.backspace(cx);
            assert_eq!(input.text(), format!("a{brk}b"), "{brk:?}");
        }
    });
}
