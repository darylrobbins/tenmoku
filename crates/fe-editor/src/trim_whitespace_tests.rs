use fe_command::Effect;

use crate::tests::{parse, render};

#[test]
fn whole_document() {
    let marked = "a  \n\t \nb\t\nc \td \n   ";
    let mut model = parse(marked);
    model.trim_trailing_whitespace();
    assert_eq!(render(&model), "a\n\nb\nc \td\n");
    model.undo().unwrap();
    assert_eq!(model.text().to_string(), parse(marked).text().to_string());
    assert_eq!(model.undo().unwrap(), None, "one undo step");
}

#[test]
fn caret_keeps_whitespace_before_it() {
    let mut model = parse("x  |  \ny \t|\t\nz | ");
    model.trim_trailing_whitespace();
    assert_eq!(render(&model), "x  |\ny \t|\nz |");
    // Several carets on one row: the last one decides.
    let mut model = parse("a| b|  \nc|  |  ");
    model.trim_trailing_whitespace();
    assert_eq!(render(&model), "a| b|\nc|  |");
    // Carets on several rows.
    let mut model = parse("a |  \nb |  \nc   ");
    model.trim_trailing_whitespace();
    assert_eq!(render(&model), "a |\nb |\nc");
}

#[test]
fn selections_stay_on_their_text() {
    let mut model = parse("[a  ]\nb [ \n ]c");
    model.trim_trailing_whitespace();
    assert_eq!(render(&model), "[a]\nb[\n ]c");
}

#[test]
fn no_change_no_edit() {
    let mut model = parse("a\n b|\n\nc");
    let before = model.text().to_string();
    model.trim_trailing_whitespace();
    assert_eq!(model.text().to_string(), before);
    assert_eq!(model.undo().unwrap(), None);
    assert!(!model.is_dirty());
    // A caret keeping the only whitespace there is makes no edit either.
    let mut model = parse("a  |");
    model.trim_trailing_whitespace();
    assert_eq!(render(&model), "a  |");
    assert_eq!(model.undo().unwrap(), None);
}

#[test]
fn is_a_registered_command() {
    let registry = crate::commands::registry();
    let meta = registry
        .commands()
        .find(|meta| meta.id.as_str() == "editor.trim_trailing_whitespace");
    assert_eq!(
        meta.map(|meta| meta.title.to_string()).as_deref(),
        Some("Trim Trailing Whitespace")
    );
    let _ = Effect::Changed;
}
