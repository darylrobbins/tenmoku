use fe_buffer::{Buffer, Origin};

use super::*;
use crate::tests::parse;

fn highlights(
    buffer: &Buffer,
    provider: &OccurrenceHighlights,
    range: Range<usize>,
) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    provider.decorations(&buffer.snapshot(), range, &mut out);
    for d in &out {
        assert_eq!(d.kind, DecorationKind::Highlight);
        assert_eq!((d.tint, d.priority), (Tint::Muted, -50));
    }
    out.into_iter().map(|d| d.range).collect()
}

fn targeted(text: &str, target: Range<usize>) -> (Buffer, OccurrenceHighlights) {
    let buffer = Buffer::new(text);
    let mut provider = OccurrenceHighlights::new();
    assert!(provider.set_target(&buffer.snapshot(), Some(target)));
    (buffer, provider)
}

#[test]
fn target_of_a_caret_or_a_whole_word() {
    assert_eq!(parse("fo|o bar").occurrence_target(), Some(0..3));
    assert_eq!(parse("foo| bar").occurrence_target(), Some(0..3));
    assert_eq!(parse("|foo bar").occurrence_target(), Some(0..3));
    assert_eq!(parse("a_1 |foo").occurrence_target(), Some(4..7));
    assert_eq!(parse("[foo] bar").occurrence_target(), Some(0..3));
    assert_eq!(parse("[foo]").occurrence_target(), Some(0..3));
}

#[test]
fn no_target_for_anything_else() {
    assert_eq!(parse("a +|+ b").occurrence_target(), None);
    assert_eq!(parse("a  |  b").occurrence_target(), None);
    assert_eq!(parse("|").occurrence_target(), None);
    assert_eq!(parse("fo|o ba|r").occurrence_target(), None);
    assert_eq!(parse("[foo\nbar]").occurrence_target(), None);
    assert_eq!(parse("f[oo] bar").occurrence_target(), None);
    assert_eq!(parse("[foo ba]r").occurrence_target(), None);
    assert_eq!(parse("[foo ]bar").occurrence_target(), None);
}

#[test]
fn whole_case_sensitive_words_including_the_targets_own() {
    let text = "foo foobar _foo Foo foo\nfoo";
    let (buffer, provider) = targeted(text, 0..3);
    assert_eq!(
        highlights(&buffer, &provider, 0..text.len()),
        [0..3, 20..23, 24..27]
    );
    // Only those overlapping the range; one straddling its end counts.
    assert_eq!(highlights(&buffer, &provider, 21..22), vec![20..23]);
    assert_eq!(highlights(&buffer, &provider, 1..21), [0..3, 20..23]);
    assert_eq!(highlights(&buffer, &provider, 3..20), []);
}

#[test]
fn set_target_reports_changes_and_edits_follow() {
    let mut buffer = Buffer::new("foo x foo");
    let mut provider = OccurrenceHighlights::new();
    assert!(!provider.set_target(&buffer.snapshot(), None));
    assert!(provider.set_target(&buffer.snapshot(), Some(0..3)));
    assert!(!provider.set_target(&buffer.snapshot(), Some(0..3)));
    assert!(provider.set_target(&buffer.snapshot(), Some(6..9)));
    assert!(provider.set_target(&buffer.snapshot(), None));
    assert!(provider.set_target(&buffer.snapshot(), Some(0..3)));

    // Elsewhere: the matches follow the text.
    buffer.edit([(0..0, "yy ")], Origin::User).unwrap();
    assert_eq!(highlights(&buffer, &provider, 0..12), [3..6, 9..12]);
    // Inside the word: nothing.
    buffer.edit([(4..4, "o")], Origin::User).unwrap();
    assert_eq!(highlights(&buffer, &provider, 0..13), []);
}

#[test]
fn bounded_by_count_and_by_range() {
    let text = "ab ".repeat(5_000);
    let (buffer, provider) = targeted(&text, 0..2);
    assert_eq!(highlights(&buffer, &provider, 0..text.len()).len(), 1_000);

    let text = format!("foo{}foo", " ".repeat(50));
    let (buffer, provider) = targeted(&text, 0..3);
    assert_eq!(highlights(&buffer, &provider, 10..20), []);
}
