//! Backtick code spans in Rust doc comments are brighter than the prose, as in
//! VS Code. `.md` files keep their own colouring.

#![cfg(all(feature = "rust", feature = "markdown"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

/// `(text, highlight)` for every span of `text` in the language `language`.
fn spans(language: &str, text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(
        language_by_name(language).unwrap(),
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

const CODE: &str = "comment.documentation.code";
const PROSE: &str = "comment.documentation";

#[test]
fn a_backtick_span_in_a_doc_comment_is_not_the_prose_colour() {
    for text in [
        "/// Uses `Vec` and [`Config`] here.\nfn a() {}\n",
        "//! Uses `Vec` and [`Config`] here.\n",
        "/** Uses `Vec` and [`Config`] here. */\nfn a() {}\n",
    ] {
        let found = spans("rust", text);
        has(&found, "`Vec`", CODE);
        has(&found, "Config", CODE);
        has(&found, " and [", PROSE);
        has(&found, "] here.", PROSE);
    }

    let found = spans("rust", "/// - item `x`\nfn a() {}\n");
    has(&found, "- item ", PROSE);
    has(&found, "`x`", CODE);

    let found = spans("markdown", "Uses `Vec` here.\n");
    has(&found, "Vec", "markup.raw");
}
