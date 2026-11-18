//! Markdown's markup in our vocabulary (T1924): strong text, emphasis,
//! strikethrough, block quotes, list markers and link labels each have a
//! highlight, grammars' other names for them map onto it, and the
//! `**`, `*` and `~~` around them stay punctuation. A Rust doc comment's
//! emphasis has names of its own under `comment.documentation`, so it keeps
//! the comment's colour.

#![cfg(all(feature = "rust", feature = "markdown"))]

use fe_buffer::Buffer;
use fe_syntax::{Highlight, Syntax, language_by_name};

/// `(text, highlight)` for every span of `text` in the language `language`,
/// once its injected languages have loaded.
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
        found.iter().any(|(t, h)| t == text && *h == highlight),
        "missing ({text:?}, {highlight:?}) in {found:?}"
    );
}

#[test]
fn the_markup_names_are_in_the_vocabulary() {
    for name in [
        "markup.strong",
        "markup.italic",
        "markup.strikethrough",
        "markup.quote",
        "markup.list",
        "markup.link.label",
        "comment.documentation.strong",
        "comment.documentation.italic",
        "comment.documentation.strikethrough",
    ] {
        let highlight = Highlight::from_capture(name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(highlight.name(), name);
    }
}

#[test]
fn grammar_names_map_onto_the_markup_names() {
    for (capture, name) in [
        // nvim-treesitter's older names, which tree-sitter-md's queries use.
        ("text.strong", "markup.strong"),
        ("text.emphasis", "markup.italic"),
        ("text.strike", "markup.strikethrough"),
        ("text.quote", "markup.quote"),
        ("text.title", "markup.heading"),
        ("text.literal", "markup.raw"),
        ("text.uri", "markup.link.url"),
        ("text.reference", "markup.link"),
        // Helix's.
        ("markup.bold", "markup.strong"),
        ("markup.strike", "markup.strikethrough"),
        ("markup.emphasis", "markup.italic"),
        ("markup.list.unnumbered", "markup.list"),
        ("markup.list.numbered", "markup.list"),
        ("markup.list.checked", "markup.list"),
        ("markup.link.label", "markup.link.label"),
        ("markup.link.text", "markup.link"),
        ("markup.quote.markdown", "markup.quote"),
        // Zed's.
        ("emphasis", "markup.italic"),
        ("emphasis.strong", "markup.strong"),
        ("title", "markup.heading"),
        ("link_text", "markup.link"),
        ("link_uri", "markup.link.url"),
        ("punctuation.list_marker", "markup.list"),
    ] {
        let found = Highlight::from_capture(capture).map(Highlight::name);
        assert_eq!(found, Some(name), "{capture}");
    }
}

#[test]
fn markdown_strong_emphasis_and_strikethrough_are_styled_inside_their_delimiters() {
    let found = spans(
        "markdown",
        "Some **bold**, _slanted_, *more* and ~~gone~~ words.\n",
    );
    has(&found, "bold", "markup.strong");
    has(&found, "slanted", "markup.italic");
    has(&found, "more", "markup.italic");
    has(&found, "gone", "markup.strikethrough");
    for delimiter in ["**", "_", "*", "~~"] {
        has(&found, delimiter, "punctuation.delimiter");
    }
    // Nothing else is styled: the prose between is the text's own.
    assert!(
        !found
            .iter()
            .any(|(text, _)| text.contains("Some") || text.contains("words")),
        "{found:?}"
    );
}

#[test]
fn markdown_quotes_list_markers_and_link_labels() {
    let text = "> A quote with *emphasis*.\n\n\
                - one\n  - nested\n1. first\n- [x] done\n\n\
                See [the docs][docs].\n\n\
                [docs]: https://example.com/docs\n";
    let found = spans("markdown", text);
    // The quote's text; its `>` and the emphasis inside keep theirs.
    has(&found, "A quote with ", "markup.quote");
    has(&found, "> ", "punctuation.special");
    has(&found, "emphasis", "markup.italic");
    has(&found, "- ", "markup.list");
    has(&found, "1. ", "markup.list");
    has(&found, "- [x]", "markup.list");
    has(&found, "the docs", "markup.link");
    for label in found.iter().filter(|(text, _)| text == "[docs]") {
        assert_eq!(label.1, "markup.link.label", "{found:?}");
    }
    assert_eq!(found.iter().filter(|(text, _)| text == "[docs]").count(), 2);
}

#[test]
fn doc_comment_emphasis_keeps_the_comment_colour() {
    let found = spans(
        "rust",
        "/// A **bold** claim, *slanted* and ~~gone~~.\nfn a() {}\n",
    );
    has(&found, "bold", "comment.documentation.strong");
    has(&found, "slanted", "comment.documentation.italic");
    has(&found, "gone", "comment.documentation.strikethrough");
    for (text, highlight) in &found {
        assert!(!highlight.starts_with("markup"), "{text:?} {highlight}");
    }
}

#[test]
fn doc_comment_quotes_and_list_items_are_block_markdown() {
    let text = "/// Intro with *slanted* text.\n\
                /// > Quoted with **bold**,\n\
                /// > over two lines.\n\
                ///\n\
                /// - one `x`\n\
                ///   - nested\n\
                ///     - deeper\n\
                ///\n\
                /// 1. first\n\
                //! - inner\n\
                fn a() {}\n";
    let found = spans("rust", text);
    has(&found, "Quoted with ", "comment.documentation.quote");
    has(&found, "over two lines.\n", "comment.documentation.quote");
    has(&found, "bold", "comment.documentation.strong");
    has(&found, "`x`", "comment.documentation.code");
    has(&found, "slanted", "comment.documentation.italic");
    let markers: Vec<_> = (found.iter())
        .filter(|(_, h)| *h == "comment.documentation.list")
        .map(|(t, _)| t.trim())
        .collect();
    // A run of lines is one layer, so the items nest however deep.
    assert_eq!(markers, ["-", "-", "-", "1.", "-"], "{found:?}");
    // The `>` and the `**` are the comment's.
    for (text, highlight) in &found {
        if text.contains('>') || text.contains("**") {
            assert_eq!(*highlight, "comment.documentation", "{text:?}");
        }
    }
    for (text, highlight) in &found {
        assert!(!highlight.starts_with("markup"), "{text:?} {highlight}");
    }
}

/// A `/** */` or `/*! */` comment is block Markdown less the ` * ` starting
/// its lines: its items nest, and its prefixes are no bullets.
#[test]
fn block_doc_comments_are_block_markdown_without_their_prefixes() {
    let text = "/** - item\n\
                 *   - nested `x`\n\
                 *     - deeper\n\
                 * > Quoted\n\
                 */\n\
                /*! - inner\n\
                    - undecorated\n */\n\
                fn a() {}\n";
    let found = spans("rust", text);
    let markers: Vec<_> = (found.iter())
        .filter(|(_, h)| *h == "comment.documentation.list")
        .map(|(t, _)| t.trim())
        .collect();
    assert_eq!(markers, ["-", "-", "-", "-", "-"], "{found:?}");
    has(&found, "Quoted\n", "comment.documentation.quote");
    has(&found, "`x`", "comment.documentation.code");
    // The decorative `*`s stay the comment's.
    for (text, highlight) in &found {
        if text.contains('*') {
            assert_eq!(*highlight, "comment.documentation", "{text:?}");
        }
    }
}

/// An undecorated `/** */` comment keeps the stars starting its lines:
/// they're its Markdown's, strong text, emphasis and list items among prose.
#[test]
fn undecorated_block_doc_comments_keep_their_markdown_stars() {
    let text = "/** Intro\n**bold** start\nPlain text\n*slanted* start\n\n* item\n*/\nfn a() {}\n";
    let found = spans("rust", text);
    has(&found, "bold", "comment.documentation.strong");
    has(&found, "slanted", "comment.documentation.italic");
    has(&found, "* ", "comment.documentation.list");
    // One line without the prefix is enough: a decorated comment's lines
    // all have it.
    let text = "/** Intro\n * - item\n**bold**\n */\nfn a() {}\n";
    let found = spans("rust", text);
    has(&found, "bold", "comment.documentation.strong");
}

/// `//!!` is an inner doc line, its text starting `!`: it has Markdown, and
/// joins the `//!` lines around it.
#[test]
fn inner_doc_lines_may_start_with_a_bang() {
    let text = "//! *start\n//!! **bold** end*\n//! - item\n";
    let found = spans("rust", text);
    has(&found, "bold", "comment.documentation.strong");
    // The emphasis spans the lines, so they're one layer.
    has(&found, "start\n", "comment.documentation.italic");
    has(&found, " end", "comment.documentation.italic");
    has(&found, " - ", "comment.documentation.list");
}

/// A fenced example in a doc comment is a code block, not prose: its
/// backticks and stars aren't markup.
#[test]
fn a_doc_comment_fence_is_a_code_block() {
    let text = "/// ```\n/// let a = `b` * *c*;\n/// ```\nfn a() {}\n";
    let found = spans("rust", text);
    let comment = text.find("fn").unwrap();
    assert_eq!(
        found[0],
        (text[..comment].to_string(), "comment.documentation"),
        "{found:?}"
    );
}

/// The markup names around the byte where `word` starts in `text`, of every
/// layer, outermost first, and the span's own highlight there.
fn around(language: &str, text: &str, word: &str) -> (Vec<&'static str>, &'static str) {
    let mut syntax = Syntax::new(
        language_by_name(language).unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    let highlights = loop {
        let highlights = syntax.checked_highlights(0..text.len());
        let missing = syntax.take_missing_languages();
        if missing.is_empty() {
            break highlights;
        }
        for name in missing {
            language_by_name(name);
        }
    };
    let at = text.find(word).unwrap();
    let own = (highlights.spans.iter())
        .find(|span| span.range.contains(&at))
        .map_or("", |span| span.highlight.name());
    let mut covering: Vec<_> = (highlights.markup.iter())
        .filter(|span| span.range.contains(&at))
        .collect();
    assert!(
        covering
            .windows(2)
            .all(|w| w[0].range.start <= w[1].range.start)
    );
    covering.sort_by_key(|span| (span.range.start, std::cmp::Reverse(span.range.end)));
    let mut names: Vec<_> = covering.iter().map(|span| span.highlight.name()).collect();
    // `~~` is a strikethrough of `~` in one of `~`.
    names.dedup();
    (names, own)
}

#[test]
fn nested_markup_is_in_every_capture_around_it() {
    let (names, own) = around("markdown", "Some ~~**gone**~~ now.\n", "gone");
    assert_eq!(own, "markup.strong");
    assert_eq!(names, ["markup.strikethrough", "markup.strong"]);
    let (names, own) = around("markdown", "**a *b* c**\n", "b*");
    assert_eq!(own, "markup.italic");
    assert_eq!(names, ["markup.strong", "markup.italic"]);
    // A heading is the block layer's, its emphasis the inline layer's.
    let (names, own) = around("markdown", "# Title *em*\n", "em");
    assert_eq!(own, "markup.italic");
    assert_eq!(names, ["markup.heading", "markup.italic"]);
    let (names, _) = around("markdown", "> A **bold** quote\n", "bold");
    assert_eq!(names, ["markup.quote", "markup.strong"]);
    // A delimiter is in the markup around it, its own too, but isn't
    // markup itself, so takes no face from it.
    let (names, own) = around("markdown", "~~**gone**~~\n", "**");
    assert_eq!(own, "punctuation.delimiter");
    assert_eq!(names, ["markup.strikethrough", "markup.strong"]);
    // Code isn't markup, nor what's around it.
    let (names, own) = around("markdown", "```rust\nfn a() {}\n```\n", "fn");
    assert_eq!(own, "keyword");
    assert_eq!(names, ["markup.raw"]);
}

#[test]
fn nested_doc_comment_markup_is_in_every_capture_around_it() {
    let (names, own) = around("rust", "/// **a *b* c**\nfn a() {}\n", "b*");
    assert_eq!(own, "comment.documentation.italic");
    assert_eq!(
        names,
        [
            "comment.documentation.strong",
            "comment.documentation.italic"
        ]
    );
    let (names, own) = around("rust", "/// > A ~~*gone*~~ one\nfn a() {}\n", "gone");
    assert_eq!(own, "comment.documentation.italic");
    assert_eq!(
        names,
        [
            "comment.documentation.quote",
            "comment.documentation.strikethrough",
            "comment.documentation.italic"
        ]
    );
}
