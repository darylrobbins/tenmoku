//! Each language's parse limit follows from the tree budget, and the limit is
//! exact. Nothing here parses: a debug-build parse of megabytes is slow, and
//! tree memory can't be counted deterministically (tree-sitter allocates
//! through C malloc, and tests run in parallel).

use fe_buffer::{Buffer, Origin};
use fe_syntax::{Syntax, TREE_BUDGET_BYTES, language_by_name, language_names};

/// Tree bytes per source byte, from PERF.md, "Tree memory".
fn ratio(name: &str) -> usize {
    match name {
        "rust" => 40,
        "json" => 30,
        "markdown" => 85,
        _ => 40,
    }
}

#[test]
fn limits_follow_the_tree_budget() {
    assert_eq!(TREE_BUDGET_BYTES, 300 * 1024 * 1024);
    for name in language_names() {
        let language = language_by_name(name).expect("every listed language loads");
        assert_eq!(
            language.parse_limit(),
            TREE_BUDGET_BYTES / ratio(name),
            "{name}"
        );
    }
}

#[cfg(feature = "rust")]
#[test]
fn rust_limit() {
    assert_eq!(language_by_name("rust").unwrap().parse_limit(), 7_864_320);
}

#[cfg(feature = "json")]
#[test]
fn json_limit() {
    assert_eq!(language_by_name("json").unwrap().parse_limit(), 10_485_760);
}

#[cfg(feature = "markdown")]
#[test]
fn markdown_limit() {
    assert_eq!(
        language_by_name("markdown").unwrap().parse_limit(),
        3_700_856
    );
}

/// Exactly `len` bytes of a cheap repeated line.
fn text_of(len: usize) -> String {
    let mut text = "a\n".repeat(len / 2);
    text.push_str(&"a".repeat(len % 2));
    text
}

fn check(name: &str) {
    let language = language_by_name(name).expect("language is compiled in");
    let limit = language.parse_limit();
    let mut buffer = Buffer::new(&text_of(limit));
    let mut syntax = Syntax::new(language, &buffer.snapshot());
    assert_eq!(buffer.snapshot().len(), limit);

    assert!(syntax.needs_parse(), "{name}: at the limit");
    assert!(
        syntax.window_job(0..limit).is_none(),
        "{name}: at the limit"
    );

    // One more byte flips it.
    buffer.edit([(0..0, "a")], Origin::User);
    syntax.edit(&buffer.snapshot());
    assert!(!syntax.needs_parse(), "{name}: one over");
    assert!(syntax.window_job(0..limit).is_some(), "{name}: one over");
}

#[test]
fn one_byte_over_the_limit_is_not_parsed() {
    #[cfg(feature = "rust")]
    check("rust");
    #[cfg(feature = "json")]
    check("json");
    #[cfg(feature = "markdown")]
    check("markdown");
}
