use proptest::prelude::*;

use super::*;
use crate::{
    caret,
    snippet::Snippet,
    tests::{parse, render},
};

/// A model of `marked`, as `parse` reads it.
fn tags(marked: &str) -> EditorModel {
    parse(marked)
}

fn link(model: &mut EditorModel, ranges: &[Range<usize>]) {
    model.set_linked_ranges(ranges.to_vec());
}

fn text(model: &EditorModel) -> String {
    model.text().to_string()
}

#[test]
fn typing_in_one_range_mirrors_to_the_other() {
    let mut model = tags("<div|></div>");
    link(&mut model, &[1..4, 7..10]);
    model.insert("x");
    assert_eq!(text(&model), "<divx></divx>");
    assert_eq!(render(&model), "<divx|></divx>");
    assert_eq!(model.linked_ranges(), Some(&[1..5, 8..12][..]));
    model.insert("y");
    assert_eq!(text(&model), "<divxy></divxy>");
}

#[test]
fn typing_at_the_start_and_in_the_second_range() {
    let mut model = tags("<div></div|>");
    link(&mut model, &[1..4, 7..10]);
    model.insert("s");
    assert_eq!(text(&model), "<divs></divs>");
    model.set_cursor(1);
    model.insert("p");
    assert_eq!(text(&model), "<pdivs></pdivs>");
    assert_eq!(render(&model), "<p|divs></pdivs>");
}

#[test]
fn backspace_and_delete_word_mirror() {
    let mut model = tags("<div|></div>");
    link(&mut model, &[1..4, 7..10]);
    model.backspace();
    assert_eq!(text(&model), "<di></di>");
    model.delete_word_left();
    assert_eq!(text(&model), "<></>");
    assert_eq!(model.linked_ranges(), Some(&[1..1, 4..4][..]));
}

#[test]
fn paste_mirrors() {
    let mut model = tags("<a|></a>");
    link(&mut model, &[1..2, 5..6]);
    model.paste("bc");
    assert_eq!(text(&model), "<abc></abc>");
    assert_eq!(model.linked_ranges(), Some(&[1..4, 7..10][..]));
}

#[test]
fn cut_mirrors() {
    let mut model = tags("<[ab]c></abc>");
    link(&mut model, &[1..4, 7..10]);
    assert_eq!(model.cut(), "ab");
    assert_eq!(text(&model), "<c></c>");
}

#[test]
fn unequal_ranges_mirror_at_the_same_inner_offset() {
    let mut model = tags("<ab|cd></xyz>");
    link(&mut model, &[1..5, 8..11]);
    model.insert("Q");
    assert_eq!(text(&model), "<abQcd></xyQz>");
    assert_eq!(model.linked_ranges(), Some(&[1..6, 9..13][..]));
}

#[test]
fn one_undo_step_undoes_both() {
    let mut model = tags("<div|></div>");
    link(&mut model, &[1..4, 7..10]);
    model.insert("x");
    model.undo().unwrap();
    assert_eq!(text(&model), "<div></div>");
    assert_eq!(model.linked_ranges(), None);
}

#[test]
fn an_edit_before_the_ranges_shifts_them() {
    // Typing in the earlier range shifts the later; in the later, not the
    // earlier.
    let mut model = tags("<a></a|> <b></b>");
    link(&mut model, &[1..2, 5..6]);
    model.insert("z");
    assert_eq!(text(&model), "<az></az> <b></b>");
    assert_eq!(model.linked_ranges(), Some(&[1..3, 6..8][..]));
}

#[test]
fn an_edit_outside_ends_the_session() {
    let mut model = tags("<a|></a>");
    link(&mut model, &[1..2, 5..6]);
    model.apply_with_carets(vec![(0..0, "q".to_string())]);
    assert_eq!(model.linked_ranges(), None);
    assert_eq!(text(&model), "q<a></a>");
}

#[test]
fn an_edit_crossing_a_boundary_ends_the_session() {
    let mut model = tags("<ab|></ab>");
    link(&mut model, &[1..3, 6..8]);
    model.apply_with_carets(vec![(0..2, String::new())]);
    assert_eq!(model.linked_ranges(), None);
    assert_eq!(text(&model), "b></ab>");
}

#[test]
fn leaving_the_range_ends_it_and_moving_to_the_other_keeps_it() {
    let mut model = tags("<ab|></ab>");
    link(&mut model, &[1..3, 6..8]);
    model.set_cursor(7);
    assert!(model.linked_ranges().is_some());
    model.set_cursor(8);
    assert!(model.linked_ranges().is_some());
    model.set_cursor(4);
    assert_eq!(model.linked_ranges(), None);
    model.insert("x");
    assert_eq!(text(&model), "<ab>x</ab>");
}

#[test]
fn a_non_word_char_ends_the_session_unmirrored() {
    let mut model = tags("<ab|></ab>");
    link(&mut model, &[1..3, 6..8]);
    model.insert(" ");
    assert_eq!(text(&model), "<ab ></ab>");
    assert_eq!(model.linked_ranges(), None);
    let mut model = tags("<ab|></ab>");
    link(&mut model, &[1..3, 6..8]);
    model.insert("-c.d:e_f1");
    assert_eq!(text(&model), "<ab-c.d:e_f1></ab-c.d:e_f1>");
}

#[test]
fn a_second_cursor_ends_it() {
    let mut model = tags("<ab|></ab>");
    link(&mut model, &[1..3, 6..8]);
    model.set_selections([caret(3), caret(7)]);
    assert_eq!(model.linked_ranges(), None);
    // And none starts with several.
    link(&mut model, &[1..3, 6..8]);
    assert_eq!(model.linked_ranges(), None);
}

#[test]
fn a_snippet_session_wins() {
    let mut model = tags("<ab|></ab> ");
    model.insert_snippet(10..10, &Snippet::parse("${1:x} ${1:x}"));
    assert!(model.in_snippet());
    link(&mut model, &[1..3, 6..8]);
    assert_eq!(model.linked_ranges(), None);
}

#[test]
fn bad_ranges_clear() {
    let mut model = tags("<ab|></ab>");
    for ranges in [
        std::iter::once(1..3).collect(),
        vec![1..3, 2..5],
        vec![1..3, 6..80],
    ] {
        link(&mut model, &ranges);
        assert_eq!(model.linked_ranges(), None, "{ranges:?}");
    }
    link(&mut model, &[1..3, 6..8]);
    assert!(model.linked_ranges().is_some());
    model.clear_linked_ranges();
    assert_eq!(model.linked_ranges(), None);
}

proptest! {
    #[test]
    fn mirrored_ranges_stay_equal(
        steps in proptest::collection::vec((0u8..4, "[a-c]{1,3}", 0usize..6), 1..20),
    ) {
        let mut model = tags("<abc|></abc>");
        link(&mut model, &[1..4, 7..10]);
        for (kind, typed, at) in steps {
            let Some(ranges) = model.linked_ranges().map(|r| r.to_vec()) else {
                break;
            };
            // Any caret in the first range.
            model.set_cursor(ranges[0].start + at.min(ranges[0].len()));
            match kind {
                0 | 1 => model.insert(&typed),
                2 => model.backspace(),
                _ => model.delete_word_left(),
            }
            if let Some(ranges) = model.linked_ranges() {
                let all = model.text().to_string();
                prop_assert_eq!(&all[ranges[0].clone()], &all[ranges[1].clone()]);
            }
        }
    }
}

#[test]
fn adjacent_ranges_link_and_the_selection_owns_a_boundary_edit() {
    let mut model = tags("ab|ab");
    link(&mut model, &[0..2, 2..4]);
    assert!(model.linked_ranges().is_some());
    // Typing at the shared boundary goes to the range the caret is in, the
    // first, and the second follows.
    model.insert("x");
    assert_eq!(text(&model), "abxabx");
    assert_eq!(render(&model), "abx|abx");
    assert_eq!(model.linked_ranges(), Some(&[0..3, 3..6][..]));
    model.insert("y");
    assert_eq!(text(&model), "abxyabxy");
    // Backspace in the second range mirrors into the first.
    model.set_cursor(8);
    model.backspace();
    assert_eq!(text(&model), "abxabx");
}
