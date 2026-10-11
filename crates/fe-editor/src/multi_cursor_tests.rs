use super::*;
use crate::tests::{parse, render};

#[test]
fn carets_above_and_below() {
    let mut model = parse("abcdef\nab\nabcd|ef");
    model.add_caret_above();
    // A shorter row puts the caret at its end, keeping the goal column.
    assert_eq!(render(&model), "abcdef\nab|\nabcd|ef");
    model.add_caret_above();
    assert_eq!(render(&model), "abcd|ef\nab|\nabcd|ef");
    // Nothing above the first row.
    model.add_caret_above();
    assert_eq!(render(&model), "abcd|ef\nab|\nabcd|ef");
    // Nor below the last.
    model.add_caret_below();
    assert_eq!(render(&model), "abcd|ef\nab|\nabcd|ef");
    // The carets move together afterwards, each keeping its goal.
    model.move_cursors(Movement::Up, false);
    assert_eq!(render(&model), "|abcd|ef\nab|\nabcdef");

    let mut model = parse("ab|cdef\nab\nabcdef");
    model.add_caret_below();
    model.add_caret_below();
    assert_eq!(render(&model), "ab|cdef\nab|\nab|cdef");

    // A folded row is skipped, both ways.
    let mut model = parse("def a():\n    one\n    two\nen|d");
    model.fold_rows(0..=2);
    model.add_caret_above();
    assert_eq!(render(&model), "de|f a():\n    one\n    two\nen|d");
    let mut model = parse("de|f a():\n    one\n    two\nend");
    model.fold_rows(0..=2);
    model.add_caret_below();
    assert_eq!(render(&model), "de|f a():\n    one\n    two\nen|d");
}

#[test]
fn carets_above_and_below_count_soft_wrapped_rows() {
    let mut model = parse("abcdef\nabcdefgh|ij");
    model.set_wrap_width(4.);
    model.set_soft_wrap(true);
    model.add_caret_above();
    // The row above the head's is the first half of its own wrapped row.
    assert_eq!(render(&model), "abcdef\nabcd|efgh|ij");
}

#[test]
fn next_occurrence() {
    let mut model = parse("fo|o foobar foo");
    model.select_next_occurrence();
    assert_eq!(render(&model), "[foo] foobar foo");
    model.select_next_occurrence();
    assert_eq!(render(&model), "[foo] foobar [foo]");
    model.select_next_occurrence();
    assert_eq!(render(&model), "[foo] foobar [foo]");

    // A selection that isn't a whole word matches inside words.
    let mut model = parse("x[foo] foobar");
    model.select_next_occurrence();
    assert_eq!(render(&model), "x[foo] [foo]bar");

    // Search wraps to the start once.
    let mut model = parse("foo foo [foo] foo");
    model.select_next_occurrence();
    assert_eq!(render(&model), "foo foo [foo] [foo]");
    model.select_next_occurrence();
    assert_eq!(render(&model), "[foo] foo [foo] [foo]");

    // A two-row needle matches across rows.
    let mut model = parse("[a\nb] a\nb a");
    model.select_next_occurrence();
    assert_eq!(render(&model), "[a\nb] [a\nb] a");

    // Matching is case-sensitive.
    let mut model = parse("[foo] Foo foo");
    model.select_next_occurrence();
    assert_eq!(render(&model), "[foo] Foo [foo]");

    // Wrapping stops on a char boundary when the needle's end falls
    // inside a multibyte char after the bottommost selection.
    let mut model = parse("ab [ab]é");
    model.select_next_occurrence();
    assert_eq!(render(&model), "[ab] [ab]é");
    let mut model = parse("aé [aé]xé");
    model.select_next_occurrence();
    assert_eq!(render(&model), "[aé] [aé]xé");
}

#[test]
fn carets_on_whitespace_stay_carets() {
    // Even just past a word, where `word_range_at` would give the word.
    for marked in ["foo  | foo", "foo| foo", "foo|\tfoo", "foo|\nfoo"] {
        let mut model = parse(marked);
        model.select_next_occurrence();
        assert_eq!(render(&model), marked);
        model.select_all_occurrences();
        assert_eq!(render(&model), marked);
    }
    // At the end of the text the word before is taken.
    let mut model = parse("foo foo|");
    model.select_next_occurrence();
    assert_eq!(render(&model), "foo [foo]");
    let mut model = parse("foo foo|");
    model.select_all_occurrences();
    assert_eq!(render(&model), "[foo] [foo]");
}

#[test]
fn whole_words_end_at_line_breaks() {
    // A run of whitespace starting a row is a whole word, as
    // `word_range_at` sees it, though a line break comes before it.
    let mut model = parse("[ ]foo\n foo");
    model.select_next_occurrence();
    assert_eq!(render(&model), "[ ]foo\n[ ]foo");
    let mut model = parse("[ ]foo\n foo");
    model.select_all_occurrences();
    assert_eq!(render(&model), "[ ]foo\n[ ]foo");
    let mut model = parse("[foo]\nfoo\nfoox");
    model.select_all_occurrences();
    assert_eq!(render(&model), "[foo]\n[foo]\nfoox");
}

#[test]
fn all_and_lines() {
    let mut model = parse("fo|o foobar foo bar_foo (foo)");
    model.select_all_occurrences();
    assert_eq!(render(&model), "[foo] foobar [foo] bar_foo ([foo])");

    let mut model = parse("a[bc\ndef\ngh]i");
    model.split_selection_into_lines();
    assert_eq!(render(&model), "a[bc]\n[def]\n[gh]i");
    model.insert("x");
    assert_eq!(render(&model), "ax|\nx|\nx|i");
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), "abc\ndef\nghi");
    // One undo step: there's nothing left to undo.
    assert_eq!(model.undo().unwrap(), None);

    // A last row reached only at its start is dropped; carets and
    // single-row selections stay.
    let mut model = parse("[ab\ncd\n]ef |g [h]");
    model.split_selection_into_lines();
    assert_eq!(render(&model), "[ab]\n[cd]\nef |g [h]");
}

#[test]
fn matches_straddle_chunks() {
    // Needles every so often, at shifting offsets, so some straddle the
    // rope's chunks.
    let mut text = String::new();
    let mut expected = Vec::new();
    for _ in 0..20 {
        text.push_str(&"-".repeat(1021));
        expected.push(text.len()..text.len() + 6);
        text.push_str("needle");
    }
    let ranges = |model: &EditorModel| -> Vec<_> {
        model
            .selections()
            .into_iter()
            .map(|selection| selection.range)
            .collect()
    };
    let mut model = EditorModel::new(fe_buffer::Buffer::new(&text), None);
    let mut boundary = 0;
    let boundaries: Vec<_> = model
        .text()
        .chunks()
        .map(|chunk| {
            boundary += chunk.len();
            boundary
        })
        .collect();
    assert!(expected.iter().any(|needle| {
        boundaries
            .iter()
            .any(|&boundary| needle.start < boundary && boundary < needle.end)
    }));
    model.set_cursor(expected[0].start);
    model.select_all_occurrences();
    assert_eq!(ranges(&model), expected);
    let mut model = EditorModel::new(fe_buffer::Buffer::new(&text), None);
    model.set_cursor(expected[0].start);
    for _ in 0..expected.len() + 1 {
        model.select_next_occurrence();
    }
    assert_eq!(ranges(&model), expected);
}

#[test]
fn single_toggle_and_box() {
    let mut model = parse("|a |b [c]");
    model.single_selection();
    assert_eq!(render(&model), "a b [c]");
    model.single_selection();
    assert_eq!(render(&model), "a b [c]");

    let mut model = parse("|abc");
    model.toggle_caret(2);
    assert_eq!(render(&model), "|ab|c");
    model.toggle_caret(2);
    assert_eq!(render(&model), "|abc");
    // Never the last selection.
    model.toggle_caret(0);
    assert_eq!(render(&model), "|abc");
    // A selection touching the offset goes.
    let mut model = parse("[ab]c|d");
    model.toggle_caret(2);
    assert_eq!(render(&model), "abc|d");

    let text = "abcdefgh\n\tefgh\nab\nabcdefgh";
    let mut model = parse(text);
    model.select_box((0, 4), (3, 6));
    assert_eq!(render(&model), "abcd[ef]gh\n\t[ef]gh\nab\nabcd[ef]gh");
    // Leftward, the selections are reversed.
    model.select_box((0, 6), (1, 4));
    assert!(
        model
            .selections()
            .iter()
            .all(|selection| selection.reversed)
    );
    assert_eq!(render(&model), "abcd[ef]gh\n\t[ef]gh\nab\nabcdefgh");
    // A zero-width box is a column of carets.
    model.select_box((0, 2), (2, 2));
    assert_eq!(render(&model), "ab|cdefgh\n|\tefgh\nab|\nabcdefgh");
    // No row reaches the box: a caret at the head.
    model.select_box((2, 5), (2, 7));
    assert_eq!(render(&model), "abcdefgh\n\tefgh\nab|\nabcdefgh");
}

#[test]
fn commands_change_only_when_selections_do() {
    use fe_command::Invocation;

    use crate::commands::{self, EditorTarget, HeadlessHost};

    let registry = commands::registry();
    let run = |model: &mut EditorModel, id: &str| {
        let mut host = HeadlessHost::new();
        registry
            .dispatch(
                &mut EditorTarget::new(model, &mut host),
                &Invocation::of(id).unwrap().context(commands::context()),
            )
            .unwrap()
            .effect
    };
    let mut model = parse("|a\nb");
    assert_eq!(run(&mut model, "editor.add_cursor_below"), Effect::Changed);
    assert_eq!(render(&model), "|a\n|b");
    assert_eq!(run(&mut model, "editor.add_cursor_below"), Effect::Nothing);
    assert_eq!(run(&mut model, "editor.single_selection"), Effect::Changed);
    assert_eq!(render(&model), "a\n|b");
    assert_eq!(run(&mut model, "editor.single_selection"), Effect::Nothing);
}
