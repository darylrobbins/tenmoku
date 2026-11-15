//! XML in C# `///` doc comments: tags, attribute names and values colour,
//! while the `///` and the text between tags keep the comment colour.

#![cfg(all(feature = "csharp", feature = "xml"))]

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

fn spans(lang: &str, text: &str) -> Vec<(String, &'static str)> {
    let mut syntax = Syntax::new(language_by_name(lang).unwrap(), Buffer::new(text).current());
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

fn csharp(text: &str) -> Vec<(String, &'static str)> {
    spans("csharp", text)
}

fn has(found: &[(String, &str)], text: &str, highlight: &str) -> usize {
    found
        .iter()
        .filter(|(t, h)| t == text && *h == highlight)
        .count()
}

#[test]
fn xml_doc_tags_and_attributes_are_highlighted() {
    let text = "class C\n{\n    /// <param name=\"count\">How many.</param>\n    void F(int count) { }\n}\n";
    let found = csharp(text);
    assert_eq!(has(&found, "param", "tag"), 2, "{found:?}");
    assert_eq!(has(&found, "name", "attribute"), 1, "{found:?}");
    // The value's text; tree-sitter-xml paints its quotes as delimiters.
    assert_eq!(has(&found, "count", "string"), 1, "{found:?}");
    assert!(
        found
            .iter()
            .any(|(t, h)| t.contains("How many.") && *h == "comment"),
        "{found:?}"
    );
    assert!(
        !found
            .iter()
            .any(|(t, h)| t.contains("How") && *h != "comment"),
        "{found:?}"
    );
}

#[test]
fn multi_line_summary_tags_are_highlighted() {
    let text =
        "class C\n{\n    /// <summary>\n    /// Text\n    /// </summary>\n    void F() { }\n}\n";
    let found = csharp(text);
    assert_eq!(has(&found, "summary", "tag"), 2, "{found:?}");
    assert!(
        found
            .iter()
            .any(|(t, h)| t.contains("Text") && *h == "comment"),
        "{found:?}"
    );
}

#[test]
fn doc_comment_marker_stays_comment() {
    let text = "/// <summary>\n/// Counts <c>x</c>.\n/// </summary>\n/// <see cref=\"Foo\"/>\nclass C { }\n";
    let found = csharp(text);
    let markers: Vec<_> = found.iter().filter(|(t, _)| t.contains("///")).collect();
    assert!(!markers.is_empty(), "{found:?}");
    for (marker, highlight) in markers {
        assert_eq!(*highlight, "comment", "{marker:?} in {found:?}");
        assert!(marker.starts_with("///"), "{marker:?} in {found:?}");
    }
    // Every line's `///` is in one comment span.
    let mut syntax = Syntax::new(
        language_by_name("csharp").unwrap(),
        Buffer::new(text).current(),
    );
    syntax.finish(syntax.parse_job().run());
    let highlights = syntax.highlights(0..text.len());
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        if line.starts_with("///") {
            assert!(
                highlights.iter().any(|span| span.range.start <= start
                    && span.range.end >= start + 3
                    && span.highlight.name() == "comment"),
                "{line:?}: {found:?}"
            );
        }
        start += line.len();
    }
}

#[test]
fn plain_comments_are_not_injected() {
    for comment in ["// <b>x</b>", "/* <b>x</b> */", "//// <b>x</b>"] {
        let text = format!("{comment}\nclass C {{ }}\n");
        let found = csharp(&text);
        assert!(
            found.contains(&(comment.to_string(), "comment")),
            "{comment}: {found:?}"
        );
        assert_eq!(has(&found, "b", "tag"), 0, "{comment}: {found:?}");
    }
    // A plain `//` line next to a doc block doesn't join it.
    let text = "// <b>\n/// <summary>x</summary>\n// </b>\nclass C { }\n";
    let found = csharp(text);
    assert_eq!(has(&found, "summary", "tag"), 2, "{found:?}");
    assert_eq!(has(&found, "b", "tag"), 0, "{found:?}");
}

#[test]
fn self_closing_see_cref() {
    let text = "/// <see cref=\"Foo\"/>\nclass C { }\n";
    let found = csharp(text);
    assert_eq!(has(&found, "see", "tag"), 1, "{found:?}");
    assert_eq!(has(&found, "cref", "attribute"), 1, "{found:?}");
    assert_eq!(has(&found, "Foo", "string"), 1, "{found:?}");
}
