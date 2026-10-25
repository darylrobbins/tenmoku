use fe_command::Invocation;

use crate::{
    EditorModel,
    commands::{self, EditorTarget, HeadlessHost},
    tests::{parse, render},
};

fn dispatch(marked: &str, name: &str) -> EditorModel {
    let mut model = parse(marked);
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of(&format!("editor.{name}"))
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(&mut model, &mut host), &invocation)
        .unwrap();
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
    assert_eq!(model.undo(), Ok(None), "{name} on {marked:?} is one step");
}

#[test]
fn selection_upper_lower() {
    one_step("[abc] d [eF]g", "transform_uppercase", "[ABC] d [EF]g");
    one_step("[ABc] D [eF]G", "transform_lowercase", "[abc] D [ef]G");
}

#[test]
fn empty_selection_uses_word() {
    one_step("foo|Bar baz", "transform_uppercase", "FOO|BAR baz");
    one_step("fo|oBar baz", "transform_uppercase", "FO|OBAR baz");
    // Two carets in one word make one edit.
    one_step("f|oo|Bar", "transform_uppercase", "F|OO|BAR");
    // Between spaces there is no word: no edit, no undo step.
    let mut model = dispatch("a | b", "transform_uppercase");
    assert_eq!(render(&model), "a | b");
    assert_eq!(model.undo(), Ok(None));
    let mut model = dispatch("a - |b-", "transform_uppercase");
    assert_eq!(render(&model), "a - |B-");
    assert!(matches!(model.undo(), Ok(Some(_))));
}

#[test]
fn title_case() {
    // VS Code 1.143.1 src/vs/editor/contrib/linesOperations/browser/linesOperations.ts
    // TitleCaseAction: `text.toLocaleLowerCase().replace(
    // /(^|[^\p{L}\p{N}']|((^|\P{L})'))\p{L}/gmu, b => b.toLocaleUpperCase())`.
    // The whole match is upper-cased, boundary character included, and `_`
    // is not a letter or digit, so it starts a word, as `-` does.
    one_step(
        "[hELLO wORLD-foo_bar]",
        "transform_titlecase",
        "[Hello World-Foo_Bar]",
    );
    one_step("[a1b 2c.d]", "transform_titlecase", "[A1b 2c.D]");
    one_step(
        "[don't STOP 'tis]",
        "transform_titlecase",
        "[Don't Stop 'Tis]",
    );
    // Unicode punctuation starts a word; Greek final sigma lowers in context.
    one_step(
        "[hello\u{2014}wORLD ΟΣ]",
        "transform_titlecase",
        "[Hello\u{2014}World Ος]",
    );
    one_step("hELLO| wORLD", "transform_titlecase", "Hello| wORLD");
    // U+0345 is no letter or digit, so it is a boundary, and is upper-cased
    // with the match: to U+0399.
    one_step("[a\u{345}b]", "transform_titlecase", "[A\u{399}B]");
    // A combining mark (U+05B0) is no \p{L} or \p{N} either.
    one_step("[a\u{5B0}b]", "transform_titlecase", "[A\u{5B0}B]");
    one_step("[x\nyZ 'q ''r]", "transform_titlecase", "[X\nYz 'Q ''R]");
}

#[test]
fn overlapping_targets() {
    // A selection and a caret in the same word: one transformed range.
    one_step("fo[oB]ar| baz", "transform_uppercase", "FO[OB]AR| baz");
    one_step("f|oo|Bar", "transform_uppercase", "F|OO|BAR");
    // Growing text shifts later selections and carets.
    one_step("fo[ab]c|d", "transform_uppercase", "FO[AB]C|D");
    one_step("a|b[c d]", "transform_uppercase", "A|B[C D]");
    // A shrinking word: two carets in it keep their places.
    one_step("|K|K", "transform_lowercase", "|k|k");
    // A growing word: the second caret follows the new text before it.
    one_step("ß|ß|x", "transform_uppercase", "SS|SS|X");
    one_step("stra|ße x|y", "transform_uppercase", "STRA|SSE X|Y");
}

#[test]
fn unicode_lengths() {
    one_step("[straße]", "transform_uppercase", "[STRASSE]");
    one_step("[straße] x|y", "transform_uppercase", "[STRASSE] X|Y");
    // The caret keeps its place in a word whose text grows.
    one_step("stra|ße", "transform_uppercase", "STRA|SSE");
}
