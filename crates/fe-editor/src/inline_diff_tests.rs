#![allow(clippy::single_range_in_vec_init)]

use std::ops::Range;
use std::time::{Duration, Instant};

use super::{MAX_INLINE_BYTES, MAX_PAIRED_ROWS};
use crate::text_diff::{HunkKind, Side, TextDiff};

/// The changed text of each of `rows` on `side`.
fn changed<'a>(left: &'a str, right: &'a str, side: Side, rows: Range<u32>) -> Vec<Vec<String>> {
    let diff = TextDiff::new(left, right);
    let inline = diff.inline(left, right);
    let text = match side {
        Side::Left => left,
        Side::Right => right,
    };
    let lines: Vec<&str> = text.lines().collect();
    rows.map(|row| {
        (inline.changed_spans(side, row).iter())
            .map(|span| lines[row as usize][span.clone()].to_string())
            .collect()
    })
    .collect()
}

#[test]
fn one_word() {
    let (left, right) = ("let x = foo(1);", "let x = bar(1);");
    let diff = TextDiff::new(left, right);
    let inline = diff.inline(left, right);
    assert_eq!(inline.changed_spans(Side::Left, 0), [8..11]);
    assert_eq!(inline.changed_spans(Side::Right, 0), [8..11]);
    assert_eq!(&left[8..11], "foo");
    assert_eq!(&right[8..11], "bar");

    // Byte ranges within each row, not the text, whatever the line endings.
    let (left, right) = ("a\r\nlet x = foo(1);\r\nz", "a\nlet x = bar(10);\nz");
    let diff = TextDiff::new(left, right);
    let inline = diff.inline(left, right);
    assert_eq!(inline.changed_spans(Side::Left, 1), [8..11, 12..13]);
    assert_eq!(inline.changed_spans(Side::Right, 1), [8..11, 12..14]);
    assert!(inline.changed_spans(Side::Left, 0).is_empty());
    assert!(inline.changed_spans(Side::Right, 2).is_empty());
}

#[test]
fn pure_rows() {
    let left = "a\nb\nc\nd\n";
    let right = "a\nnew\nb\nd\n";
    let diff = TextDiff::new(left, right);
    let kinds: Vec<_> = diff.hunks().iter().map(|hunk| hunk.kind()).collect();
    assert_eq!(kinds, [HunkKind::Added, HunkKind::Removed]);
    let inline = diff.inline(left, right);
    for row in 0..5 {
        assert!(
            inline.changed_spans(Side::Left, row).is_empty(),
            "left {row}"
        );
        assert!(
            inline.changed_spans(Side::Right, row).is_empty(),
            "right {row}"
        );
    }

    // Unchanged rows around a modified one.
    let (left, right) = ("same\nold one\nsame", "same\nnew one\nsame");
    assert_eq!(
        changed(left, right, Side::Left, 0..3),
        [vec![], vec!["old"], vec![]]
    );
    assert_eq!(
        changed(left, right, Side::Right, 0..3),
        [vec![], vec!["new"], vec![]]
    );
}

#[test]
fn pairing() {
    let left = "top\nfn a(x: u8)\nlet y = 1;\nend\n";
    let right = "top\nfn b(x: u8)\nlet y = 2;\nextra row\nend\n";
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks().len(), 1);
    assert_eq!(diff.hunks()[0].left, 1..3);
    assert_eq!(diff.hunks()[0].right, 1..4);

    assert_eq!(
        changed(left, right, Side::Left, 1..3),
        [vec!["a"], vec!["1"]]
    );
    assert_eq!(
        changed(left, right, Side::Right, 1..4),
        [vec!["b"], vec!["2"], vec![]]
    );
}

#[test]
fn bounds() {
    let long = |word: &str| format!("{} {word}", "x".repeat(MAX_INLINE_BYTES));
    let (left, right) = (long("old"), long("new"));
    let diff = TextDiff::new(&left, &right);
    assert_eq!(diff.hunks()[0].kind(), HunkKind::Modified);
    let inline = diff.inline(&left, &right);
    assert!(inline.changed_spans(Side::Left, 0).is_empty());
    assert!(inline.changed_spans(Side::Right, 0).is_empty());

    // Right at the cap still diffs.
    let at = |word: &str| format!("{} {word}", "x".repeat(MAX_INLINE_BYTES - 4));
    let (left, right) = (at("old"), at("new"));
    let diff = TextDiff::new(&left, &right);
    let inline = diff.inline(&left, &right);
    assert_eq!(
        inline.changed_spans(Side::Left, 0),
        [MAX_INLINE_BYTES - 3..MAX_INLINE_BYTES]
    );

    // A hunk over the row cap: every row changes, so it is one hunk.
    let rows = |tag: &str| {
        (0..=MAX_PAIRED_ROWS)
            .map(|i| format!("row {i} {tag}\n"))
            .collect::<String>()
    };
    let (left, right) = (rows("old"), rows("new"));
    let diff = TextDiff::new(&left, &right);
    assert_eq!(diff.hunks().len(), 1);
    let inline = diff.inline(&left, &right);
    let started = Instant::now();
    for row in 0..=MAX_PAIRED_ROWS as u32 {
        assert!(inline.changed_spans(Side::Left, row).is_empty());
        assert!(inline.changed_spans(Side::Right, row).is_empty());
    }
    // One under it diffs every pair.
    let (left, right) = (
        &left[left.find('\n').unwrap() + 1..],
        &right[right.find('\n').unwrap() + 1..],
    );
    let diff = TextDiff::new(left, right);
    let inline = diff.inline(left, right);
    for row in 0..MAX_PAIRED_ROWS as u32 {
        assert_eq!(inline.changed_spans(Side::Left, row).len(), 1, "row {row}");
    }
    // Rows at the byte cap, every token changed, in every pair of a hunk
    // at the row cap.
    let noisy = |seed: usize| {
        (0..MAX_PAIRED_ROWS)
            .map(|row| {
                let mut line = String::new();
                let mut i = row * 7 + seed;
                while line.len() < MAX_INLINE_BYTES - 8 {
                    line.push_str(&format!("{} ", i % 97));
                    i = (i * 31 + 17) % 10_007;
                }
                line + "\n"
            })
            .collect::<String>()
    };
    let (left, right) = (noisy(1), noisy(2));
    let diff = TextDiff::new(&left, &right);
    let inline = diff.inline(&left, &right);
    for row in 0..MAX_PAIRED_ROWS as u32 {
        inline.changed_spans(Side::Right, row);
    }
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_millis(500), "took {elapsed:?}");

    graphemes_are_never_split();
}

fn graphemes_are_never_split() {
    // `é` as e and a combining acute stays one grapheme in the word; the
    // flag and the family emoji are one grapheme each.
    let left = "caf\u{65}\u{301} 🇨🇦 👨‍👩‍👧 x";
    let right = "cafe 🇫🇷 👨‍👩‍👦 x";
    let diff = TextDiff::new(left, right);
    let inline = diff.inline(left, right);
    for (side, text) in [(Side::Left, left), (Side::Right, right)] {
        let spans = inline.changed_spans(side, 0);
        assert!(!spans.is_empty());
        for span in spans {
            assert!(text.is_char_boundary(span.start) && text.is_char_boundary(span.end));
        }
    }
    let texts = |side, text: &'static str| -> Vec<&str> {
        (inline.changed_spans(side, 0).iter())
            .map(|span| &text[span.clone()])
            .collect()
    };
    assert_eq!(texts(Side::Left, left), ["caf\u{65}\u{301}", "🇨🇦", "👨‍👩‍👧"]);
    assert_eq!(texts(Side::Right, right), ["cafe", "🇫🇷", "👨‍👩‍👦"]);
}
