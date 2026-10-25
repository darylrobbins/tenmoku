use crate::tests::{parse, render};
use fe_command::{Effect, Invocation};

use crate::{
    EditorModel,
    commands::{self, EditorTarget, HeadlessHost},
};

fn dispatch(marked: &str, name: &str) -> EditorModel {
    let mut model = parse(marked);
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of(&format!("editor.{name}"))
        .unwrap()
        .context(commands::context());
    let effect = commands::registry()
        .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
        .unwrap()
        .effect;
    assert_eq!(effect, Effect::Changed);
    model
}

/// Runs `name` on `marked`, checks it gives `expected` as one undo step.
fn one_step(marked: &str, name: &str, expected: &str) {
    let mut model = dispatch(marked, name);
    assert_eq!(render(&model), expected, "{name} on {marked:?}");
    model.undo().unwrap();
    assert_eq!(
        model.text().to_string(),
        parse(marked).text().to_string(),
        "undoing {name}"
    );
}

#[test]
fn sort_ascending_whole_document() {
    // Case ignored first, stable, final newline kept in place.
    one_step(
        "b\nB\na\nA|\nc\n",
        "sort_lines_ascending",
        "a\nA|\nb\nB\nc\n",
    );
    one_step("|b\n\na\n", "sort_lines_ascending", "\na\n|b\n");
}

#[test]
fn sort_descending_selection() {
    one_step(
        "1\n[b\nc\na\n]d\ne",
        "sort_lines_descending",
        "1\n[c\nb\na\n]d\ne",
    );
    // Within one row there's nothing to sort.
    let mut model = parse("b\n[x]y\na");
    model.sort_lines(false);
    assert_eq!(render(&model), "b\n[x]y\na");
}

#[test]
fn multiple_selections() {
    one_step(
        "[c\na]\nz\n[y\nx]",
        "sort_lines_ascending",
        "[a\nc]\nz\n[x\ny]",
    );
}

#[test]
fn delete_duplicates() {
    one_step("a\nb\n|a\nc\nb\n", "delete_duplicate_lines", "|a\nb\nc\n");
    one_step("[a\na\na]", "delete_duplicate_lines", "[a]");
    one_step("a\n[b\nb\na]\na", "delete_duplicate_lines", "a\n[b\na]\na");
}

#[test]
fn accents_sort_beside_their_letter() {
    one_step(
        "|z\né\ne\nE\nf\n",
        "sort_lines_ascending",
        "e\nE\né\nf\n|z\n",
    );
    one_step("|z\né\ne\nf\n", "sort_lines_descending", "|z\nf\né\ne\n");
}

#[test]
fn adjacent_selections_stay_apart() {
    // Each selection is within one row, so neither has two rows to sort.
    let mut model = parse("[b]\n[a]");
    model.sort_lines(false);
    assert_eq!(render(&model), "[b]\n[a]");
    let mut model = parse("[a]\n[a]");
    model.delete_duplicate_lines();
    assert_eq!(render(&model), "[a]\n[a]");
    // Adjacent two-row ranges sort on their own.
    one_step("[d\nc]\n[b\na]", "sort_lines_ascending", "[c\nd]\n[a\nb]");
}

#[test]
fn selection_ending_at_the_next_row_collapses() {
    one_step("b[b\na\n]z", "sort_lines_ascending", "|a\nbb\nz");
}

#[test]
fn duplicates_in_several_ranges() {
    one_step(
        "[a\na]\nx\n[b\nb\nb]",
        "delete_duplicate_lines",
        "[a\n]x\n[b]",
    );
}

#[test]
fn commands_have_their_ids_and_titles() {
    let registry = commands::registry();
    for (id, title) in [
        ("editor.sort_lines_ascending", "Sort Lines Ascending"),
        ("editor.sort_lines_descending", "Sort Lines Descending"),
        ("editor.delete_duplicate_lines", "Delete Duplicate Lines"),
    ] {
        let meta = registry
            .commands()
            .find(|meta| meta.id.as_str() == id)
            .unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(meta.title, title, "{id}");
    }
}

#[test]
fn letters_that_do_not_decompose_sort_beside_their_base() {
    one_step(
        "|z\nø\no\næ\nb\na\n",
        "sort_lines_ascending",
        "a\næ\nb\no\nø\n|z\n",
    );
    one_step("|a\næ\nb\n", "sort_lines_descending", "b\næ\n|a\n");
}

#[test]
fn caret_on_the_empty_last_row_stays() {
    one_step("b\na\n|", "sort_lines_ascending", "a\nb\n|");
    one_step("a\nb\n|", "sort_lines_descending", "b\na\n|");
}

#[test]
fn deleted_caret_in_a_later_range_follows_the_range_start() {
    one_step(
        "[a\na\na\na]\nx\n[bb\nb]b|\nz",
        "delete_duplicate_lines",
        "[a\n]x\n|[bb\n]z",
    );
}

#[test]
fn sort_uses_locale_order() {
    // What JS `localeCompare` gives: digits before letters, case ignored
    // first (lowercase first on a tie), an accented letter after its base.
    let expected = ["10", "9", "a", "A", "b", "B", "e", "é"];
    let input = "|b\nB\na\nA\né\ne\n10\n9";
    let mut model = dispatch(input, "sort_lines_ascending");
    assert_eq!(model.text().to_string(), expected.join("\n"));
    model.undo().unwrap();
    let reversed: Vec<&str> = expected.iter().rev().copied().collect();
    let mut model = dispatch(input, "sort_lines_descending");
    assert_eq!(model.text().to_string(), reversed.join("\n"));
    model.undo().unwrap();
    // Root collation, not code points: z before þ, å before ä.
    one_step("|þ\nz", "sort_lines_ascending", "z\n|þ");
    one_step("|ä\nå", "sort_lines_ascending", "å\n|ä");
    one_step("|z\nþ", "sort_lines_descending", "þ\n|z");
    one_step("|å\nä", "sort_lines_descending", "ä\n|å");
}

#[test]
fn shared_row_selections_keep_their_text() {
    // The first selection's "b\nc" stays together; the second's "\naa" does not.
    one_step("[b\nc]c[\naa]", "sort_lines_ascending", "|aa\n[b\nc]c");
    // The second selection keeps the "x" that survives.
    one_step("[aa\na][a\nx\naa]", "delete_duplicate_lines", "[aa\n][x]");
}

#[test]
fn exclusive_ends_keep_the_selected_rows() {
    // The second selection's "b\nc\n" ends at the start of "d", outside the
    // group; it stays on those rows ascending, and collapses once they part.
    one_step(
        "[z\nb]b[b\nc\n]d",
        "sort_lines_ascending",
        "|bb[b\nc\n]z\nd",
    );
    one_step("[z\nb]b[b\nc\n]d", "sort_lines_descending", "|z\nc\nbbb\nd");
}
