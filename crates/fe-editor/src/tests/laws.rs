//! Algebraic laws of movement and editing, plus a fuzzer that drives random
//! command sequences and checks the model stays well formed throughout.

use proptest::prelude::*;
use rand::{Rng, SeedableRng, rngs::StdRng};
use unicode_segmentation::UnicodeSegmentation;

use super::{parse, render};
use crate::*;

fn cases() -> u32 {
    std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|cases| cases.parse().ok())
        .unwrap_or(256)
}

/// Text mixing words, punctuation, indentation, tabs, blank rows and
/// multi-codepoint graphemes.
fn text_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            6 => proptest::char::range('a', 'e').prop_map(String::from),
            2 => Just(" ".to_string()),
            1 => Just("\t".to_string()),
            1 => Just("_".to_string()),
            1 => Just(".".to_string()),
            1 => Just("(".to_string()),
            2 => Just("\n".to_string()),
            1 => Just("    ".to_string()),
            1 => Just("é".to_string()),
            1 => Just("e\u{301}".to_string()),
            1 => Just("😀".to_string()),
            1 => Just("👍🏽".to_string()),
            1 => Just("中".to_string()),
        ],
        0..60,
    )
    .prop_map(|pieces| pieces.concat())
}

/// Offsets a cursor can rest on: grapheme boundaries within each row, plus
/// each row's end.
fn rest_points(text: &str) -> Vec<usize> {
    let mut points = Vec::new();
    let mut row_start = 0;
    for row in text.split('\n') {
        points.extend(row.grapheme_indices(true).map(|(ix, _)| row_start + ix));
        points.push(row_start + row.len());
        row_start += row.len() + 1;
    }
    points
}

fn model_with_caret(text: &str, offset: usize) -> EditorModel {
    let mut model = EditorModel::new(Buffer::new(text), None);
    model.set_cursor(offset);
    model
}

fn moved(text: &str, offset: usize, movement: Movement) -> usize {
    let mut model = model_with_caret(text, offset);
    model.move_cursors(movement, false);
    model.primary_selection().head()
}

/// Checks the invariants every command must preserve.
fn check_well_formed(model: &EditorModel) -> Result<(), TestCaseError> {
    let text = model.text();
    let selections = model.selections();
    prop_assert!(!selections.is_empty(), "no selections");
    for selection in &selections {
        prop_assert!(selection.range.start <= selection.range.end);
        prop_assert!(selection.range.end <= text.len(), "selection past the end");
        prop_assert!(text.is_char_boundary(selection.range.start));
        prop_assert!(text.is_char_boundary(selection.range.end));
    }
    for pair in selections.windows(2) {
        prop_assert!(
            pair[0].range.end <= pair[1].range.start,
            "selections overlap: {:?}",
            pair
        );
        prop_assert!(pair[0].range != pair[1].range, "duplicate carets");
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(cases()))]

    /// Right then Left returns to where you started, from any rest point.
    #[test]
    fn right_then_left_round_trips(text in text_strategy()) {
        for offset in rest_points(&text) {
            if offset == text.len() {
                continue;
            }
            let right = moved(&text, offset, Movement::Right);
            prop_assert!(right > offset, "Right didn't advance from {offset}");
            prop_assert!(rest_points(&text).contains(&right), "Right landed mid-grapheme at {right}");
            prop_assert_eq!(moved(&text, right, Movement::Left), offset);
        }
    }

    /// Word moves always make progress and stay on char boundaries.
    #[test]
    fn word_moves_make_progress(text in text_strategy()) {
        for offset in rest_points(&text) {
            let right = moved(&text, offset, Movement::WordRight);
            let left = moved(&text, offset, Movement::WordLeft);
            if offset < text.len() {
                prop_assert!(right > offset);
            } else {
                prop_assert_eq!(right, offset);
            }
            if offset > 0 {
                prop_assert!(left < offset);
            } else {
                prop_assert_eq!(left, 0);
            }
            prop_assert!(text.is_char_boundary(right) && text.is_char_boundary(left));
        }
    }

    /// Home toggles between the first non-blank and column 0; End is the row's end.
    #[test]
    fn home_and_end(text in text_strategy()) {
        let buffer_text = Text::new(&text);
        for offset in rest_points(&text) {
            let row = buffer_text.offset_to_point(offset).row;
            let row_start = buffer_text.row_start(row);
            let row_text = buffer_text.row_to_string(row);
            let indent = row_start + row_text.len() - row_text.trim_start().len();
            let home = moved(&text, offset, Movement::LineStart);
            prop_assert!(home == row_start || home == indent);
            let home_twice = moved(&text, home, Movement::LineStart);
            prop_assert!(home_twice == row_start || home_twice == indent);
            if indent != row_start {
                prop_assert!(home != home_twice, "Home should toggle");
            }
            let end = moved(&text, offset, Movement::LineEnd);
            prop_assert_eq!(end, row_start + row_text.len());
        }
    }

    /// Down then Up returns to the start when the row below is long enough
    /// to keep the column. (Tab alignment is covered by unit tests.)
    #[test]
    fn vertical_round_trip(rows in proptest::collection::vec("[a-z é😀中]{0,12}", 2..6), row in 0usize..6, column in 0usize..14) {
        let text = rows.join("\n");
        let row = row.min(rows.len() - 2);
        let width = |row: &str| row.chars().count();
        let column = column.min(width(&rows[row])).min(width(&rows[row + 1]));
        let mut model = EditorModel::new(Buffer::new(&text), None);
        let row_start = model.text().row_start(row as u32);
        let start = row_start + rows[row].char_indices().nth(column).map_or(rows[row].len(), |(ix, _)| ix);

        model.set_cursor(start);
        model.move_cursors(Movement::Down, false);
        let below = model.primary_selection().head();
        let next_start = model.text().row_start(row as u32 + 1);
        prop_assert_eq!(model.text().slice_to_string(next_start..below).chars().count(), column);
        model.move_cursors(Movement::Up, false);
        prop_assert_eq!(model.primary_selection().head(), start);
    }

    /// Typing at several carets is the same as typing at each position in a
    /// plain string, back to front.
    #[test]
    fn multi_cursor_typing_matches_string(text in text_strategy(), picks in proptest::collection::vec(any::<prop::sample::Index>(), 1..5), typed in "[a-z\n😀]{1,4}") {
        let points = rest_points(&text);
        let mut carets: Vec<usize> = picks.iter().map(|pick| points[pick.index(points.len())]).collect();
        carets.sort_unstable();
        carets.dedup();
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.set_selections(carets.iter().map(|&offset| caret(offset)));
        model.insert(&typed);

        let mut expected = text.clone();
        for &offset in carets.iter().rev() {
            expected.insert_str(offset, &typed);
        }
        prop_assert_eq!(model.text().to_string(), expected);
        // Each caret ends up just after its typed text.
        let heads: Vec<usize> = model.selections().iter().map(|s| s.head()).collect();
        let expected_heads: Vec<usize> = carets
            .iter()
            .enumerate()
            .map(|(ix, &offset)| offset + typed.len() * (ix + 1))
            .collect();
        prop_assert_eq!(heads, expected_heads);
    }

    /// Typing then backspacing the same number of graphemes is a no-op.
    #[test]
    fn typing_then_backspacing_restores(text in text_strategy(), pick in any::<prop::sample::Index>(), typed in "[a-z😀é]{1,6}") {
        let points = rest_points(&text);
        let offset = points[pick.index(points.len())];
        let mut model = model_with_caret(&text, offset);
        model.insert(&typed);
        for _ in typed.graphemes(true) {
            model.backspace();
        }
        prop_assert_eq!(model.text().to_string(), text);
        prop_assert_eq!(model.primary_selection().head(), offset);
    }

    /// Cutting a selection and pasting it back restores the text.
    #[test]
    fn cut_then_paste_restores(text in text_strategy(), a in any::<prop::sample::Index>(), b in any::<prop::sample::Index>()) {
        let points = rest_points(&text);
        let (a, b) = (points[a.index(points.len())], points[b.index(points.len())]);
        if a == b {
            // Nothing to cut; skipping beats rejecting (short texts hit this often).
            return Ok(());
        }
        let range = a.min(b)..a.max(b);
        let mut model = EditorModel::new(Buffer::new(&text), None);
        model.select_range(range.clone(), false);
        let (copied, whole_rows) = model.copy_text();
        prop_assert!(!whole_rows);
        prop_assert_eq!(&copied, &text[range.clone()]);
        let cut = model.cut();
        prop_assert_eq!(&cut, &copied);
        prop_assert_eq!(model.primary_selection().range, range.start..range.start);
        model.paste(&cut);
        prop_assert_eq!(model.text().to_string(), text);
    }

    /// Random command sequences keep the model well formed, and undoing
    /// everything gets back the original text.
    #[test]
    fn random_commands_keep_the_model_well_formed(text in text_strategy(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut model = EditorModel::new(Buffer::new(&text), None);
        let movements = [
            Movement::Left, Movement::Right, Movement::Up, Movement::Down,
            Movement::WordLeft, Movement::WordRight, Movement::LineStart, Movement::LineEnd,
            Movement::DocumentStart, Movement::DocumentEnd, Movement::PageUp(3), Movement::PageDown(3),
        ];
        let mut log = Vec::new();
        for _ in 0..60 {
            let len = model.text().len();
            let command = rng.random_range(0..17);
            log.push(command);
            match command {
                0 => model.move_cursors(movements[rng.random_range(0..movements.len())], rng.random_bool(0.4)),
                1 => model.set_cursor(rng.random_range(0..=len + 3)),
                2 => model.select_range(rng.random_range(0..=len)..rng.random_range(0..=len + 2), rng.random_bool(0.5)),
                3 => model.extend_primary_to(rng.random_range(0..=len)),
                4 => model.insert(["x", "yz", "\n", "😀", "e\u{301}", ""][rng.random_range(0..6)]),
                5 => model.backspace(),
                6 => model.delete_forward(),
                7 => model.delete_word_left(),
                8 => model.newline(),
                9 => model.tab(),
                10 => { model.cut(); }
                11 => model.paste(["a\nb", "line\n", "😀"][rng.random_range(0..3)]),
                12 => { let _ = model.undo(); }
                13 => { let _ = model.redo(); }
                14 => model.select_all(),
                15 => {
                    // Add a caret somewhere, as multi-cursor features will.
                    let mut selections = model.selections();
                    let offset = model.text().clip_offset(rng.random_range(0..=len), Bias::Left);
                    selections.push(caret(offset));
                    model.set_selections(selections);
                }
                _ => {
                    let offset = model.text().clip_offset(rng.random_range(0..=len), Bias::Left);
                    let word = model.word_range_at(offset);
                    prop_assert!(word.start <= offset && offset <= word.end.max(offset));
                    model.select_range(word, false);
                }
            }
            check_well_formed(&model).map_err(|error| {
                TestCaseError::fail(format!("{error}\ncommands: {log:?}\nstate: {:?}", render(&model)))
            })?;
        }
        // A fresh edit discards pending redos, so "redo everything" below
        // means exactly the edits that produced this text.
        model.insert("!");
        let edited = model.text().to_string();
        while model.undo().unwrap().is_some() {}
        prop_assert_eq!(model.text().to_string(), text);
        while model.redo().unwrap().is_some() {}
        prop_assert_eq!(model.text().to_string(), edited);
    }
}

#[test]
fn notation_parses_multiple_selections() {
    let model = parse("[ab]c|d");
    assert_eq!(model.selections().len(), 2);
}
