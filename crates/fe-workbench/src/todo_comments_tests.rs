//! The keyword scanner and where it looks.

use std::ops::Range;

use fe_buffer::Buffer;
use fe_syntax::{Syntax, language_by_name};

use crate::todo_comments::{Keywords, Kind, TodoHit, comment_ranges, scan};

/// Every byte of `source` as one comment, so only the scanner is tested.
fn hits(source: &str) -> Vec<TodoHit> {
    let buffer = Buffer::new(source);
    let comment = 0..source.len();
    scan(
        buffer.text(),
        std::slice::from_ref(&comment),
        &Keywords::default(),
    )
}

fn tags(source: &str) -> Vec<&str> {
    hits(source)
        .iter()
        .map(|hit| &source[hit.tag_range.clone()])
        .collect()
}

/// `buffer` parsed as `language`, with what it injects compiled.
fn parsed(language: &str, buffer: &Buffer) -> Syntax {
    for name in ["markdown_inline", "html", "rustdoc_inline"] {
        language_by_name(name).unwrap();
    }
    let mut syntax = Syntax::new(language_by_name(language).unwrap(), buffer.current());
    syntax.finish(syntax.parse_job().run());
    syntax
}

/// The text of the comments `comment_ranges` finds in all of `source`.
fn comments(language: &str, source: &str) -> Vec<String> {
    let buffer = Buffer::new(source);
    let syntax = parsed(language, &buffer);
    let text = buffer.text();
    for _ in 0..50 {
        if let Some(ranges) = comment_ranges(Some(&syntax), text, 0..u32::MAX) {
            return ranges
                .into_iter()
                .map(|range: Range<usize>| text.slice_to_string(range))
                .collect();
        }
        for name in syntax.take_missing_languages() {
            language_by_name(name).unwrap();
        }
    }
    panic!("highlighting never finished");
}

#[test]
fn finds_keyword_colon_in_a_comment() {
    let source = "// TODO: write this\n# FIXME: and this";
    let found = hits(source);
    assert_eq!(found.len(), 2);
    assert_eq!(&source[found[0].keyword_range.clone()], "TODO");
    assert_eq!(&source[found[0].tag_range.clone()], "TODO:");
    assert_eq!(found[0].kind, Kind::Todo);
    assert_eq!(found[0].user, None);
    assert_eq!(&source[found[1].tag_range.clone()], "FIXME:");
}

#[test]
fn ignores_a_keyword_in_a_string_or_code() {
    let source = "let s = \"TODO: not this\"; // TODO: this\nfn TODO() {}\n";
    assert_eq!(comments("rust", source), ["// TODO: this"]);
    let buffer = Buffer::new(source);
    let syntax = parsed("rust", &buffer);
    let ranges = loop {
        match comment_ranges(Some(&syntax), buffer.text(), 0..u32::MAX) {
            Some(ranges) => break ranges,
            None => {
                for name in syntax.take_missing_languages() {
                    language_by_name(name).unwrap();
                }
            }
        }
    };
    let found = scan(buffer.text(), &ranges, &Keywords::default());
    assert_eq!(found.len(), 1);
    assert_eq!(&source[found[0].tag_range.clone()], "TODO:");
    assert!(found[0].tag_range.start > source.find("//").unwrap());
}

#[test]
fn requires_the_colon_and_a_word_boundary() {
    assert_eq!(tags("TODO fix this"), Vec::<&str>::new());
    assert_eq!(tags("TODOS: many"), Vec::<&str>::new());
    assert_eq!(tags("MYTODO: x"), Vec::<&str>::new());
    assert_eq!(tags("a_TODO: x"), Vec::<&str>::new());
    assert_eq!(tags("todo: lowercase"), Vec::<&str>::new());
    assert_eq!(tags("TODO(: broken"), Vec::<&str>::new());
    assert_eq!(tags("(TODO): x TODO:"), ["TODO:"]);
    assert_eq!(tags("see: NOTE: x"), ["NOTE:"]);
}

#[test]
fn aliases_map_to_their_kind() {
    for (word, kind) in [
        ("TODO", Kind::Todo),
        ("FIX", Kind::Fix),
        ("FIXME", Kind::Fix),
        ("BUG", Kind::Fix),
        ("FIXIT", Kind::Fix),
        ("ISSUE", Kind::Fix),
        ("HACK", Kind::Hack),
        ("XXX", Kind::Hack),
        ("WARN", Kind::Warn),
        ("WARNING", Kind::Warn),
        ("PERF", Kind::Perf),
        ("OPTIMIZE", Kind::Perf),
        ("PERFORMANCE", Kind::Perf),
        ("NOTE", Kind::Note),
        ("INFO", Kind::Note),
        ("TEST", Kind::Test),
        ("TESTING", Kind::Test),
        ("PASSED", Kind::Test),
        ("FAILED", Kind::Test),
    ] {
        let source = format!("{word}: x");
        let found = hits(&source);
        assert_eq!(found.len(), 1, "{word}");
        assert_eq!(found[0].kind, kind, "{word}");
    }
}

#[test]
fn captures_the_user_in_parentheses() {
    let source = "// TODO(daryl): later";
    let found = hits(source);
    assert_eq!(found.len(), 1);
    let user = found[0].user.clone().unwrap();
    assert_eq!(&source[user], "daryl");
    assert_eq!(&source[found[0].keyword_range.clone()], "TODO");
    assert_eq!(&source[found[0].tag_range.clone()], "TODO(daryl):");
    assert_eq!(tags("TODO(): x"), Vec::<&str>::new());
}

#[test]
fn comment_ranges_come_from_the_highlights_for_rust_and_python() {
    let rust = "let a = 1; // one\n/* two\n   three */\nlet b = \"// no\";\n";
    assert_eq!(comments("rust", rust), ["// one", "/* two\n   three */"]);
    let python = "x = 1  # one\ns = \"# no\"\n";
    assert_eq!(comments("python", python), ["# one"]);

    // Only the rows asked for.
    let buffer = Buffer::new("// a\nlet x = 1;\n// b\n");
    let syntax = parsed("rust", &buffer);
    let text = buffer.text();
    let ranges = comment_ranges(Some(&syntax), text, 2..3).unwrap();
    let found: Vec<_> = ranges
        .into_iter()
        .map(|r| text.slice_to_string(r))
        .collect();
    assert_eq!(found, ["// b"]);
}

#[test]
fn markdown_has_none() {
    let buffer = Buffer::new("# TODO: heading\n\n<!-- TODO: html -->\n");
    let syntax = parsed("markdown", &buffer);
    assert_eq!(
        comment_ranges(Some(&syntax), buffer.text(), 0..u32::MAX),
        Some(Vec::new())
    );
    assert_eq!(
        comment_ranges(None, buffer.text(), 0..u32::MAX),
        Some(Vec::new())
    );
}

#[test]
fn many_malformed_candidates_on_one_line_are_read_in_linear_time() {
    let source = "TODO(".repeat(200_000);
    let started = std::time::Instant::now();
    assert_eq!(hits(&source), []);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(2),
        "took {:?}",
        started.elapsed()
    );
    // Names of any length are names.
    let long = format!("TODO({}): x", "a".repeat(100));
    assert_eq!(hits(&long).len(), 1);
    let source = format!("{}TODO({}): x", "TODO(".repeat(50_000), "b".repeat(100_000));
    assert_eq!(hits(&source).len(), 1);
    assert_eq!(hits("TODO(a b): x").len(), 1);
}
