use super::*;
use crate::tests::{parse, render};
use fe_command::{Actor, Invocation};
use fe_display::Fold;

use crate::commands::{self, EditorTarget, HeadlessHost};

/// Runs `editor.<name>` on `marked` and returns the model and its effect.
fn dispatch(marked: &str, name: &str) -> (EditorModel, Effect) {
    let mut model = parse(marked);
    let effect = dispatch_on(&mut model, name);
    (model, effect)
}

fn dispatch_on(model: &mut EditorModel, name: &str) -> Effect {
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of(&format!("editor.{name}"))
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap()
        .effect
}

fn ran(marked: &str, run: impl Fn(&mut EditorModel)) -> String {
    let mut model = parse(marked);
    run(&mut model);
    render(&model)
}

/// Runs `name` on `marked`, checks it gives `expected` as one undo step,
/// and that redoing gives it again.
fn one_step(marked: &str, name: &str, expected: &str) {
    let (mut model, effect) = dispatch(marked, name);
    assert_eq!(render(&model), expected, "{name} on {marked:?}");
    assert_eq!(effect, Effect::Changed, "{name} on {marked:?}");
    let before = parse(marked).text().to_string();
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), before, "undoing {name}");
    model.redo().unwrap();
    assert_eq!(
        model.text().to_string(),
        parse(expected).text().to_string(),
        "redoing {name}"
    );
}

#[test]
fn indent_and_outdent() {
    // The selection ends at row 3's start, so row 3 isn't touched; the
    // blank row 1 isn't indented.
    one_step("[a\n\n  b\n]c", "indent", "    [a\n\n      b\n]c");
    one_step("    [a\n\n      b\n]c", "outdent", "[a\n\n  b\n]c");
    // Fewer spaces than a level all go, a tab goes alone, and a row with
    // no leading whitespace is left alone.
    one_step("[  a\n\t\tb\nc]", "outdent", "[a\n\tb\nc]");
    one_step("    a\n\t  |b", "outdent", "    a\n  |b");
    let (model, effect) = dispatch("[a\nb]", "outdent");
    assert_eq!((render(&model), effect), ("[a\nb]".into(), Effect::Nothing));

    // A caret or selection end at a row's start moves with it; one inside
    // the indentation going goes to the row's start.
    assert_eq!(
        ran("|ab\nc|d", EditorModel::indent_rows),
        "    |ab\n    c|d"
    );
    assert_eq!(ran("  a|b", EditorModel::indent_rows), "      a|b");
    assert_eq!(ran("  |  ab", EditorModel::outdent_rows), "|ab");
    // A reversed selection stays reversed.
    let mut model = parse("[a\nb]");
    let range = model.selections()[0].range.clone();
    model.select_range(range, true);
    model.indent_rows();
    assert_eq!(render(&model), "    [a\n    b]");
    assert!(model.primary_selection().reversed);

    // Selections on adjacent rows each indent their rows once.
    assert_eq!(
        ran("[a\nb]\n|c\nd\n|e", EditorModel::indent_rows),
        "    [a\n    b]\n    |c\nd\n    |e"
    );
    // The tab size counts.
    let mut model = parse("[a\nb]");
    model.set_tab_size(2);
    model.indent_rows();
    assert_eq!(render(&model), "  [a\n  b]");
}

#[test]
fn tab_indents_multi_row_selections() {
    one_step("a[b\nc]d", "tab", "    a[b\n    c]d");
    // A selection ending at a row's start touches one row: it's replaced.
    assert_eq!(ran("[ab\n]c", EditorModel::tab), "    |c");
    // Carets and single-row selections behave as before.
    assert_eq!(ran("a|b\nc", EditorModel::tab), "a   |b\nc");
    assert_eq!(ran("a[bc]d", EditorModel::tab), "a   |d");
    // One multi-row selection among others indents every touched row.
    assert_eq!(
        ran("a|b\n[c\nd]", EditorModel::tab),
        "    a|b\n    [c\n    d]"
    );
}

#[test]
fn move_rows() {
    one_step("[a\nb]\nc", "move_rows_down", "c\n[a\nb]");
    one_step("c\n[a\nb]", "move_rows_up", "[a\nb]\nc");
    // The last row, with no line break after it, keeps none.
    one_step("a\nb|", "move_rows_up", "b|\na");
    one_step("a|\nb", "move_rows_down", "b\na|");
    // The break the selection covered is now before it.
    one_step("a\n[b\n]c", "move_rows_down", "a\nc\n[b]");
    one_step("a\n\n|", "move_rows_up", "a\n|\n");
    one_step("a\n|\n", "move_rows_down", "a\n\n|");
    // A selection ending at the last row's start, which it doesn't touch,
    // stays on its text.
    one_step("[a\n]b", "move_rows_down", "b\n[a]");

    // Nothing at the top or bottom, even for blocks elsewhere.
    for (marked, name) in [
        ("|a\nb", "move_rows_up"),
        ("a\n|b", "move_rows_down"),
        ("|a\nb\n|c", "move_rows_up"),
        ("|a\nb\n|c", "move_rows_down"),
    ] {
        let (model, effect) = dispatch(marked, name);
        assert_eq!((render(&model), effect), (marked.into(), Effect::Nothing));
    }

    // Blocks move together, each past its own row.
    assert_eq!(
        ran("a\n|b\nc\nd\n[e\nf]", |model| model.move_rows(true)),
        "|b\na\nc\n[e\nf]\nd"
    );

    // A folded range is passed whole and stays folded.
    let text = "|x\ndef a():\n    one\n    two\nend";
    let mut model = parse(text);
    model.fold_rows(1..=3);
    model.move_rows(false);
    assert_eq!(render(&model), "def a():\n    one\n    two\n|x\nend");
    assert_eq!(model.display().folds(), [Fold { start: 0, end: 2 }]);
    model.move_rows(true);
    assert_eq!(render(&model), text);
    assert_eq!(model.display().folds(), [Fold { start: 1, end: 3 }]);
    // Up to the last row too.
    let mut model = parse("def a():\n    one\n    two\n|x");
    model.fold_rows(0..=2);
    model.move_rows(true);
    assert_eq!(render(&model), "|x\ndef a():\n    one\n    two");
    assert_eq!(model.display().folds(), [Fold { start: 1, end: 3 }]);
    model.move_rows(false);
    assert_eq!(render(&model), "def a():\n    one\n    two\n|x");
    assert_eq!(model.display().folds(), [Fold { start: 0, end: 2 }]);
    // A block on a fold's header takes the folded rows with it.
    let mut model = parse("de|f a():\n    one\nend");
    model.fold_rows(0..=1);
    model.move_rows(false);
    assert_eq!(render(&model), "end\nde|f a():\n    one");
    assert_eq!(model.display().folds(), [Fold { start: 1, end: 2 }]);
}

#[test]
fn duplicate_and_delete() {
    one_step("a\n[b]\nc", "duplicate_rows", "a\nb\n[b]\nc");
    one_step("a\nb|", "duplicate_rows", "a\nb\nb|");
    one_step("[a\nb\n]c", "duplicate_rows", "a\nb\n[a\nb\n]c");
    // Each block is copied below itself.
    one_step("|a\nx\nb|", "duplicate_rows", "a\n|a\nx\nb\nb|");
    // A folded range is copied whole.
    let mut model = parse("de|f a():\n    one\nend");
    model.fold_rows(0..=1);
    model.duplicate_rows();
    assert_eq!(render(&model), "def a():\n    one\nde|f a():\n    one\nend");

    // The caret keeps its column on the row taking the block's place.
    one_step("a\nbb|\nccc", "delete_rows", "a\ncc|c");
    // On the last row, the break before it goes too.
    one_step("abc\nd|e", "delete_rows", "a|bc");
    // Clamped to the row's end.
    one_step("a\nbbbb|\nc", "delete_rows", "a\nc|");
    // Display columns count tabs.
    one_step("a\n\tx|\nabcdefgh", "delete_rows", "a\nabcde|fgh");
    one_step("a\nabcde|fgh\n\tx", "delete_rows", "a\n\tx|");
    // A column inside a tab goes before it.
    one_step("a\nab|c\n\tx", "delete_rows", "a\n|\tx");
    // A multi-row block goes whole; its carets go where their heads were.
    one_step("a|b\nc|d\ne", "delete_rows", "e|");
    one_step("[ab\nc]\nde", "delete_rows", "d|e");
    // Everything.
    one_step("a|\nb", "delete_rows", "b|");
    one_step("[a\nb]", "delete_rows", "|");
    // A folded range goes whole.
    let mut model = parse("de|f a():\n    one\nend");
    model.fold_rows(0..=1);
    model.delete_rows();
    assert_eq!(render(&model), "en|d");
}

#[test]
fn row_commands_run_as_the_actor() {
    let mut model = parse("[a\nb]");
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of("editor.indent")
        .unwrap()
        .context(commands::context())
        .actor(Actor::Ai {
            agent: "agent".into(),
            session: "s".into(),
        });
    let dispatched = commands::registry()
        .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
        .unwrap();
    assert_eq!(dispatched.transactions.len(), 1);
    // The user can't undo it.
    model.undo().unwrap();
    assert_eq!(render(&model), "    [a\n    b]");
}
