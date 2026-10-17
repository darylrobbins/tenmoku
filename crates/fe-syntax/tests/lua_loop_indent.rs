#![cfg(feature = "lua")]

use fe_buffer::Buffer;
use fe_syntax::{
    IndentSuggestion::{self, Relative},
    Syntax, language_by_name, suggest_indent,
};

/// The suggestion for a new blank row after `row`, as Enter at its end makes.
fn after(text: &str, row: u32) -> Option<IndentSuggestion> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.insert(row as usize + 1, "");
    let text = lines.join("\n");
    let buffer = Buffer::new(&text);
    let mut syntax = Syntax::new(language_by_name("lua").unwrap(), &buffer.snapshot());
    syntax.finish(syntax.parse_job().run());
    suggest_indent(&syntax, buffer.text(), row + 1)
}

#[test]
fn an_unclosed_lua_loop_indents_its_next_row() {
    for header in [
        "for i = 1, 2 do",
        "for k, v in pairs(t) do",
        "while x do",
        "do",
        "repeat",
    ] {
        assert_eq!(
            after(header, 0),
            Some(Relative {
                basis_row: 0,
                delta: 1
            }),
            "{header}"
        );
    }
    for (text, row) in [
        ("function f()\n    for i = 1, 2 do", 1),
        ("if x then\n    while y do", 1),
    ] {
        assert_eq!(
            after(text, row),
            Some(Relative {
                basis_row: 1,
                delta: 1
            }),
            "{text}"
        );
    }
}
