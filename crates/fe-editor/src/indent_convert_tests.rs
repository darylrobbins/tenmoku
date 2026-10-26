use crate::{
    IndentStyle,
    tests::{parse, render},
};

#[test]
fn to_spaces() {
    let mut model = parse("\tone\n\t  t[w]o\n  \tthree\t\tx\n\n\t\n");
    model.set_indent_style(Some(IndentStyle::Tabs));
    assert!(model.convert_indentation_to_spaces());
    assert_eq!(
        render(&model),
        "    one\n      t[w]o\n    three\t\tx\n\n    \n"
    );
    assert_eq!(model.indent_style(), Some(IndentStyle::Spaces(4)));
    model.undo().unwrap();
    assert_eq!(
        model.text().to_string(),
        "\tone\n\t  two\n  \tthree\t\tx\n\n\t\n"
    );
    assert_eq!(model.undo().unwrap(), None, "one undo step");
}

#[test]
fn to_spaces_follows_the_tab_size() {
    let mut model = parse("\ta\n \t\tb");
    model.set_tab_size(2);
    model.convert_indentation_to_spaces();
    assert_eq!(model.text().to_string(), "  a\n    b");
    assert_eq!(model.indent_style(), Some(IndentStyle::Spaces(2)));
}

#[test]
fn to_tabs() {
    let mut model = parse("        one\n      tw|o\n  x\n \t y\n\"\"\"\n    in a string\n\"\"\"\n");
    assert!(model.convert_indentation_to_tabs());
    assert_eq!(
        render(&model),
        "\t\tone\n\t  tw|o\n  x\n\t y\n\"\"\"\n\tin a string\n\"\"\"\n"
    );
    assert_eq!(model.indent_style(), Some(IndentStyle::Tabs));
    model.undo().unwrap();
    assert_eq!(model.undo().unwrap(), None, "one undo step");
}

#[test]
fn detect() {
    let two = "a:\n  b:\n    c: 1\n    d: 2\n  e:\n    f: 3\n";
    let mut model = parse(two);
    model.set_indent_style(Some(IndentStyle::Tabs));
    assert!(model.detect_indentation());
    assert_eq!(model.indent_style(), Some(IndentStyle::Spaces(2)));
    assert!(!model.detect_indentation(), "already so");
    // Too few indented rows to tell.
    let mut model = parse("a\n  b\nc\n");
    model.set_indent_style(Some(IndentStyle::Tabs));
    assert!(!model.detect_indentation());
    assert_eq!(model.indent_style(), Some(IndentStyle::Tabs));
    assert_eq!(model.undo().unwrap(), None);
}

#[test]
fn empty_edit_is_unchanged() {
    let mut model = parse("a\n  b|\n\nc");
    model.set_indent_style(Some(IndentStyle::Spaces(4)));
    assert!(!model.convert_indentation_to_spaces());
    assert_eq!(render(&model), "a\n  b|\n\nc");
    assert_eq!(model.undo().unwrap(), None);
    assert!(!model.is_dirty());
    let mut model = parse("a\n\tb\n\n");
    model.set_indent_style(Some(IndentStyle::Tabs));
    assert!(!model.convert_indentation_to_tabs());
    assert_eq!(model.undo().unwrap(), None);
    assert!(!model.is_dirty());
}

#[test]
fn are_registered_commands() {
    let registry = crate::commands::registry();
    for (id, title) in [
        (
            "editor.convert_indentation_to_spaces",
            "Convert Indentation to Spaces",
        ),
        (
            "editor.convert_indentation_to_tabs",
            "Convert Indentation to Tabs",
        ),
        (
            "editor.detect_indentation",
            "Detect Indentation from Content",
        ),
    ] {
        let meta = registry.commands().find(|meta| meta.id.as_str() == id);
        assert_eq!(
            meta.map(|m| m.title.to_string()).as_deref(),
            Some(title),
            "{id}"
        );
    }
}

#[test]
fn selections_inside_the_indentation_keep_their_column() {
    let mut model = parse("\t|\tx\n \t[ ]y");
    model.convert_indentation_to_spaces();
    assert_eq!(render(&model), "    |    x\n    [ ]y");
    let mut model = parse("      |  x\n  [  ]  y");
    model.convert_indentation_to_tabs();
    assert_eq!(render(&model), "\t|\tx\n[\t]  y");
}
