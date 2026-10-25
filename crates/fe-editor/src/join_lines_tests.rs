use fe_buffer::ResolvedSelection;

use crate::tests::{parse, render};

/// Joins on `marked`, checks it gives `expected` as one undo step.
fn one_step(marked: &str, expected: &str) {
    let mut model = parse(marked);
    model.join_lines();
    assert_eq!(render(&model), expected, "join on {marked:?}");
    model.undo().unwrap();
    assert_eq!(
        model.text().to_string(),
        parse(marked).text().to_string(),
        "undoing {marked:?}"
    );
}

#[test]
fn bare_caret() {
    one_step("|a\n   b\nc", "a| b\nc");
    one_step("a|\n\tb\nc", "a| b\nc");
}

#[test]
fn selection_over_rows() {
    one_step("[a\n b\n  c]\nd", "[a b c]\nd");
    // Ending at column 0 of a later row leaves that row alone.
    one_step("[a\n b\n]c\nd", "[a b\n]c\nd");
    // Ending at column 0 of the next row, from a single row: nothing joins,
    // forwards or reversed, and no undo step is made.
    for reversed in [false, true] {
        let mut model = parse("[a\n]b\nc");
        model.set_selections(vec![ResolvedSelection {
            range: 0..2,
            reversed,
        }]);
        model.join_lines();
        assert_eq!(model.text().to_string(), "a\nb\nc");
        assert_eq!(model.selections()[0].range, 0..2);
        assert_eq!(model.undo().unwrap(), None);
    }
    // Inside one row joins it with the next.
    one_step("[a]b\nc", "[a]b c");
}

#[test]
fn no_space_cases() {
    one_step("a\n|\nb", "a\n|b");
    one_step("a|\n\n  b", "a|\n  b");
    one_step("a|\n  )b", "a|)b");
    one_step("a|\n   \nb", "a|\nb");
    // An empty row joined onto stays empty until text arrives.
    one_step("[\n\n b]", "[b]");
    let mut model = parse("a\nb|");
    model.join_lines();
    assert_eq!(render(&model), "a\nb|");
    // No undo step was made.
    assert_eq!(model.undo().unwrap(), None);
}

#[test]
fn multiple_selections() {
    one_step("|a\nb\nc\n|d\ne", "a| b\nc\nd| e");
    one_step("|a\n|b\nc\nd", "a| b| c\nd");
}
