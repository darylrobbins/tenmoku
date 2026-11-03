//! End-to-end tests of the editor view: real keystrokes go through the real
//! keymap and action dispatch into a focused `EditorView` in a headless
//! window, and the resulting text and selections are checked.
//!
//! Notation: `|` is a caret, `[…]` a selection whose head is at its end, and
//! `{…}` one whose head is at its start.

use std::{cell::Cell, collections::BTreeSet, rc::Rc};

use fe_buffer::{Buffer, ResolvedSelection};
use fe_display::{CharWidth, WRAP_SLICE_BYTES};
use fe_editor::EditorModel;
use gpui::{
    ClipboardItem, Entity, EntityInputHandler as _, Focusable as _, Keystroke, Modifiers,
    MouseButton, MouseDownEvent, Pixels, Point, ScrollDelta, ScrollWheelEvent, TestAppContext,
    VisualTestContext, point,
};

use crate::*;

mod vim_visual_keys;

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
pub(crate) fn editor<'a>(
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

pub(crate) fn state(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> String {
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
    /// The folds expected afterwards, as `(header, last)` rows.
    folds: Option<&'static [(u32, u32)]>,
}

const fn case(binding: &'static str, before: &'static str, after: &'static str) -> Case {
    Case {
        binding,
        keys: binding,
        before,
        after,
        clipboard_before: None,
        clipboard_after: None,
        folds: None,
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

    const fn folds(self, folds: &'static [(u32, u32)]) -> Self {
        Case {
            folds: Some(folds),
            ..self
        }
    }
}

/// Blocks seven deep, the caret below them all.
const DEEP: &str = "a\n b\n  c\n   d\n    e\n     f\n      g\n       h\nz|";

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
    case("ctrl-alt-left", "fooBar|", "foo|Bar"),
    case("ctrl-alt-right", "|fooBar", "foo|Bar"),
    case("ctrl-alt-shift-left", "fooBar|", "foo{Bar}"),
    case("ctrl-alt-shift-right", "|fooBar", "[foo]Bar"),
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
    case("cmd-alt-[", "a:\n  b|\nc", "a:|\n  b\nc").folds(&[(0, 1)]),
    // Plain text folds every indented block.
    case("cmd-k cmd-0", "a:\n  b|\nc:\n  d", "a:|\n  b\nc:\n  d").folds(&[(0, 1), (2, 3)]),
    case("cmd-k cmd-j", "a:\n  b|\nc:\n  d", "a:|\n  b\nc:\n  d")
        .keys("cmd-k cmd-0 cmd-k cmd-j")
        .folds(&[]),
    // A level folds the blocks that many deep, as indentation nests them.
    case("cmd-k cmd-1", DEEP, DEEP).folds(&[(0, 7)]),
    case("cmd-k cmd-2", DEEP, DEEP).folds(&[(1, 7)]),
    case("cmd-k cmd-3", DEEP, DEEP).folds(&[(2, 7)]),
    case("cmd-k cmd-4", DEEP, DEEP).folds(&[(3, 7)]),
    case("cmd-k cmd-5", DEEP, DEEP).folds(&[(4, 7)]),
    case("cmd-k cmd-6", DEEP, DEEP).folds(&[(5, 7)]),
    case("cmd-k cmd-7", DEEP, DEEP).folds(&[(6, 7)]),
    case("cmd-k cmd-[", "a:|\n  b:\n    c\nd", "a:|\n  b:\n    c\nd").folds(&[(0, 2), (1, 2)]),
    case("cmd-k cmd-]", "a:|\n  b:\n    c\nd", "a:|\n  b:\n    c\nd")
        .keys("cmd-k cmd-[ cmd-k cmd-]")
        .folds(&[]),
    case("cmd-k cmd-x", "a  \nb\t\nc|", "a\nb\nc|"),
    case("cmd-k cmd-q", "a|\nb\nc", "ax|\nb\nc").keys("x down down cmd-k cmd-q"),
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

/// Bindings exercised by their own tests rather than the table. `cmd-k
/// cmd-i` (`editor.show_hover`) is `view::hover_tests::show_hover_command`'s,
/// and `ctrl-space` (`editor.trigger_completion`) is
/// `view::completion_tests::menu_opens_narrows_and_accepts`'. The row and
/// comment keys (`cmd-]`, `cmd-[`, `cmd-shift-k`, `cmd-/`, `cmd-shift-\`) are checked to resolve to their commands by
/// fe-keymap's `editing_bindings`, and the language-server keys (`f2`,
/// `cmd-.`, `cmd-k cmd-f`, `cmd-shift-space`) by its
/// `lsp_bindings`. The git keys (`alt-f3`, `shift-alt-f3`, `cmd-alt-z`) are
/// checked by its `git_keys`, and `cmd-alt-g b` by its
/// `blame_annotation_keys`. The Markdown preview keys (`cmd-shift-v`,
/// `cmd-k v`) by its `markdown_preview_bindings`. Peek Definition's
/// `alt-f12` by its `peek_keys`, and Peek References' `shift-f12` by its
/// `peek_references_keys`.
const DEDICATED: &[&str] = &[
    "cmd-s",
    "cmd-alt-]",
    "alt-z",
    "cmd-k cmd-i",
    // diff.compare_saved: fe-keymap's compare_saved_key.
    "cmd-k d",
    // spell.suggest: fe-keymap's spell_bindings, and fe-workbench's spell_commands_tests.
    "cmd-alt-.",
    "ctrl-space",
    "cmd-]",
    "cmd-[",
    "cmd-shift-k",
    "cmd-/",
    "cmd-shift-\\",
    "f2",
    "cmd-.",
    "shift-alt-o",
    "ctrl-shift-r",
    "cmd-k cmd-f",
    "cmd-shift-space",
    "alt-f3",
    "shift-alt-f3",
    "cmd-alt-z",
    "cmd-alt-g b",
    "cmd-shift-v",
    "cmd-k v",
    "alt-f12",
    "shift-f12",
];

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
    if let Some(expected) = case.folds {
        let folds: Vec<_> = view.update(cx, |view, _| {
            let folds = view.model().display().folds();
            folds.iter().map(|fold| (fold.start, fold.end)).collect()
        });
        assert_eq!(
            folds, expected,
            "folds after `{}` from {:?}",
            case.binding, case.before
        );
    }
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

/// Normalises a keystroke string, or a chord of several, so differently
/// written forms compare equal.
fn normalized(keystrokes: &str) -> String {
    let keystrokes: Vec<_> = keystrokes
        .split_whitespace()
        .map(|keystroke| {
            Keystroke::parse(keystroke)
                .expect("valid keystroke")
                .unparse()
        })
        .collect();
    keystrokes.join(" ")
}

#[test]
fn bindings_are_all_tested() {
    let keymap = fe_keymap::load_keymap(fe_keymap::DEFAULT_MACOS, None).0;
    let bound: BTreeSet<String> = keymap
        .bindings()
        .iter()
        .filter(|binding| {
            binding
                .context
                .as_ref()
                // `enter` leaves the chat's composer out, for its own.
                .is_some_and(|context| {
                    let context = context.to_string();
                    context == KEY_CONTEXT || context == "editor && !chat_composer"
                })
        })
        // `find.show` is fe-workbench's, tested there (`find_keys`).
        .filter(|binding| !binding.command.as_str().starts_with("find."))
        .map(|binding| normalized(&binding.keys.to_string()))
        .collect();
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
fn folds_follow_the_syntax_tree(cx: &mut TestAppContext) {
    // Not indented at all, so indentation alone would fold nothing.
    let text = "function f()\nif x then\n|y()\nend\nz()\nend";
    let (view, cx) = editor(cx, text);
    view.update(cx, |view, cx| view.set_language(Some("lua"), cx));
    cx.run_until_parked();
    let parsed = view.update(cx, |view, _| {
        view.syntax().and_then(fe_syntax::Syntax::tree).is_some()
    });
    assert!(parsed);
    let folds = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            let folds = view.model().display().folds();
            folds
                .iter()
                .map(|fold| (fold.start, fold.end))
                .collect::<Vec<_>>()
        })
    };
    // A caret in a block, heading none, folds the block it's in, up to its
    // `end`.
    cx.simulate_keystrokes("cmd-alt-[");
    assert_eq!(folds(cx), [(1, 2)]);
    assert_eq!(
        state(&view, cx),
        "function f()\nif x then|\ny()\nend\nz()\nend"
    );
    cx.simulate_keystrokes("cmd-k cmd-j cmd-k cmd-0");
    assert_eq!(folds(cx), [(0, 4), (1, 2)]);
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

/// Delivers frames until no rows are pending a wrap. Returns how many.
fn wrap_through(view: &Entity<EditorView>, cx: &mut VisualTestContext) -> usize {
    let pending = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| view.model().display().wrap_is_pending())
    };
    let mut frames = 0;
    while pending(cx) {
        assert!(frames < 1000, "wrapping stalled");
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
        frames += 1;
    }
    frames
}

#[gpui::test]
fn large_files_wrap_across_frames_keeping_the_top_row(cx: &mut TestAppContext) {
    // 1.6 MB: about a dozen wrap slices.
    let text: String = (0..4_000)
        .map(|row| format!("{row:05} {}\n", "word ".repeat(80)))
        .collect();
    let (view, cx) = editor(cx, &format!("|{text}"));

    // The command wraps the caret's rows, and frames wrap the rest.
    cx.simulate_keystrokes("alt-z");
    assert!(wrap_through(&view, cx) > 1);

    // Scroll a continuation row to the top.
    let (top, anchor) = view.update(cx, |view, _| {
        let display = view.model().display();
        let top = display.row_for_buffer_row(2_000) + 1;
        let row = display.row(top);
        assert!(!row.first, "{row:?}");
        (top, row.range.start)
    });
    let position = position_of(&view, cx, 0);
    cx.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0., -(top as f32))),
        ..Default::default()
    });
    let before = position_of(&view, cx, anchor);

    // Widen until no row wraps: the anchor's text stays at the top.
    let height = cx.update(|window, _| window.viewport_size().height);
    cx.simulate_resize(gpui::size(gpui::px(8_000.), height));
    cx.run_until_parked();
    assert!(wrap_through(&view, cx) > 1);
    view.update(cx, |view, _| {
        let display = view.model().display();
        let row = display.row(display.row_for_offset(anchor));
        assert!(row.first && row.last, "{row:?}");
    });
    assert_eq!(position_of(&view, cx, anchor).y, before.y);
}

/// Fixed char widths, counting the bytes measured: ASCII ones separately,
/// since the wrap measures every ASCII char once per width to cache them.
#[derive(Clone, Default)]
struct Counting {
    ascii: Rc<Cell<usize>>,
    other: Rc<Cell<usize>>,
}

impl Counting {
    /// The ASCII and other bytes measured since the last call.
    fn take(&self) -> (usize, usize) {
        (self.ascii.take(), self.other.take())
    }
}

impl CharWidth for Counting {
    fn char_width(&mut self, ch: char) -> f32 {
        let count = if ch.is_ascii() {
            &self.ascii
        } else {
            &self.other
        };
        count.set(count.get() + ch.len_utf8());
        8.
    }
}

#[gpui::test]
fn each_frame_wraps_at_most_one_slice(cx: &mut TestAppContext) {
    // 100k rows of non-ASCII chars, which wrapping measures one by one.
    let text: String = (0..100_000)
        .map(|row| format!("{}\n", "é".repeat(1 + row % 8)))
        .collect();
    // A slice stops at the first row that reaches its budget.
    let slice = WRAP_SLICE_BYTES + "é".repeat(8).len();
    let (view, cx) = editor(cx, &format!("|{text}"));
    let measured = Counting::default();
    view.update(cx, |view, _| {
        view.model_mut().set_char_widths(Box::new(measured.clone()));
    });
    measured.take();
    let pending = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| view.model().display().wrap_is_pending())
    };
    let painted = |cx: &mut VisualTestContext| {
        view.update(cx, |view, _| {
            let painted = view.frame_timings().frames().len();
            view.frame_timings_mut().clear();
            painted
        })
    };
    // Each frame after the first asks for the next while rows are pending,
    // paints once, and wraps one slice.
    let frames = |cx: &mut VisualTestContext| {
        for _ in 0..5 {
            let requested = cx.update(|window, cx| window.simulate_next_frame(cx));
            assert!(requested > 0, "no frame requested");
            cx.run_until_parked();
            assert_eq!(painted(cx), 1);
            let (ascii, other) = measured.take();
            assert_eq!(ascii, 0, "no new width, no new ASCII cache");
            assert!(other > 0, "the frame wrapped nothing");
            assert!(other <= slice, "{other} bytes");
            assert!(pending(cx));
        }
    };

    // alt-z wraps one slice around the caret, after caching the ASCII
    // widths; its frame then wraps one around the viewport.
    painted(cx);
    let command = cx.update(|window, cx| {
        window.dispatch_keystroke(Keystroke::parse("alt-z").unwrap(), cx);
        // Taken before the frame this update ends with.
        measured.take()
    });
    cx.run_until_parked();
    assert!(view.update(cx, |view, _| view.model().soft_wrap()));
    let (ascii, other) = command;
    assert_eq!(ascii, 128);
    assert!(other > 0 && other <= slice, "{other} bytes");
    // The test window may also redraw for the keystroke itself, before the
    // toggle: one slice in all.
    assert!(painted(cx) >= 1);
    let (ascii, other) = measured.take();
    assert_eq!(ascii, 0);
    assert!(other > 0 && other <= slice, "{other} bytes");
    assert!(pending(cx));
    frames(cx);

    // Once wrapped, no more frames are asked for.
    view.update(cx, |view, _| while view.model_mut().wrap_pending(0) {});
    cx.update(|window, cx| window.simulate_next_frame(cx));
    cx.run_until_parked();
    assert_eq!(cx.update(|window, cx| window.simulate_next_frame(cx)), 0);
    painted(cx);
    measured.take();

    // A window an em narrower: its frame caches the ASCII widths for the
    // new width and wraps one slice.
    let (em, wrap_width) = cx.update(|window, cx| {
        let view = view.read(cx);
        let text_system = window.text_system();
        let font_id = text_system.resolve_font(&gpui::font(view.theme.font_family.clone()));
        let em = text_system
            .em_advance(font_id, view.theme.font_size)
            .unwrap();
        (em, view.model().display().wrap_width().unwrap())
    });
    let size = cx.update(|window, _| window.viewport_size());
    cx.simulate_resize(gpui::size(size.width - em, size.height));
    cx.run_until_parked();
    let narrower = view.update(cx, |view, _| view.model().display().wrap_width().unwrap());
    assert!((wrap_width - narrower - f32::from(em)).abs() < 0.01);
    assert_eq!(painted(cx), 1);
    let (ascii, other) = measured.take();
    assert_eq!(ascii, 128);
    assert!(other > 0 && other <= slice, "{other} bytes");
    assert!(pending(cx));
    frames(cx);
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
            assert_eq!(view.model().text().to_string(), "ab");
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
    let frames: Vec<_> = view.update(cx, |view, _| {
        view.frame_timings().frames().copied().collect()
    });
    assert!(!frames.is_empty());
    for frame in frames {
        assert!(frame.prepaint_started + frame.prepaint + frame.paint <= frame.painted_at);
    }
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
            let syntax = view.syntax.as_ref().expect("language loaded").syntax();
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

/// The colour the last frame drew the byte at `offset` in, and the theme's
/// keyword colour.
fn drawn_color(
    view: &Entity<EditorView>,
    cx: &mut VisualTestContext,
    offset: usize,
) -> (Option<gpui::Hsla>, Option<gpui::Hsla>) {
    view.update(cx, |view, _| {
        let frame = &view.layout.as_ref().expect("drawn").frame;
        let row = frame
            .rows
            .iter()
            .find(|row| row.shown().contains(&offset))
            .expect("offset shown");
        let keyword = fe_syntax::Highlight::all().find(|highlight| highlight.name() == "keyword");
        (
            row.line.color_at(offset - row.shown().start),
            view.theme.syntax_color(keyword.unwrap()),
        )
    })
}

/// A frame that needs an injected language, here a Markdown fence's, has
/// the view compile it in the background and draw again, without input.
#[gpui::test]
fn injected_languages_load_without_input(cx: &mut TestAppContext) {
    cx.update(init);
    let text = "# Notes\n\n```python\ndef f(): pass\n```\n";
    let (view, cx) = cx.add_window_view(move |_, cx| {
        let model = EditorModel::new(Buffer::new(text), Some("README.md".into()));
        EditorView::new(model, EditorTheme::default_dark(cx), cx)
    });
    cx.run_until_parked();
    assert!(fe_syntax::loaded_language("python").is_some());
    let (def, keyword) = drawn_color(&view, cx, text.find("def").unwrap());
    assert_eq!(def, keyword);
}

/// A language compiled elsewhere, as by another view, after a frame's
/// highlights found it missing but before the view asks what's missing,
/// still has the view draw again.
#[gpui::test]
fn injected_languages_compiled_meanwhile_draw_again(cx: &mut TestAppContext) {
    cx.update(init);
    fe_syntax::language_by_name("markdown");
    // No other test here loads Swift, whether tests share a process or not.
    assert!(fe_syntax::loaded_language("swift").is_none());
    let text = "```swift\nlet x = 1\n```\n";
    let raced = Rc::new(Cell::new(false));
    let (view, cx) = cx.add_window_view({
        let raced = raced.clone();
        move |_, cx| {
            let model = EditorModel::new(Buffer::new(text), Some("README.md".into()));
            let mut view = EditorView::new(model, EditorTheme::default_dark(cx), cx);
            view.before_taking_missing = Some(Box::new(move |syntax| {
                // Once a frame has highlighted the parsed tree without Lua.
                if !syntax.needs_parse() && fe_syntax::loaded_language("swift").is_none() {
                    fe_syntax::language_by_name("swift");
                    raced.set(true);
                }
            }));
            view
        }
    });
    cx.run_until_parked();
    assert!(raced.get());
    let (let_, keyword) = drawn_color(&view, cx, text.find("let").unwrap());
    assert_eq!(let_, keyword);
}
