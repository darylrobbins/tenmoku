use fe_command::Invocation;

use crate::{
    EditorModel,
    commands::{self, EditorTarget, HeadlessHost},
    tests::{parse, render},
};

fn dispatch(marked: &str, name: &str) -> EditorModel {
    let mut model = parse(marked);
    run(&mut model, name);
    model
}

fn run(model: &mut EditorModel, name: &str) {
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of(&format!("editor.{name}"))
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap();
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
fn delete_all_left() {
    let name = "delete_all_left";
    one_step("    foo ba|r baz", name, "|r baz");
    one_step("one\n  two|", name, "one\n|");
    // At column 0 the row break before goes.
    one_step("one\n|two", name, "one|two");
    one_step("|one", name, "|one");
    one_step("a[bc]d", name, "a|d");
    // Carets on different rows, and ones whose ranges touch.
    one_step("ab|c\nde|f", name, "|c\n|f");
    one_step("ab|c|d", name, "|d");
    one_step("ab|\n|cd", name, "|cd");
}

#[test]
fn delete_all_right() {
    let name = "delete_all_right";
    one_step("fo|o bar\nbaz", name, "fo|\nbaz");
    // At the row end the row break goes.
    one_step("one|\ntwo", name, "one|two");
    one_step("one|", name, "one|");
    one_step("a[bc]d", name, "a|d");
    one_step("a|b\nc|d", name, "a|\nc|");
    one_step("a|b|cd\nx", name, "a|\nx");
}

#[test]
fn delete_word_right() {
    let name = "delete_word_right";
    one_step("|foo bar", name, "| bar");
    one_step("foo| bar baz", name, "foo| baz");
    one_step("a|  ++ b", name, "a| b");
    one_step("a|b\nc", name, "a|\nc");
    one_step("foo|\nbar", name, "foo|bar");
    one_step("foo|", name, "foo|");
    one_step("a[bc]d", name, "a|d");
    // Mirrors delete_word_left.
    one_step("foo bar|", "delete_word_left", "foo |");
    one_step("|foo bar", name, "| bar");
}

#[test]
fn graphemes_and_crlf() {
    // Combining mark: never split.
    one_step("e\u{301}|x", "delete_all_left", "|x");
    one_step("a|e\u{301}x", "delete_word_right", "a|x");
    one_step("a\n|e\u{301}", "delete_all_left", "a|e\u{301}");
    // A caret inside a cluster (word movement stops there) deletes it whole.
    one_step("a\ne|\u{301}x", "delete_all_left", "a\n|x");
    one_step("e|\u{301}x", "delete_all_right", "|");
    one_step("e|\u{301}x y", "delete_word_right", "|x y");
    // Emoji ZWJ sequence.
    let family = "\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}";
    one_step(&format!("a|{family}b"), "delete_all_right", "a|");
    one_step(&format!("a|{family}\nb"), "delete_all_right", "a|\nb");
    one_step(
        &format!("{family}|\nb"),
        "delete_all_right",
        &format!("{family}|b"),
    );
    one_step(
        &format!("{family}\n|b"),
        "delete_all_left",
        &format!("{family}|b"),
    );
    // A CRLF row break is one `\n` in the buffer, so it goes whole.
    for (marked, name, expected, saved) in [
        ("a|\nb", "delete_all_right", "a|b", b"ab".as_slice()),
        ("a\n|b", "delete_all_left", "a|b", b"ab"),
        ("a|\nb\nc", "delete_word_right", "a|b\nc", b"ab\r\nc"),
    ] {
        let mut model = parse(marked);
        model.set_end_of_line_crlf();
        run(&mut model, name);
        assert_eq!(render(&model), expected, "{name} on {marked:?}");
        let mut out = Vec::new();
        model.buffer().write_to(&mut out).unwrap();
        assert_eq!(out, saved, "{name} on {marked:?}");
    }
}
