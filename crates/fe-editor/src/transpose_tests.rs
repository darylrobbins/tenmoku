use fe_command::{Effect, Invocation};

use crate::{
    EditorModel,
    commands::{self, EditorTarget, HeadlessHost},
    tests::{parse, render},
};

fn dispatch(marked: &str) -> (EditorModel, Effect) {
    let mut model = parse(marked);
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of("editor.transpose_letters")
        .unwrap()
        .context(commands::context());
    let effect = commands::registry()
        .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
        .unwrap()
        .effect;
    (model, effect)
}

/// Checks `marked` becomes `expected` as one undo step.
fn one_step(marked: &str, expected: &str) {
    let (mut model, effect) = dispatch(marked);
    assert_eq!(render(&model), expected, "on {marked:?}");
    assert_eq!(effect, Effect::Changed, "on {marked:?}");
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), parse(marked).text().to_string());
    assert_eq!(model.undo(), Ok(None), "{marked:?} is one step");
}

fn unchanged(marked: &str) {
    let (mut model, effect) = dispatch(marked);
    assert_eq!((render(&model), effect), (marked.into(), Effect::Nothing));
    assert_eq!(model.undo(), Ok(None), "{marked:?} makes no step");
}

#[test]
fn swaps_around_the_caret() {
    one_step("a|b", "ba|");
    one_step("x|yz", "yx|z");
}

#[test]
fn row_edges() {
    one_step("abc|", "acb|");
    one_step("a\nbc|\nd", "a\ncb|\nd");
    unchanged("|ab");
    unchanged("a\n|bc");
    unchanged("a|");
    unchanged("a\nb|\nc");
}

#[test]
fn graphemes_and_carets() {
    one_step("a|👨‍👩‍👧z", "👨‍👩‍👧a|z");
    one_step("e\u{301}|x", "xe\u{301}|");
    one_step("a|bc\nd|ef", "ba|c\ned|f");
    // Disjoint pairs on one row both swap.
    one_step("a|bc|d", "ba|dc|");
    // Pairs that would overlap: the later caret is dropped.
    one_step("a|b|cd", "ba|cd");
    unchanged("[a]b");
}
