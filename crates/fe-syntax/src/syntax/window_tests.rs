use std::ops::Range;

use fe_buffer::{Buffer, Origin};

use super::{FAULT, MAX_PARSE_BYTES, WINDOW_BYTES};
use crate::{HighlightSpan, Syntax, language_by_name};

/// One line of Rust, with multibyte chars so windows can end mid-char.
const ITEM: &str = "fn é(x: u32) -> &'static str { let s = \"a\\n\"; s } // ü\n";

/// `item` repeated until the text is over [`MAX_PARSE_BYTES`].
fn over_limit(item: &str) -> String {
    item.repeat(MAX_PARSE_BYTES / item.len() + 1)
}

fn syntax(buffer: &Buffer) -> Syntax {
    Syntax::new(language_by_name("rust").unwrap(), buffer.current())
}

/// Parses the window around `visible` and installs it.
fn show(syntax: &mut Syntax, visible: Range<usize>) -> Range<usize> {
    syntax.finish(syntax.window_job(visible).unwrap().run());
    syntax.window().unwrap()
}

/// `spans` moved by `delta` bytes.
fn shifted(spans: Vec<HighlightSpan>, delta: isize) -> Vec<HighlightSpan> {
    spans
        .into_iter()
        .map(|span| HighlightSpan {
            range: span.range.start.checked_add_signed(delta).unwrap()
                ..span.range.end.checked_add_signed(delta).unwrap(),
            ..span
        })
        .collect()
}

#[test]
fn a_window_highlights_like_a_parse_of_its_text() {
    let text = over_limit(ITEM);
    let buffer = Buffer::new(&text);
    let mut syntax = syntax(&buffer);
    assert!(syntax.tree().is_none() && !syntax.needs_parse());
    assert!(
        syntax.highlights(0..100).is_empty(),
        "plain until a window lands"
    );

    let window = show(&mut syntax, 4_000_000..4_000_100);
    assert!(window.contains(&4_000_000) && window.contains(&4_000_100));
    assert_eq!(window.start % ITEM.len(), 0, "{window:?} starts at an item");

    let mut alone = syntax_of(&text[window.clone()]);
    alone.finish(alone.parse_job().run());
    let expected = shifted(alone.highlights(0..window.len()), window.start as isize);
    assert!(expected.len() > 1000);
    assert_eq!(syntax.highlights(window.clone()), expected);
    // Only the window highlights.
    let around = window.start - 1000..window.end + 1000;
    assert_eq!(syntax.highlights(around), expected);
    assert!(syntax.highlights(0..1000).is_empty());
}

fn syntax_of(text: &str) -> Syntax {
    syntax(&Buffer::new(text))
}

#[test]
fn window_parses_are_bounded() {
    let small = Buffer::new(&ITEM.repeat(1000));
    assert!(syntax(&small).window_job(0..100).is_none());

    let check = |syntax: &Syntax, visible: Range<usize>| -> Option<Range<usize>> {
        let window = syntax.window_job(visible)?.window.unwrap();
        let text = syntax.snapshot.text();
        assert!(
            !window.is_empty() && window.len() <= WINDOW_BYTES,
            "{window:?}"
        );
        assert_eq!(text.offset_to_point(window.start).column, 0, "{window:?}");
        assert!(text.is_char_boundary(window.end), "{window:?}");
        Some(window)
    };
    let text = over_limit(ITEM);
    let len = text.len();
    let syntax = syntax(&Buffer::new(&text));
    for offset in (0..len).step_by(len / 97).chain([len]) {
        let window = check(&syntax, offset..offset + 10).unwrap();
        assert!(window.contains(&offset) || offset >= len - ITEM.len());
    }
    check(&syntax, 0..len).unwrap();

    // A single 9 MB line: only its start has a row start to begin at.
    let line = "é".repeat(9 << 19);
    let mut syntax = syntax_of(&line);
    let window = check(&syntax, 0..100).unwrap();
    assert!(window.start == 0 && window.len() > WINDOW_BYTES - 2);
    assert!(check(&syntax, 3 << 20..(3 << 20) + 100).is_none());
    let window = show(&mut syntax, 0..100);
    let root = syntax.window.as_ref().unwrap().1.root_node();
    assert!(root.end_byte() <= window.end, "{root:?} in {window:?}");
}

#[test]
fn the_window_follows_edits_until_replaced() {
    let mut text = over_limit(ITEM);
    let mut buffer = Buffer::new(&text);
    let mut syntax = syntax(&buffer);
    let window = show(&mut syntax, 4_000_000..4_000_100);
    let before = syntax.highlights(window.clone());

    // An edit above the window moves it and its highlights.
    buffer.edit([(10..10, "\n// x\n")], Origin::User);
    syntax.edit(buffer.current());
    let moved = window.start + 6..window.end + 6;
    assert_eq!(syntax.window(), Some(moved.clone()));
    let tree = &syntax.window.as_ref().unwrap().1;
    let included = &tree.included_ranges()[0];
    assert_eq!(included.start_byte..included.end_byte, moved);
    assert_eq!(syntax.highlights(moved.clone()), shifted(before.clone(), 6));

    // One inside it keeps the highlights outside the edited text.
    let at = moved.start + 20 * ITEM.len();
    buffer.edit([(at..at, "\"")], Origin::User);
    syntax.edit(buffer.current());
    assert_eq!(syntax.window(), Some(moved.start..moved.end + 1));
    let after = syntax.highlights(moved.start..moved.end + 1);
    let (above, below): (Vec<_>, Vec<_>) = shifted(before, 6)
        .into_iter()
        .filter(|span| span.range.end < at || span.range.start > at)
        .partition(|span| span.range.end < at);
    for span in above.iter().chain(&shifted(below, 1)) {
        assert!(after.contains(span), "{span:?} is kept");
    }

    // Results land out of order. One for an older version than the
    // snapshot is edited forward.
    let first = syntax.window_job(1_000_000..1_000_100).unwrap();
    let again = syntax.window_job(1_000_000..1_000_100).unwrap();
    buffer.edit([(0..0, "\n")], Origin::User);
    syntax.edit(buffer.current());
    text.insert_str(10, "\n// x\n");
    text.insert(at, '"');
    text.insert(0, '\n');
    syntax.finish(first.run());
    let early = syntax.window().unwrap();
    assert!(early.contains(&1_000_001));
    let mut alone = syntax_of(&text[early.clone()]);
    alone.finish(alone.parse_job().run());
    let expected = shifted(alone.highlights(0..early.len()), early.start as isize);
    assert_eq!(syntax.highlights(early), expected);
    // A newer one replaces it, and the older one then doesn't.
    let latest = show(&mut syntax, 2_000_000..2_000_100);
    syntax.finish(again.run());
    assert_eq!(syntax.window(), Some(latest));
    // Another of the same version does: the view scrolled.
    let scrolled = show(&mut syntax, 3_000_000..3_000_100);
    assert!(scrolled.contains(&3_000_000));
}

#[test]
fn a_panicking_window_parse_fails_the_buffer() {
    let mut syntax = syntax(&Buffer::new(&over_limit(ITEM)));
    show(&mut syntax, 0..100);
    let job = syntax.window_job(1_000_000..1_000_100).unwrap();
    FAULT.set(Some("read"));
    syntax.finish(job.run());
    assert!(syntax.failed() && syntax.window().is_none());
    assert!(syntax.window_job(0..100).is_none());
    assert!(syntax.highlights(0..100).is_empty());
}
