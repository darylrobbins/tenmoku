// SPDX-License-Identifier: LicenseRef-Proprietary

//! The single-line text-edit model: motions, edits, undo, IME composition
//! and UTF-16 offsets, and random edit sequences that must keep the
//! selection on grapheme boundaries.

use proptest::prelude::*;
use unicode_segmentation::UnicodeSegmentation as _;

use crate::{Motion, Selection, TextEdit};

/// `text` with the selection drawn in: `|` for a caret, `[`..`]` for a
/// selection made rightwards and `]`..`[` for one made leftwards.
fn show(edit: &TextEdit) -> String {
    let Selection { anchor, head } = edit.selection();
    let text = edit.text();
    if anchor == head {
        return format!("{}|{}", &text[..head], &text[head..]);
    }
    let range = edit.selected_range();
    let (open, close) = if head > anchor {
        ("[", "]")
    } else {
        ("]", "[")
    };
    format!(
        "{}{open}{}{close}{}",
        &text[..range.start],
        &text[range.clone()],
        &text[range.end..]
    )
}

/// An edit of `text` with the caret where `|` is.
fn at(text: &str) -> TextEdit {
    let caret = text.find('|').expect("a caret");
    let mut edit = TextEdit::with_text(&text.replace('|', ""));
    edit.move_caret_to(caret);
    edit
}

#[test]
fn typing_goes_at_the_caret_and_replaces_the_selection() {
    let mut edit = TextEdit::new();
    assert!(edit.insert("hello"));
    assert_eq!(show(&edit), "hello|");
    edit.move_by(Motion::Home);
    edit.insert("> ");
    assert_eq!(show(&edit), "> |hello");
    edit.select_by(Motion::End);
    assert_eq!(show(&edit), "> [hello]");
    edit.insert("bye");
    assert_eq!(show(&edit), "> bye|");
}

#[test]
fn line_breaks_become_spaces() {
    let mut edit = TextEdit::with_text("a\nb");
    assert_eq!(edit.text(), "a b");
    edit.paste("c\r\nd\re");
    assert_eq!(show(&edit), "a bc d e|");
}

#[test]
fn left_and_right_step_over_whole_graphemes() {
    // e + combining acute, a family emoji (one grapheme of seven chars),
    // and a flag (two regional indicators).
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    let mut edit = TextEdit::with_text(&format!("e\u{301}{family}\u{1F1E8}\u{1F1E6}x"));
    let mut stops = vec![edit.caret()];
    while edit.caret() > 0 {
        edit.move_by(Motion::Left);
        stops.push(edit.caret());
    }
    assert_eq!(stops, [30, 29, 21, 3, 0]);
    edit.move_by(Motion::Right);
    edit.move_by(Motion::Right);
    assert_eq!(edit.caret(), 21);
    edit.backspace();
    assert_eq!(edit.text(), "e\u{301}\u{1F1E8}\u{1F1E6}x");
    edit.delete();
    assert_eq!(edit.text(), "e\u{301}x");
}

#[test]
fn offsets_snap_back_to_grapheme_boundaries() {
    let mut edit = TextEdit::with_text("e\u{301}x");
    edit.move_caret_to(1);
    assert_eq!(edit.caret(), 0);
    edit.set_selection(2, 99);
    assert_eq!(edit.selected_range(), 0..4);
}

#[test]
fn left_and_right_collapse_a_selection_to_its_ends() {
    let mut edit = at("ab|cd");
    edit.select_by(Motion::Right);
    edit.select_by(Motion::Right);
    assert_eq!(show(&edit), "ab[cd]");
    edit.move_by(Motion::Left);
    assert_eq!(show(&edit), "ab|cd");
    edit.select_by(Motion::Home);
    assert_eq!(show(&edit), "]ab[cd");
    assert!(edit.selection().reversed());
    edit.move_by(Motion::Right);
    assert_eq!(show(&edit), "ab|cd");
}

#[test]
fn word_motions_stop_at_words_and_punctuation_runs() {
    let mut edit = TextEdit::with_text("let src/main.rs  = x_y;");
    let mut stops = vec![];
    while edit.caret() > 0 {
        edit.move_by(Motion::WordLeft);
        stops.push(show(&edit));
    }
    assert_eq!(
        stops,
        [
            "let src/main.rs  = x_y|;",
            "let src/main.rs  = |x_y;",
            "let src/main.rs  |= x_y;",
            "let src/main.|rs  = x_y;",
            "let src/main|.rs  = x_y;",
            "let src/|main.rs  = x_y;",
            "let src|/main.rs  = x_y;",
            "let |src/main.rs  = x_y;",
            "|let src/main.rs  = x_y;",
        ]
    );
    let mut stops = vec![];
    while edit.caret() < edit.text().len() {
        edit.move_by(Motion::WordRight);
        stops.push(edit.caret());
    }
    assert_eq!(stops, [3, 7, 8, 12, 13, 15, 18, 22, 23]);
    edit.move_by(Motion::Home);
    edit.select_by(Motion::WordRight);
    edit.select_by(Motion::WordRight);
    assert_eq!(edit.selected_text(), "let src");
}

#[test]
fn delete_word_left_takes_the_spaces_and_the_word_before_them() {
    let mut edit = TextEdit::with_text("cargo test --all  ");
    edit.delete_word_left();
    assert_eq!(show(&edit), "cargo test --|");
    edit.delete_word_left();
    assert_eq!(show(&edit), "cargo test |");
    edit.delete_word_left();
    assert_eq!(show(&edit), "cargo |");
    let mut edit = at("one |two");
    edit.delete_word_right();
    assert_eq!(show(&edit), "one |");
    let mut edit = TextEdit::new();
    assert!(!edit.delete_word_left());
    assert!(!edit.backspace());
    assert!(!edit.delete());
    assert!(!edit.can_undo());
}

#[test]
fn word_range_at_finds_the_run_under_a_click() {
    let edit = TextEdit::with_text("foo_bar.baz  qux");
    assert_eq!(edit.word_range_at(2), 0..7);
    assert_eq!(edit.word_range_at(7), 7..8);
    assert_eq!(edit.word_range_at(12), 11..13);
    assert_eq!(edit.word_range_at(16), 13..16);
    assert_eq!(TextEdit::new().word_range_at(0), 0..0);
}

#[test]
fn consecutive_typing_is_one_undo_step() {
    let mut edit = TextEdit::new();
    for ch in ["h", "e", "l", "l", "o"] {
        edit.insert(ch);
    }
    edit.move_by(Motion::Home);
    edit.insert("> ");
    edit.move_by(Motion::End);
    edit.backspace();
    edit.backspace();
    assert_eq!(edit.text(), "> hel");

    assert!(edit.undo());
    assert_eq!(show(&edit), "> hello|");
    assert!(edit.undo());
    assert_eq!(show(&edit), "|hello");
    assert!(edit.undo());
    assert_eq!(show(&edit), "|");
    assert!(!edit.undo());

    assert!(edit.redo());
    assert!(edit.redo());
    assert_eq!(edit.text(), "> hello");
    // A new edit drops what could be redone.
    edit.insert("!");
    assert!(!edit.can_redo());
    assert!(!edit.redo());
}

#[test]
fn paste_cut_and_clear_are_their_own_undo_steps() {
    let mut edit = TextEdit::new();
    edit.insert("ab");
    edit.paste("cd");
    edit.insert("ef");
    edit.select_all();
    assert_eq!(edit.cut().as_deref(), Some("abcdef"));
    assert_eq!(edit.cut(), None);
    edit.undo();
    assert_eq!(edit.text(), "abcdef");
    edit.undo();
    assert_eq!(edit.text(), "abcd");
    edit.undo();
    assert_eq!(edit.text(), "ab");
    edit.redo();
    edit.clear();
    assert_eq!(show(&edit), "|");
    edit.undo();
    assert_eq!(show(&edit), "abcd|");
}

#[test]
fn set_text_forgets_the_history() {
    let mut edit = TextEdit::new();
    edit.insert("x");
    edit.set_text("query");
    assert_eq!(show(&edit), "query|");
    assert!(!edit.can_undo());
}

#[test]
fn composition_replaces_its_own_text_and_commits_as_one_step() {
    let mut edit = at("a|z");
    // Pinyin: "n", then "ni", then the candidate 你 is chosen.
    assert!(edit.replace_and_mark(None, "n", None));
    assert_eq!(edit.marked_range(), Some(1..2));
    edit.replace_and_mark(None, "ni", Some(2..2));
    assert_eq!(show(&edit), "ani|z");
    assert_eq!(edit.marked_range(), Some(1..3));
    edit.insert("你");
    assert_eq!(show(&edit), "a你|z");
    assert_eq!(edit.marked_range(), None);
    edit.undo();
    assert_eq!(show(&edit), "a|z");

    // An empty composition ends it, taking its text.
    let mut edit = at("a|");
    edit.replace_and_mark(None, "`", None);
    edit.replace_and_mark(None, "", None);
    assert_eq!(show(&edit), "a|");
    assert_eq!(edit.marked_range(), None);

    // Moving the caret ends it too, keeping its text.
    let mut edit = at("a|");
    edit.replace_and_mark(None, "ka", None);
    edit.unmark();
    assert_eq!(edit.marked_range(), None);
    edit.replace_and_mark(None, "x", None);
    edit.move_by(Motion::Home);
    assert_eq!((edit.text(), edit.marked_range()), ("akax", None));
}

#[test]
fn utf16_offsets_count_surrogate_pairs_twice() {
    let mut edit = TextEdit::with_text("a😀é");
    assert_eq!(edit.len_utf16(), 4);
    assert_eq!(edit.offset_to_utf16(1), 1);
    assert_eq!(edit.offset_to_utf16(5), 3);
    assert_eq!(edit.offset_to_utf16(7), 4);
    assert_eq!(edit.offset_from_utf16(3), 5);
    // Inside the surrogate pair: the char's end.
    assert_eq!(edit.offset_from_utf16(2), 5);
    assert_eq!(edit.offset_from_utf16(99), 7);
    assert_eq!(edit.range_from_utf16(&(1..3)), 1..5);
    assert_eq!(edit.range_to_utf16(&(1..7)), 1..4);
    // An IME range in UTF-16.
    let range = edit.range_from_utf16(&(3..4));
    edit.replace_range(range, "e");
    assert_eq!(show(&edit), "a😀e|");
}

#[test]
fn replace_range_clamps_to_char_boundaries() {
    let mut edit = TextEdit::with_text("😀x");
    edit.replace_range(2..99, "y");
    assert_eq!(show(&edit), "y|");
}

#[test]
fn undo_keeps_a_bounded_history() {
    let mut edit = TextEdit::new();
    for _ in 0..300 {
        edit.paste("x");
    }
    let mut steps = 0;
    while edit.undo() {
        steps += 1;
    }
    assert_eq!(steps, 200);
    assert_eq!(edit.text().len(), 100);
}

/// One random operation on a [`TextEdit`].
#[derive(Clone, Debug)]
enum Op {
    Insert(String),
    Paste(String),
    Mark(
        Option<std::ops::Range<usize>>,
        String,
        Option<std::ops::Range<usize>>,
    ),
    Unmark,
    Replace(std::ops::Range<usize>, String),
    Move(Motion),
    Select(Motion),
    SetSelection(usize, usize),
    SelectAll,
    Backspace,
    Delete,
    DeleteWordLeft,
    DeleteWordRight,
    Cut,
    Undo,
    Redo,
}

fn text() -> impl Strategy<Value = String> {
    // Letters, punctuation, spaces, a combining mark, an emoji with a
    // modifier, a ZWJ, a regional indicator, CJK and a line break.
    prop::collection::vec(
        prop::sample::select(vec![
            "a",
            "Z",
            "_",
            ".",
            "/",
            " ",
            "\u{301}",
            "😀",
            "\u{1F3FD}",
            "\u{200D}",
            "\u{1F1E8}",
            "你",
            "\n",
            "é",
        ]),
        0..6,
    )
    .prop_map(|parts| parts.concat())
}

fn motion() -> impl Strategy<Value = Motion> {
    prop::sample::select(vec![
        Motion::Left,
        Motion::Right,
        Motion::WordLeft,
        Motion::WordRight,
        Motion::Home,
        Motion::End,
    ])
}

fn op() -> impl Strategy<Value = Op> {
    let offset = 0..40usize;
    prop_oneof![
        text().prop_map(Op::Insert),
        text().prop_map(Op::Paste),
        (
            prop::option::of(offset.clone()).prop_map(|start| start.map(|s| s..s + 3)),
            text(),
            prop::option::of((0..6usize).prop_map(|s| s..s + 1)),
        )
            .prop_map(|(range, text, selected)| Op::Mark(range, text, selected)),
        Just(Op::Unmark),
        (offset.clone(), 0..5usize, text())
            .prop_map(|(start, len, text)| Op::Replace(start..start + len, text)),
        motion().prop_map(Op::Move),
        motion().prop_map(Op::Select),
        (offset.clone(), offset).prop_map(|(a, h)| Op::SetSelection(a, h)),
        Just(Op::SelectAll),
        Just(Op::Backspace),
        Just(Op::Delete),
        Just(Op::DeleteWordLeft),
        Just(Op::DeleteWordRight),
        Just(Op::Cut),
        Just(Op::Undo),
        Just(Op::Redo),
    ]
}

fn apply(edit: &mut TextEdit, op: &Op) {
    match op.clone() {
        Op::Insert(text) => _ = edit.insert(&text),
        Op::Paste(text) => _ = edit.paste(&text),
        Op::Mark(range, text, selected) => _ = edit.replace_and_mark(range, &text, selected),
        Op::Unmark => edit.unmark(),
        Op::Replace(range, text) => _ = edit.replace_range(range, &text),
        Op::Move(motion) => edit.move_by(motion),
        Op::Select(motion) => edit.select_by(motion),
        Op::SetSelection(anchor, head) => edit.set_selection(anchor, head),
        Op::SelectAll => edit.select_all(),
        Op::Backspace => _ = edit.backspace(),
        Op::Delete => _ = edit.delete(),
        Op::DeleteWordLeft => _ = edit.delete_word_left(),
        Op::DeleteWordRight => _ = edit.delete_word_right(),
        Op::Cut => _ = edit.cut(),
        Op::Undo => _ = edit.undo(),
        Op::Redo => _ = edit.redo(),
    }
}

fn is_boundary(text: &str, offset: usize) -> bool {
    offset == text.len() || text.grapheme_indices(true).any(|(ix, _)| ix == offset)
}

proptest! {
    #[test]
    fn random_edits_keep_offsets_on_boundaries(ops in prop::collection::vec(op(), 0..40)) {
        let mut edit = TextEdit::new();
        for op in &ops {
            apply(&mut edit, op);
            let text = edit.text();
            let Selection { anchor, head } = edit.selection();
            prop_assert!(!text.contains(['\n', '\r']), "{op:?} left a line break");
            prop_assert!(is_boundary(text, anchor), "{op:?}: anchor {anchor} in {text:?}");
            prop_assert!(is_boundary(text, head), "{op:?}: head {head} in {text:?}");
            if let Some(marked) = edit.marked_range() {
                prop_assert!(marked.start <= marked.end && marked.end <= text.len());
                prop_assert!(text.is_char_boundary(marked.start));
                prop_assert!(text.is_char_boundary(marked.end));
            }
            // UTF-16 round trips at every char boundary.
            for (ix, _) in text.char_indices().chain([(text.len(), ' ')]) {
                prop_assert_eq!(edit.offset_from_utf16(edit.offset_to_utf16(ix)), ix);
            }
            prop_assert_eq!(edit.offset_to_utf16(text.len()), edit.len_utf16());
        }
    }

    #[test]
    fn undoing_everything_restores_the_start(
        start in text(),
        ops in prop::collection::vec(op(), 0..30),
    ) {
        let mut edit = TextEdit::with_text(&start);
        let original = edit.text().to_string();
        for op in &ops {
            apply(&mut edit, op);
        }
        while edit.undo() {}
        prop_assert_eq!(edit.text(), original.as_str());
        // And redoing everything is idempotent with undoing it again.
        while edit.redo() {}
        let redone = edit.text().to_string();
        while edit.undo() {}
        prop_assert_eq!(edit.text(), original.as_str());
        while edit.redo() {}
        prop_assert_eq!(edit.text(), redone.as_str());
    }
}
