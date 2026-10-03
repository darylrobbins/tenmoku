//! End-to-end tests of the editor view: real keystrokes go through the real
//! keymap and action dispatch into a focused `EditorView` in a headless
//! window, and the resulting text and selections are checked.
//!
//! Notation: `|` is a caret, `[…]` a selection whose head is at its end, and
//! `{…}` one whose head is at its start.

use std::collections::BTreeSet;

use fe_buffer::{Buffer, ResolvedSelection};
use fe_editor::EditorModel;
use gpui::{
    ClipboardItem, Entity, EntityInputHandler as _, Focusable as _, Keystroke, Modifiers,
    MouseButton, MouseDownEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext,
    VisualTestContext, point,
};

use crate::*;

fn parse(marked: &str) -> (String, Vec<ResolvedSelection>) {
    let mut text = String::new();
    let mut selections = Vec::new();
    let mut open = None;
    for ch in marked.chars() {
        match ch {
            '|' => selections.push(ResolvedSelection {
                range: text.len()..text.len(),
                reversed: false,
            }),
            '[' | '{' => open = Some(text.len()),
            ']' | '}' => selections.push(ResolvedSelection {
                range: open.take().expect("balanced")..text.len(),
                reversed: ch == '}',
            }),
            _ => text.push(ch),
        }
    }
    (text, selections)
}

fn render(model: &EditorModel) -> String {
    let text = model.text().to_string();
    let mut marks: Vec<(usize, u8, char)> = Vec::new();
    for selection in model.selections() {
        let range = selection.range;
        if range.is_empty() {
            marks.push((range.start, 1, '|'));
        } else if selection.reversed {
            marks.push((range.start, 2, '{'));
            marks.push((range.end, 0, '}'));
        } else {
            marks.push((range.start, 2, '['));
            marks.push((range.end, 0, ']'));
        }
    }
    marks.sort();
    let mut out = String::new();
    let mut last = 0;
    for (offset, _, mark) in marks {
        out.push_str(&text[last..offset]);
        out.push(mark);
        last = offset;
    }
    out.push_str(&text[last..]);
    out
}

/// A focused editor showing `marked`, in a headless window.
fn editor<'a>(
    cx: &'a mut TestAppContext,
    marked: &str,
) -> (Entity<EditorView>, &'a mut VisualTestContext) {
    cx.update(init);
    let (text, selections) = parse(marked);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_selections(selections);
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.run_until_parked();
    (view, cx)
}

fn state(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
    view.update(cx, |view, _| render(view.model()))
}

/// One binding, exercised from `before` by typing `keys` (which end with the
/// binding itself), expected to leave `after`.
#[derive(Clone, Copy)]
struct Case {
    binding: &'static str,
    keys: &'static str,
    before: &'static str,
    after: &'static str,
    clipboard_before: Option<&'static str>,
    clipboard_after: Option<&'static str>,
}

const fn case(binding: &'static str, before: &'static str, after: &'static str) -> Case {
    Case {
        binding,
        keys: binding,
        before,
        after,
        clipboard_before: None,
        clipboard_after: None,
    }
}

impl Case {
    const fn keys(self, keys: &'static str) -> Self {
        Case { keys, ..self }
    }

    const fn clipboard(self, before: Option<&'static str>, after: &'static str) -> Self {
        Case {
            clipboard_before: before,
            clipboard_after: Some(after),
            ..self
        }
    }
}

/// Every default binding. `bindings_are_all_tested` fails if one is missing.
const CASES: &[Case] = &[
    // Moving.
    case("left", "ab|c", "a|bc"),
    case("left", "a[bc]d", "a|bcd"),
    case("right", "a|bc", "ab|c"),
    case("right", "a[bc]d", "abc|d"),
    case("up", "abc\nd|e", "a|bc\nde"),
    case("down", "a|bc\nde", "abc\nd|e"),
    case("alt-left", "foo bar|", "foo |bar"),
    case("alt-right", "|foo bar", "foo| bar"),
    case("cmd-left", "  ab|c", "  |abc"),
    case("home", "  ab|c", "  |abc"),
    case("cmd-right", "a|bc\nd", "abc|\nd"),
    case("end", "a|bc\nd", "abc|\nd"),
    case("cmd-up", "ab\nc|d", "|ab\ncd"),
    case("cmd-down", "a|b\ncd", "ab\ncd|"),
    case("pageup", "a\nb\nc|", "|a\nb\nc"),
    case("pagedown", "a|\nb\nc", "a\nb\nc|"),
    // Selecting.
    case("shift-left", "ab|c", "a{b}c"),
    case("shift-right", "a|bc", "a[b]c"),
    case("shift-up", "abc\nd|e", "a{bc\nd}e"),
    case("shift-down", "a|bc\nde", "a[bc\nd]e"),
    case("alt-shift-left", "foo bar|", "foo {bar}"),
    case("alt-shift-right", "|foo bar", "[foo] bar"),
    case("cmd-shift-left", "  ab|c", "  {ab}c"),
    case("shift-home", "  ab|c", "  {ab}c"),
    case("cmd-shift-right", "a|bc\nd", "a[bc]\nd"),
    case("shift-end", "a|bc\nd", "a[bc]\nd"),
    case("cmd-shift-up", "ab\nc|d", "{ab\nc}d"),
    case("cmd-shift-down", "a|b\ncd", "a[b\ncd]"),
    case("shift-pageup", "a\nb\nc|", "{a\nb\nc}"),
    case("shift-pagedown", "a|\nb\nc", "a[\nb\nc]"),
    case("cmd-a", "a|b\nc", "[ab\nc]"),
    // Extending an existing selection keeps its tail.
    case("shift-right", "a[b]cd", "a[bc]d"),
    case("shift-left", "a[bc]d", "a[b]cd"),
    // Editing.
    case("backspace", "ab|c", "a|c"),
    case("backspace", "a[bc]d", "a|d"),
    case("shift-backspace", "ab|c", "a|c"),
    case("delete", "a|bc", "a|c"),
    case("alt-backspace", "foo bar|", "foo |"),
    case("enter", "  ab|c", "  ab\n  |c"),
    case("tab", "ab|", "ab  |"),
    // A caret folded away moves to the fold's header.
    case("cmd-alt-[", "a:\n  b|\nc", "a:|\n  b\nc"),
    // Every cursor gets the command.
    case("backspace", "a|b\nc|d", "|b\n|d"),
    case("alt-right", "|ab cd\n|ef gh", "ab| cd\nef| gh"),
    case("enter", "a|b|", "a\n|b\n|"),
    // Clipboard.
    case("cmd-c", "a[bc]d", "a[bc]d").clipboard(None, "bc"),
    case("cmd-c", "a|b\ncd", "a|b\ncd").clipboard(None, "ab\n"),
    case("cmd-x", "a[bc]d", "a|d").clipboard(None, "bc"),
    case("cmd-x", "a|b\ncd", "|cd").clipboard(None, "ab\n"),
    case("cmd-v", "a|d", "abc|d").clipboard(Some("bc"), "bc"),
    case("cmd-v", "a[xx]d", "abc|d").clipboard(Some("bc"), "bc"),
    case("cmd-v", "|a\n|b", "1|a\n2|b").clipboard(Some("1\n2"), "1\n2"),
    // History.
    case("cmd-z", "ab|c", "ab|c").keys("backspace cmd-z"),
    case("cmd-z", "ab|c", "ab|c").keys("x y cmd-z cmd-z"),
    case("cmd-shift-z", "ab|c", "a|c").keys("backspace cmd-z cmd-shift-z"),
];

/// Bindings exercised by their own tests rather than the table.
const DEDICATED: &[&str] = &["cmd-s", "cmd-alt-]", "alt-z"];

fn run_case(case: &Case) {
    let mut cx = TestAppContext::single();
    let (view, cx) = editor(&mut cx, case.before);
    if let Some(text) = case.clipboard_before {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_string()));
    }
    cx.simulate_keystrokes(case.keys);
    let after = state(&view, cx);
    assert_eq!(
        after, case.after,
        "`{}` from {:?} (keys {:?})",
        case.binding, case.before, case.keys
    );
    if let Some(expected) = case.clipboard_after {
        let clipboard = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(
            clipboard.as_deref(),
            Some(expected),
            "clipboard after `{}` from {:?}",
            case.binding,
            case.before
        );
    }
}

#[test]
fn every_binding_does_what_it_should() {
    for case in CASES {
        assert!(
            case.keys.ends_with(case.binding),
            "keys for `{}` must end with it",
            case.binding
        );
        run_case(case);
    }
}

/// Normalises a keystroke string so differently written forms compare equal.
fn normalized(keystroke: &str) -> String {
    Keystroke::parse(keystroke)
        .expect("valid keystroke")
        .unparse()
}

#[gpui::test]
fn bindings_are_all_tested(cx: &mut TestAppContext) {
    cx.update(init);
    let bound: BTreeSet<String> = cx.update(|cx| {
        cx.key_bindings()
            .borrow()
            .bindings()
            .filter(|binding| {
                binding
                    .predicate()
                    .is_some_and(|p| p.to_string() == KEY_CONTEXT)
            })
            .map(|binding| {
                binding
                    .keystrokes()
                    .iter()
                    .map(|keystroke| keystroke.unparse())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    });
    assert!(
        bound.len() >= 40,
        "found only {} editor bindings",
        bound.len()
    );
    let tested: BTreeSet<String> = CASES
        .iter()
        .map(|case| case.binding)
        .chain(DEDICATED.iter().copied())
        .map(normalized)
        .collect();
    let untested: Vec<_> = bound.difference(&tested).collect();
    assert!(untested.is_empty(), "bindings without a test: {untested:?}");
    let stale: Vec<_> = tested.difference(&bound).collect();
    assert!(
        stale.is_empty(),
        "tests for bindings that don't exist: {stale:?}"
    );
}

#[gpui::test]
fn unfold_shows_folded_rows(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "a|:\n  b\nc");
    let rows =
        |cx: &mut VisualTestContext| view.update(cx, |view, _| view.model().display().row_count());
    cx.simulate_keystrokes("cmd-alt-[");
    assert_eq!(rows(cx), 2);
    cx.simulate_keystrokes("cmd-alt-]");
    assert_eq!(rows(cx), 3);
}

#[gpui::test]
fn soft_wrap_wraps_to_the_window(cx: &mut TestAppContext) {
    let long = "word ".repeat(1000);
    let (view, cx) = editor(cx, &format!("|{long}\nend"));
    let rows =
        |cx: &mut VisualTestContext| view.update(cx, |view, _| view.model().display().row_count());
    cx.simulate_keystrokes("alt-z");
    assert!(rows(cx) > 10, "wrapped into {} rows", rows(cx));
    // Down moves within the wrapped row.
    cx.simulate_keystrokes("down");
    let head = view.update(cx, |view, _| view.model().primary_selection().head());
    assert!(head > 0 && head < long.len(), "head at {head}");
    cx.simulate_keystrokes("alt-z");
    assert_eq!(rows(cx), 2);
}

#[gpui::test]
fn bindings_only_apply_to_a_focused_editor(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "ab|c");
    cx.update(|window, cx| window.blur(cx));
    cx.simulate_keystrokes("backspace left");
    assert_eq!(state(&view, cx), "ab|c");
}

#[gpui::test]
fn typing_goes_to_every_cursor(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "|a\n|b");
    cx.simulate_input("xé");
    assert_eq!(state(&view, cx), "xé|a\nxé|b");
}

#[gpui::test]
fn typing_replaces_the_selection(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "a[bc]d");
    cx.simulate_input("Z");
    assert_eq!(state(&view, cx), "aZ|d");
    cx.simulate_keystrokes("cmd-z");
    assert_eq!(state(&view, cx), "abc|d");
}

#[gpui::test]
fn input_method_composition(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "ab|");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            // Typing "ka" in a Japanese input method composes "か".
            view.replace_and_mark_text_in_range(None, "k", Some(1..1), window, cx);
            assert_eq!(view.marked_text_range(window, cx), Some(2..3));
            view.replace_and_mark_text_in_range(None, "か", Some(0..1), window, cx);
            assert_eq!(view.model().text().to_string(), "abか");
            assert_eq!(view.marked_text_range(window, cx), Some(2..3));
            // The selection goes where the input method asked, relative to the composition.
            let selection = view.selected_text_range(false, window, cx).unwrap();
            assert_eq!(selection.range, 2..3);
            // Choosing a candidate commits it in place of the composition.
            view.replace_text_in_range(None, "加", window, cx);
            assert_eq!(view.marked_text_range(window, cx), None);
        })
    });
    assert_eq!(state(&view, cx), "ab加|");

    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            // The OS asks about text using UTF-16 offsets.
            let mut actual = None;
            let text = view.text_for_range(0..3, &mut actual, window, cx);
            assert_eq!(text.as_deref(), Some("ab加"));
            assert_eq!(actual, Some(0..3));
            let selection = view.selected_text_range(false, window, cx).unwrap();
            assert_eq!(selection.range, 3..3);
            // Positions on screen exist for visible text.
            let bounds = view.bounds_for_range(0..1, Default::default(), window, cx);
            assert!(bounds.is_some());
            // Unmarking keeps the composed text but ends the composition.
            view.replace_and_mark_text_in_range(None, "k", None, window, cx);
            view.unmark_text(window, cx);
            assert_eq!(view.marked_text_range(window, cx), None);
            assert_eq!(view.model().text().to_string(), "ab加k");
        })
    });
}

#[gpui::test]
fn input_method_utf16_offsets_with_emoji(cx: &mut TestAppContext) {
    // '😀' is two UTF-16 code units; the OS addresses text in those.
    let (view, cx) = editor(cx, "😀x|");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let selection = view.selected_text_range(false, window, cx).unwrap();
            assert_eq!(selection.range, 3..3);
            view.replace_text_in_range(Some(2..3), "y", window, cx);
        })
    });
    assert_eq!(state(&view, cx), "😀y|");
}

#[gpui::test]
fn frames_are_timed(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "fn main() {}|");
    let frames = view.update(cx, |view, _| view.frame_timings().frames().len());
    assert!(frames > 0);
}

#[gpui::test]
fn page_moves_by_the_visible_rows(cx: &mut TestAppContext) {
    let text: String = (0..300).map(|row| format!("row {row}\n")).collect();
    let (view, cx) = editor(cx, &format!("|{text}"));
    let visible = view.update(cx, |view, _| {
        view.layout.as_ref().unwrap().frame.visible_rows()
    });
    assert!(visible > 3);
    cx.simulate_keystrokes("pagedown");
    let row = view.update(cx, |view, _| {
        let head = view.model().primary_selection().head();
        view.model().text().offset_to_point(head).row
    });
    // A page keeps a little overlap with the previous one.
    assert_eq!(row, visible - 2);
}

/// The window position of the caret at `offset`, as reported to input methods.
fn position_of(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    offset: usize,
) -> Point<Pixels> {
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            let utf16 = view.model().text().offset_to_utf16(offset);
            view.bounds_for_range(utf16..utf16, Default::default(), window, cx)
                .expect("visible")
                .center()
        })
    })
}

fn double_click(cx: &mut VisualTestContext, position: Point<Pixels>) {
    cx.simulate_event(MouseDownEvent {
        position,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 2,
        first_mouse: false,
    });
}

fn drag(cx: &mut VisualTestContext, from: Point<Pixels>, to: Point<Pixels>) {
    cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
}

#[gpui::test]
fn mouse_clicks_and_drags_select(cx: &mut TestAppContext) {
    let (view, cx) = editor(cx, "|one two\nthree four\nfive");
    let at = |offset, cx: &mut VisualTestContext| position_of(&view, cx, offset);

    let p = at(5, cx);
    cx.simulate_click(p, Modifiers::none());
    assert_eq!(state(&view, cx), "one t|wo\nthree four\nfive");
    cx.update(|window, cx| {
        let index = view.update(cx, |view, cx| view.character_index_for_point(p, window, cx));
        assert_eq!(index, Some(5));
    });
    let p = at(12, cx);
    cx.simulate_click(p, Modifiers::shift());
    assert_eq!(state(&view, cx), "one t[wo\nthre]e four\nfive");

    let (from, to) = (at(12, cx), at(1, cx));
    drag(cx, from, to);
    assert_eq!(state(&view, cx), "o{ne two\nthre}e four\nfive");

    // Double-click selects a word; dragging grows it a word at a time.
    let (from, to) = (at(5, cx), at(16, cx));
    double_click(cx, from);
    assert_eq!(state(&view, cx), "one [two]\nthree four\nfive");
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    assert_eq!(state(&view, cx), "one [two\nthree four]\nfive");
    let (from, to) = (at(16, cx), at(1, cx));
    double_click(cx, from);
    cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    assert_eq!(state(&view, cx), "{one two\nthree four}\nfive");

    // Triple-click selects the row.
    let p = at(10, cx);
    cx.simulate_event(MouseDownEvent {
        position: p,
        button: MouseButton::Left,
        modifiers: Modifiers::none(),
        click_count: 3,
        first_mouse: false,
    });
    assert_eq!(state(&view, cx), "one two\n[three four\n]five");
}

#[gpui::test]
fn scrolling_moves_what_a_click_hits(cx: &mut TestAppContext) {
    let text: String = (0..300).map(|row| format!("row {row}\n")).collect();
    let (view, cx) = editor(cx, &format!("|{text}"));
    // The start of the third visible row.
    let target = position_of(&view, cx, "row 0\nrow 1\n".len());
    let scroll = |lines: f32, cx: &mut VisualTestContext| {
        cx.simulate_event(ScrollWheelEvent {
            position: target,
            delta: ScrollDelta::Lines(point(0., lines)),
            ..Default::default()
        });
    };
    let clicked_row = |cx: &mut VisualTestContext| {
        cx.simulate_click(target, Modifiers::none());
        view.update(cx, |view, _| {
            let head = view.model().primary_selection().head();
            view.model().text().offset_to_point(head).row
        })
    };

    // Wheel deltas are negative when scrolling down.
    scroll(-3., cx);
    let row_5 = text.find("row 5").unwrap();
    assert_eq!(
        position_of(&view, cx, row_5),
        target,
        "caret boxes move with the scroll"
    );
    assert_eq!(clicked_row(cx), 5);
    scroll(10., cx);
    assert_eq!(clicked_row(cx), 2, "can't scroll above the start");
}

#[gpui::test]
fn the_caret_stays_in_view(cx: &mut TestAppContext) {
    let text: String = (0..300).map(|row| format!("row {row}\n")).collect();
    let long_row = "x".repeat(500);
    let (view, cx) = editor(cx, &format!("|{text}{long_row}"));
    cx.simulate_keystrokes("cmd-down");
    let caret = position_of(&view, cx, text.len() + long_row.len());
    let (bounds, line_height) = view.update(cx, |view, _| {
        let layout = view.layout.as_ref().unwrap();
        let frame = &layout.frame;
        let size = gpui::size(gpui::px(frame.width), gpui::px(frame.height));
        (
            gpui::Bounds::new(layout.origin, size),
            gpui::px(frame.line_height),
        )
    });
    assert!(bounds.contains(&caret), "{caret:?} outside {bounds:?}");
    // A few rows of context stay below the caret.
    assert!(caret.y + line_height * 2. < bounds.bottom());
}

#[gpui::test]
fn save_writes_the_file(cx: &mut TestAppContext) {
    let dir = std::env::temp_dir().join(format!("fastedit-gpui-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("file.txt");
    std::fs::write(&path, "one\r\n").unwrap();

    cx.update(init);
    let open = path.clone();
    let (view, cx) = cx.add_window_view(move |_, cx| {
        EditorView::new(
            EditorModel::open(&open).unwrap(),
            EditorTheme::default_dark(cx),
            cx,
        )
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    cx.simulate_keystrokes("cmd-right");
    cx.simulate_input("!");
    assert!(view.update(cx, |view, _| view.model().is_dirty()));
    cx.simulate_keystrokes("cmd-s");
    assert!(!view.update(cx, |view, _| view.model().is_dirty()));
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "one!\r\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The first parse of a large file runs in the background; later edits keep
/// the tree current.
#[gpui::test]
fn highlighting_follows_edits(cx: &mut TestAppContext) {
    cx.update(init);
    let text = "fn f() {}\n".repeat(20_000);
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(&text), Some("main.rs".into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.update(|window, cx| window.focus(&view.focus_handle(cx), cx));
    let first_keyword = |cx: &mut VisualTestContext| {
        cx.run_until_parked();
        view.update(cx, |view, _| {
            let syntax = view.syntax.as_ref().expect("language loaded");
            assert!(!syntax.needs_parse());
            let spans = syntax.highlights(0..40);
            spans
                .into_iter()
                .find(|span| span.highlight.name() == "keyword")
                .map(|span| span.range)
        })
    };
    assert_eq!(first_keyword(cx), Some(0..2));
    cx.simulate_input("// ");
    assert_eq!(first_keyword(cx), Some(13..15));
}
