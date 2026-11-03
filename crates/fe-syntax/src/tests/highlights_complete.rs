//! [`Syntax::checked_highlights`] says whether its spans are all there are, so
//! a caller can tell text no capture covers from text no query reached.

use std::ops::Range;

use fe_buffer::{Buffer, Origin};

use super::parsed;
use crate::{
    Syntax, language_by_name, loaded_language,
    syntax::{DEADLINE_AFTER, DEADLINE_CHECKS, FAULT},
};

/// About 2,000 rows of TOML and the range of 60 rows in its middle.
fn pipeline() -> (Buffer, Range<usize>) {
    let block = "[[pipeline.stages]]\nname = \"test\"\n";
    let start = 500 * block.len();
    (
        Buffer::new(&block.repeat(1000)),
        start..start + 30 * block.len(),
    )
}

fn toml(buffer: &Buffer) -> Syntax {
    parsed(language_by_name("toml").unwrap(), buffer)
}

/// Runs `f` with every deadline passing after `after` checks, and counts them.
fn with_deadline_after<T>(after: usize, f: impl FnOnce() -> T) -> (T, usize) {
    DEADLINE_CHECKS.set(0);
    DEADLINE_AFTER.set(after);
    let result = f();
    DEADLINE_AFTER.set(usize::MAX);
    (result, DEADLINE_CHECKS.get())
}

#[test]
fn a_finished_parse_is_complete_and_so_is_its_cached_result() {
    let (buffer, range) = pipeline();
    let syntax = toml(&buffer);
    let first = syntax.checked_highlights(range.clone());
    assert!(first.complete);
    assert!(!first.spans.is_empty());
    assert_eq!(first.spans, syntax.highlights(range.clone()));
    let (again, checks) = with_deadline_after(usize::MAX, || syntax.checked_highlights(range));
    assert_eq!((again, checks), (first, 0));
}

#[test]
fn a_range_with_nothing_to_highlight_is_complete() {
    let buffer = Buffer::new("\n\n\n");
    let syntax = parsed(language_by_name("rust").unwrap(), &buffer);
    let found = syntax.checked_highlights(0..3);
    assert!(found.complete);
    assert!(found.spans.is_empty());
}

#[test]
fn a_query_cut_short_is_incomplete_up_to_the_cap_and_after_it() {
    let (buffer, range) = pipeline();
    let syntax = toml(&buffer);
    let (_, checks) = with_deadline_after(usize::MAX, || syntax.highlights(range.clone()));
    let after = checks / 2;
    assert!(after >= 2, "only {checks} deadline checks");

    let syntax = toml(&buffer);
    for budget_ms in [2, 4, 8, 16] {
        let (cut, _) = with_deadline_after(after, || syntax.checked_highlights(range.clone()));
        assert!(!cut.complete, "{budget_ms} ms");
        assert!(!cut.spans.is_empty(), "{budget_ms} ms");
    }
    // At the cap it's kept and not queried again, but it's no more complete.
    let (cached, checks) = with_deadline_after(after, || syntax.checked_highlights(range.clone()));
    assert_eq!(checks, 0);
    assert!(!cached.complete);
    assert!(!syntax.checked_highlights(range).complete);
}

#[test]
fn an_injection_out_of_time_is_incomplete_and_comes_right() {
    let source = "Some `code` here\n\nMore `code` here\n";
    let buffer = Buffer::new(source);
    language_by_name("markdown_inline").unwrap();
    let markdown = language_by_name("markdown").unwrap();
    let syntax = parsed(markdown, &buffer);
    let (full, checks) =
        with_deadline_after(usize::MAX, || syntax.checked_highlights(0..source.len()));
    assert!(full.complete);

    // At every step the deadline can pass, it's either all there or says not.
    for after in 0..checks {
        let syntax = parsed(markdown, &buffer);
        let (cut, _) = with_deadline_after(after, || syntax.checked_highlights(0..source.len()));
        assert!(
            !cut.complete || cut.spans == full.spans,
            "complete after {after} checks of {checks}: {:?}",
            cut.spans
        );
    }
    let syntax = parsed(markdown, &buffer);
    let (cut, _) = with_deadline_after(0, || syntax.checked_highlights(0..source.len()));
    assert!(!cut.complete);
    assert_eq!(syntax.checked_highlights(0..source.len()), full);
}

/// A result with an injection left out for good is kept, as querying again
/// wouldn't find it, without its passing for complete.
fn is_incomplete_but_cached(syntax: &Syntax, range: Range<usize>) {
    let first = syntax.highlights_compiling(range.clone());
    let (again, checks) = with_deadline_after(usize::MAX, || syntax.checked_highlights(range));
    assert!(!again.complete);
    assert_eq!(again.spans, first);
    assert_eq!(checks, 0, "queried again");
    assert_eq!(syntax.take_missing_languages(), Vec::<&str>::new());
}

#[test]
fn injections_past_the_limit_are_incomplete() {
    // A paragraph is an injection of the inline grammar each.
    let source = "Some `code` here\n\n".repeat(5000);
    let buffer = Buffer::new(&source);
    let syntax = parsed(language_by_name("markdown").unwrap(), &buffer);
    is_incomplete_but_cached(&syntax, 0..source.len());
    // A screenful of it has them all.
    assert!(syntax.highlights_compiling(0..180).len() > 1);
    assert!(syntax.checked_highlights(0..180).complete);
}

#[test]
fn an_injection_with_too_many_ranges_is_incomplete() {
    // A block quote's paragraph is one injection, with a range per line between
    // the markers.
    let source = "> a\n".repeat(20_000);
    let buffer = Buffer::new(&source);
    let syntax = parsed(language_by_name("markdown").unwrap(), &buffer);
    is_incomplete_but_cached(&syntax, 0..source.len());
}

#[test]
fn injections_nested_past_the_limit_are_incomplete() {
    // Markdown, then HTML in a fence, then its script, then a tagged template's
    // HTML, which could inject in turn.
    let source = "```html\n<script>const a = html`<p>b</p>`;</script>\n```\n";
    let buffer = Buffer::new(source);
    let syntax = parsed(language_by_name("markdown").unwrap(), &buffer);
    is_incomplete_but_cached(&syntax, 0..source.len());
    // Without the template, it nests no further than it's allowed.
    let source = "```html\n<script>const a = 1;</script>\n```\n";
    let buffer = Buffer::new(source);
    let syntax = parsed(language_by_name("markdown").unwrap(), &buffer);
    syntax.highlights_compiling(0..source.len());
    assert!(syntax.checked_highlights(0..source.len()).complete);
}

#[test]
fn a_missing_injected_language_is_incomplete_until_compiled() {
    // A test run by nextest has its own process, so nothing has compiled this.
    if loaded_language("markdown_inline").is_some() {
        return;
    }
    let buffer = Buffer::new("Some `code` here\n");
    let syntax = parsed(language_by_name("markdown").unwrap(), &buffer);
    assert!(!syntax.checked_highlights(0..16).complete);
    assert!(syntax.take_missing_languages().contains(&"markdown_inline"));
    language_by_name("markdown_inline").unwrap();
    assert!(syntax.checked_highlights(0..16).complete);
}

#[test]
fn an_unknown_injected_language_is_no_reason_to_wait() {
    let source = "```nosuchlanguage\ncode\n```\n";
    let buffer = Buffer::new(source);
    let syntax = parsed(language_by_name("markdown").unwrap(), &buffer);
    assert!(syntax.highlights_compiling(0..source.len()).len() > 1);
    assert!(syntax.checked_highlights(0..source.len()).complete);
}

#[test]
fn no_parse_a_stale_parse_or_a_window_is_incomplete() {
    let rust = language_by_name("rust").unwrap();
    let mut buffer = Buffer::new("// a remark\n");
    let mut syntax = Syntax::new(rust, buffer.current());
    assert!(!syntax.checked_highlights(0..5).complete);

    // Complete once parsed, and not once edited, even for a result it had.
    syntax.finish(syntax.parse_job().run());
    assert!(syntax.checked_highlights(0..5).complete);
    buffer.edit([(11..11, " more")], Origin::User);
    syntax.edit(buffer.current());
    let stale = syntax.checked_highlights(0..5);
    assert!(!stale.complete);
    assert!(!stale.spans.is_empty());
    syntax.finish(syntax.parse_job().run());
    assert!(syntax.checked_highlights(0..5).complete);

    // A job for an older version leaves it stale.
    let older = syntax.parse_job();
    let len = buffer.text().len();
    buffer.edit([(len..len, "\n")], Origin::User);
    syntax.edit(buffer.current());
    syntax.finish(older.run());
    assert!(!syntax.checked_highlights(0..5).complete);

    // Too big for a tree: the window highlights, but only approximately.
    let big = "// remark\n".repeat(rust.parse_limit() / 10 + 1);
    let buffer = Buffer::new(&big);
    let mut syntax = Syntax::new(rust, buffer.current());
    syntax.finish(syntax.window_job(0..10).unwrap().run());
    let window = syntax.checked_highlights(0..10);
    assert!(!window.complete);
    assert!(!window.spans.is_empty());
}

#[test]
fn a_panic_is_incomplete() {
    let buffer = Buffer::new("// a remark\n");
    let syntax = parsed(language_by_name("rust").unwrap(), &buffer);
    FAULT.set(Some("query"));
    let found = syntax.checked_highlights(0..5);
    FAULT.set(None);
    assert!(!found.complete);
    assert!(found.spans.is_empty());
    assert!(syntax.failed());
    assert!(!syntax.checked_highlights(0..5).complete);
}

#[test]
fn email_autolinks_are_highlighted_like_url_ones() {
    let source = "Mail <someone@example.com> or <https://example.com>\n";
    language_by_name("markdown_inline").unwrap();
    let buffer = Buffer::new(source);
    let syntax = parsed(language_by_name("markdown").unwrap(), &buffer);
    let spans: Vec<_> = syntax
        .highlights_compiling(0..source.len())
        .into_iter()
        .map(|span| (source[span.range].to_owned(), span.highlight.name()))
        .collect();
    assert_eq!(
        spans,
        [
            ("<someone@example.com>".to_owned(), "markup.link.url"),
            ("<https://example.com>".to_owned(), "markup.link.url"),
        ]
    );
}
