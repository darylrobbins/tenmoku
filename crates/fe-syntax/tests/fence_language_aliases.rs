#![cfg(all(
    feature = "markdown",
    feature = "go",
    feature = "csharp",
    feature = "python",
    feature = "dockerfile",
    feature = "make",
    feature = "bash",
    feature = "javascript",
    feature = "typescript",
    feature = "json"
))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

#[test]
fn markdown_fences_named_by_common_aliases_are_highlighted() {
    for language in [
        "go",
        "csharp",
        "python",
        "dockerfile",
        "make",
        "bash",
        "javascript",
        "tsx",
        "json",
    ] {
        language_by_name(language).unwrap();
    }
    let markdown = language_by_name("markdown").unwrap();

    for (fence, source, keyword, highlight) in [
        ("golang", "func f() {}", "func", "keyword"),
        ("Golang", "func f() {}", "func", "keyword"),
        ("c#", "class C {}", "class", "keyword"),
        ("C#", "class C {}", "class", "keyword"),
        ("python3", "def f(): pass", "def", "keyword"),
        ("docker", "FROM alpine", "FROM", "keyword"),
        ("containerfile", "FROM alpine", "FROM", "keyword"),
        ("makefile", "ifeq (a,b)\nendif", "ifeq", "keyword"),
        ("Makefile", "ifeq (a,b)\nendif", "ifeq", "keyword"),
        ("shell-script", "if true; then :; fi", "if", "keyword"),
        ("node", "const a = 1;", "const", "keyword"),
        ("javascriptreact", "const a = 1;", "const", "keyword"),
        ("typescriptreact", "const a = 1;", "const", "keyword"),
        ("jsonc", "[true]", "true", "constant.builtin"),
    ] {
        let text = format!("```{fence}\n{source}\n```\n");
        let buffer = Buffer::new(&text);
        let mut syntax = Syntax::new(markdown, &buffer.snapshot());
        syntax.finish(syntax.parse_job().run());
        let found = syntax.highlights(0..text.len()).into_iter().any(|span| {
            text[span.range].starts_with(keyword) && span.highlight.name() == highlight
        });
        assert!(
            found,
            "a ```{fence} fence leaves {keyword} without {highlight}"
        );
    }
}
