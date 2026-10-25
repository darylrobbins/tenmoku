//! Rust doc comments colour link text and code spans, as VS Code does: escapes
//! and emphasis stay in the comment colour. `.md` files keep theirs.

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

fn none_of(found: &[(String, &str)], highlights: &[&str]) {
    for (text, highlight) in found {
        assert!(
            !highlights.iter().any(|h| highlight.contains(h)),
            "unexpected ({text:?}, {highlight:?}) in {found:?}"
        );
    }
}

#[test]
fn escapes_and_emphasis_keep_the_comment_colour() {
    let found = spans(
        "rust",
        "/// Uses `Vec` and [`Option`], \\_ *e*.\nfn a() {}\n",
    );
    none_of(
        &found,
        &[
            "markup.raw",
            "punctuation.delimiter",
            "escape",
            "markup.italic",
            "emphasis",
        ],
    );
    has(&found, "`Vec`", "comment.documentation.code");
    has(&found, "`Option`", "comment.documentation.code");
}

#[test]
fn inner_and_block_doc_comments_agree() {
    for text in ["//! A `x`.\n", "/** A `x` */\nfn a() {}\n"] {
        let found = spans("rust", text);
        none_of(&found, &["markup.raw", "punctuation.delimiter"]);
        has(&found, "`x`", "comment.documentation.code");
    }
}

#[test]
fn markdown_files_still_colour_code_spans() {
    let found = spans("markdown", "Uses `Vec` here.\n");
    has(&found, "Vec", "markup.raw");
}
