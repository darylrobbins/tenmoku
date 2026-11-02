#![cfg(all(
    feature = "markdown",
    feature = "html",
    feature = "php",
    feature = "sql",
    feature = "xml",
    feature = "javascript",
    feature = "json",
    feature = "json5",
    feature = "python",
    feature = "scala",
    feature = "bash",
    feature = "ruby",
    feature = "java"
))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

#[test]
fn markdown_fences_named_as_vscode_names_them_are_highlighted() {
    for language in [
        "html",
        "php",
        "sql",
        "xml",
        "javascript",
        "json",
        "json5",
        "python",
        "scala",
        "bash",
        "ruby",
        "java",
    ] {
        language_by_name(language).unwrap();
    }
    let markdown = language_by_name("markdown").unwrap();

    for (fence, source, keyword) in [
        ("xhtml", "<p>x</p>", "p"),
        ("shtml", "<p>x</p>", "p"),
        ("phtml", "<?php if (1) {} ?>", "if"),
        ("php5", "<?php if (1) {} ?>", "if"),
        ("ddl", "SELECT 1;", "SELECT"),
        ("xsd", "<a/>", "a"),
        ("rss", "<a/>", "a"),
        ("xslt", "<a/>", "a"),
        ("es6", "const a = 1;", "const"),
        ("json5", "[true]", "true"),
        ("py3", "def f(): pass", "def"),
        ("gyp", "def f(): pass", "def"),
        ("sbt", "object A", "object"),
        ("bashrc", "if true; then :; fi", "if"),
        ("rbx", "def f; end", "def"),
        ("Gemfile", "def f; end", "def"),
        ("bsh", "class C {}", "class"),
        ("js:app.js", "const a = 1;", "const"),
        ("python:main.py", "def f(): pass", "def"),
    ] {
        let text = format!("```{fence}\n{source}\n```\n");
        let fence_start = text.find('\n').unwrap();
        let buffer = Buffer::new(&text);
        let mut syntax = Syntax::new(markdown, &buffer.snapshot());
        syntax.finish(syntax.parse_job().run());
        let found = syntax
            .highlights(0..text.len())
            .into_iter()
            .any(|span| span.range.start > fence_start && text[span.range].starts_with(keyword));
        assert!(found, "a ```{fence} fence leaves {keyword} unhighlighted");
    }
}
