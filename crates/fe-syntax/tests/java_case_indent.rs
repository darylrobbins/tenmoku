#![cfg(feature = "java")]

use fe_buffer::Buffer;
use fe_syntax::{
    IndentSuggestion::{self, Relative},
    Syntax, language_by_name, suggest_indent,
};

fn suggest(text: &str, row: u32) -> Option<IndentSuggestion> {
    let buffer = Buffer::new(text);
    let mut syntax = Syntax::new(language_by_name("java").unwrap(), &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    suggest_indent(&syntax, buffer.text(), row)
}

/// The suggestion for a new blank row after `row`, as Enter at its end makes.
fn after(text: &str, row: u32) -> Option<IndentSuggestion> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.insert(row as usize + 1, "");
    suggest(&lines.join("\n"), row + 1)
}

/// The suggestion for `row` as it stands.
fn at(text: &str, row: u32) -> Option<IndentSuggestion> {
    suggest(text, row)
}

fn rel(basis_row: u32, delta: i32) -> Option<IndentSuggestion> {
    Some(Relative { basis_row, delta })
}

#[test]
fn a_new_row_after_a_java_case_label_is_a_level_in() {
    let text = "class A {\n    void f(int a) {\n        switch (a) {\n            case 1:\n        }\n    }\n}";
    assert_eq!(after(text, 3), rel(3, 1));

    let text = "class A {\n    void f(int a) {\n        switch (a) {\n            case 1:\n                g();\n            default:\n                h();\n        }\n    }\n}";
    assert_eq!(after(text, 5), rel(5, 1));
    assert_eq!(at(text, 4), rel(3, 1));
}
