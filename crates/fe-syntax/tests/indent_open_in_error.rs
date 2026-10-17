#![cfg(all(feature = "javascript", feature = "typescript", feature = "java"))]

use fe_buffer::Buffer;
use fe_syntax::{
    IndentSuggestion::{self, Relative},
    Syntax, language_by_name, suggest_indent,
};

/// The suggestion for a new blank row after `row`, as Enter at its end makes.
fn after(language: &str, text: &str, row: u32) -> Option<IndentSuggestion> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.insert(row as usize + 1, "");
    let buffer = Buffer::new(&lines.join("\n"));
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    suggest_indent(&syntax, buffer.text(), row + 1)
}

fn rel(basis_row: u32, delta: i32) -> Option<IndentSuggestion> {
    Some(Relative { basis_row, delta })
}

#[test]
fn an_unclosed_arrow_function_body_indents_the_next_row() {
    for language in ["javascript", "typescript", "tsx"] {
        assert_eq!(
            after(language, "const f = () => {", 0),
            rel(0, 1),
            "{language}"
        );
        assert_eq!(
            after(language, "function g() {\n    const f = () => {", 1),
            rel(1, 1),
            "{language} nested"
        );
    }
    assert_eq!(after("typescript", "const f = (): void => {", 0), rel(0, 1));
}

#[test]
fn an_unclosed_java_switch_block_indents_the_next_row() {
    let text = "class A {\n    void f(int a) {\n        switch (a) {";
    assert_eq!(after("java", text, 2), rel(2, 1));
}
