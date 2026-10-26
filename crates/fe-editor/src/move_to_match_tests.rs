use fe_command::Invocation;

use crate::{
    EditorModel,
    commands::{self, EditorTarget, HeadlessHost},
    tests::{parse, render},
};

const NAME: &str = "move_last_selection_to_next_find_match";

fn run(model: &mut EditorModel) {
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of(&format!("editor.{NAME}"))
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap();
}

fn dispatch(marked: &str) -> EditorModel {
    let mut model = parse(marked);
    run(&mut model);
    model
}

#[test]
fn moves_the_last_selection() {
    let model = dispatch("[a] x [foo] y foo z foo");
    assert_eq!(render(&model), "[a] x foo y [foo] z foo");
    assert_eq!(model.primary_selection().range, 10..13);
    assert_eq!(model.selections().len(), 2);
    // Whole words only when the selection is one.
    let model = dispatch("[a] [foo] foobar foo");
    assert_eq!(render(&model), "[a] foo foobar [foo]");
    let model = dispatch("[a] x[foo] foobar");
    assert_eq!(render(&model), "[a] xfoo [foo]bar");
}

#[test]
fn empty_selection_uses_the_word() {
    let mut model = parse("fo|o foobar foo");
    run(&mut model);
    assert_eq!(render(&model), "[foo] foobar foo");
    run(&mut model);
    assert_eq!(render(&model), "foo foobar [foo]");
}

#[test]
fn wraps_and_skips() {
    let model = dispatch("foo bar [foo]");
    assert_eq!(render(&model), "[foo] bar foo");
    // Landing above a retained selection, it is not the primary (the
    // bottommost, which the view follows): the documented limit.
    let model = dispatch("foo [bar] [foo]");
    assert_eq!(render(&model), "[foo] [bar] foo");
    assert_eq!(model.primary_selection().range, 4..7);
    // A match another caret holds is skipped.
    let model = dispatch("[foo] foo [foo] foo");
    assert_eq!(render(&model), "[foo] foo foo [foo]");
    let model = dispatch("[foo] foo foo [foo]");
    assert_eq!(render(&model), "[foo] [foo] foo foo");
    // No other match: nothing changes.
    let model = dispatch("[foo] bar");
    assert_eq!(render(&model), "[foo] bar");
    let model = dispatch("[foo] [foo]");
    assert_eq!(render(&model), "[foo] [foo]");
    // A word over the length limit is no target.
    let long = "a".repeat(300);
    let model = dispatch(&format!("[{long}] {long}"));
    assert_eq!(render(&model), format!("[{long}] {long}"));
}

#[test]
fn undo() {
    let mut model = parse("|a foo foo");
    model.insert("b");
    model.select_range(3..6, false);
    let before = render(&model);
    assert_eq!(before, "ba [foo] foo");
    run(&mut model);
    let moved = render(&model);
    assert_eq!(moved, "ba foo [foo]");
    // One Cursor Undo step back, no text edit.
    cursor_undo(&mut model);
    assert_eq!(render(&model), before);
    // Normal Undo after the command changes nothing it did: it reverts
    // only the earlier text edit.
    run(&mut model);
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), "a foo foo");

    // Seeding a caret is one step too.
    let mut model = parse("fo|o foo");
    run(&mut model);
    assert_eq!(render(&model), "[foo] foo");
    cursor_undo(&mut model);
    assert_eq!(render(&model), "fo|o foo");
    assert_eq!(model.undo(), Ok(None));
}

fn cursor_undo(model: &mut EditorModel) {
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of("editor.cursor_undo")
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap();
}

#[test]
fn a_caret_is_seeded_only_when_it_is_the_primary() {
    // Another caret stays one while the primary selection moves.
    let model = dispatch("a|bc [foo] foo");
    assert_eq!(render(&model), "a|bc foo [foo]");
    let model = dispatch("a| [foo] foo");
    assert_eq!(render(&model), "a| foo [foo]");
    // A caret primary seeds every caret, then moves nothing.
    let model = dispatch("a|bc [foo] fo|o");
    assert_eq!(render(&model), "[abc] [foo] [foo]");
}

#[test]
fn a_word_over_the_limit_is_left_alone_with_a_caret() {
    for long in ["a".repeat(300), "é".repeat(150)] {
        let (head, tail) = long.split_at(long.chars().next().unwrap().len_utf8());
        let marked = format!("{head}|{tail} {long}");
        let model = dispatch(&marked);
        assert_eq!(render(&model), marked);
    }
    let at_limit = "a".repeat(256);
    let model = dispatch(&format!("{}|a x", &at_limit[1..]));
    assert_eq!(render(&model), format!("[{at_limit}] x"));
}
