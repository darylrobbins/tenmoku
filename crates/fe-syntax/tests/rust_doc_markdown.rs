//! Markdown inside Rust doc comments: link text is highlighted by an injected
//! `rustdoc_inline` layer, prose and code spans keep the comment colour.

#![cfg(all(feature = "rust", feature = "markdown"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

/// `(text, highlight)` for every span of `text`.
fn spans(text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name("rust").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    loop {
        let spans = syntax.highlights(0..text.len());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            return spans
                .into_iter()
                .map(|span| (text[span.range].to_string(), span.highlight.name()))
                .collect();
        }
        for name in missing {
            language_by_name(name);
        }
    }
}

fn has(found: &[(String, &str)], text: &str, highlight: &str) {
    assert!(
        found
            .iter()
            .any(|(t, h)| t.contains(text) && *h == highlight),
        "missing ({text:?}, {highlight:?}) in {found:?}"
    );
}

#[test]
fn line_doc_comment_spans_and_links() {
    let found = spans("/// Uses `Vec` and [`Option`].\nfn a() {}\n");
    has(&found, "/// Uses ", "comment.documentation");
    has(&found, "Vec", "comment.documentation");
    has(&found, " and ", "comment.documentation");
    has(&found, "`Option`", "markup.link");
    has(&found, ".\n", "comment.documentation");
    has(&found, "fn", "keyword");
}

#[test]
fn inner_doc_comment_spans() {
    let found = spans("//! Crate `x`.\n");
    has(&found, "//! Crate ", "comment.documentation");
    has(&found, "x", "comment.documentation");
    has(&found, ".\n", "comment.documentation");
}

#[test]
fn block_doc_comment_spans() {
    let found = spans("/** A `y` here */\nfn a() {}\n");
    has(&found, "/** A ", "comment.documentation");
    has(&found, "y", "comment.documentation");
    has(&found, " here */", "comment.documentation");
}

#[test]
fn plain_comment_is_untouched() {
    assert_eq!(
        spans("// plain `z`\n"),
        [("// plain `z`".to_string(), "comment")]
    );
}

#[test]
fn macro_bodies_are_still_rust() {
    let found = spans("fn a() { vec![1, 2]; }\n");
    has(&found, "1", "constant.builtin");
    has(&found, "2", "constant.builtin");
}

#[test]
fn unterminated_backtick_stays_in_its_comment() {
    let found = spans("/// open `tick\nfn a() { let s = 1; }\n");
    has(&found, "/// open `tick\n", "comment.documentation");
    has(&found, "fn", "keyword");
    has(&found, "let", "keyword");
    assert!(!found.iter().any(|(_, h)| *h == "markup.raw"), "{found:?}");
}
