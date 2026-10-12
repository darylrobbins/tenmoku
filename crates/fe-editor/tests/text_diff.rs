//! `TextDiff`: hunks of two texts, side-by-side alignment, inline removed
//! rows, moving between hunks, and applying chosen hunks or rows.

use fe_editor::text_diff::MAX_DIFF_BYTES;
use fe_editor::{Hunk, HunkKind, Side, TextDiff};

/// Rows a..g with one added row (X after a), one removed (c) and one
/// modified (f to F).
const LEFT: &str = "a\nb\nc\nd\ne\nf\ng\n";
const RIGHT: &str = "a\nX\nb\nd\ne\nF\ng\n";

fn hunk(left: std::ops::Range<u32>, right: std::ops::Range<u32>) -> Hunk {
    Hunk { left, right }
}

fn row_count(text: &str) -> u32 {
    text.split('\n').count() as u32
}

#[test]
fn hunks_and_kinds() {
    let diff = TextDiff::new(LEFT, RIGHT);
    assert!(!diff.too_large());
    assert_eq!(
        diff.hunks(),
        [hunk(1..1, 1..2), hunk(2..3, 3..3), hunk(5..6, 5..6)]
    );
    let kinds: Vec<_> = diff.hunks().iter().map(Hunk::kind).collect();
    assert_eq!(
        kinds,
        [HunkKind::Added, HunkKind::Removed, HunkKind::Modified]
    );

    assert!(TextDiff::new(LEFT, LEFT).hunks().is_empty());

    let large = "x".repeat(MAX_DIFF_BYTES + 1);
    for diff in [TextDiff::new(&large, RIGHT), TextDiff::new(LEFT, &large)] {
        assert!(diff.too_large());
        assert!(diff.hunks().is_empty());
    }
}

/// Every unchanged row's pair, `(left row, right row)`.
fn unchanged_pairs(diff: &TextDiff, left: &str) -> Vec<(u32, u32)> {
    let mut pairs = Vec::new();
    let (mut l, mut r) = (0, 0);
    for hunk in diff.hunks() {
        while l < hunk.left.start {
            pairs.push((l, r));
            (l, r) = (l + 1, r + 1);
        }
        (l, r) = (hunk.left.end, hunk.right.end);
    }
    while l < row_count(left) {
        pairs.push((l, r));
        (l, r) = (l + 1, r + 1);
    }
    pairs
}

#[test]
fn alignment() {
    let diff = TextDiff::new("a\nb\nc\nd\ne\n", "a\n1\n2\n3\n4\n5\ne\n");
    assert_eq!(diff.hunks(), [hunk(1..4, 1..6)]);
    assert_eq!(diff.fillers(Side::Left), [(4, 2)]);
    assert!(diff.fillers(Side::Right).is_empty());
    assert_eq!(diff.row_at_screen(Side::Left, 4), (4, true));
    assert_eq!(diff.row_at_screen(Side::Left, 5), (4, true));
    assert_eq!(diff.row_at_screen(Side::Left, 6), (4, false));

    // An addition, a removal and a two-row modification of one row.
    let (left, right) = (LEFT, "a\nX\nb\nd\ne\nF1\nF2\ng\n");
    let diff = TextDiff::new(left, right);
    assert_eq!(
        diff.hunks(),
        [hunk(1..1, 1..2), hunk(2..3, 3..3), hunk(5..6, 5..7)]
    );
    assert_eq!(diff.fillers(Side::Left), [(1, 1), (6, 1)]);
    assert_eq!(diff.fillers(Side::Right), [(3, 1)]);
    let pairs = unchanged_pairs(&diff, left);
    assert_eq!(pairs.len(), 6);
    for (l, r) in pairs {
        assert_eq!(
            diff.screen_row(Side::Left, l),
            diff.screen_row(Side::Right, r),
            "left row {l}, right row {r}"
        );
    }
    for (side, text) in [(Side::Left, left), (Side::Right, right)] {
        for row in 0..row_count(text) {
            let screen = diff.screen_row(side, row);
            assert_eq!(
                diff.row_at_screen(side, screen),
                (row, false),
                "{side:?} {row}"
            );
        }
    }
    assert_eq!(diff.row_at_screen(Side::Left, 1), (1, true));
    assert_eq!(diff.row_at_screen(Side::Right, 3), (3, true));
}

#[test]
fn navigation() {
    // Hunks: left 1..1 / right 1..2, left 2..3 / right 3..3, left 5..6 /
    // right 5..6.
    let diff = TextDiff::new(LEFT, RIGHT);

    assert_eq!(diff.next_hunk(Side::Left, 0), Some(0));
    assert_eq!(diff.next_hunk(Side::Left, 1), Some(1));
    assert_eq!(diff.next_hunk(Side::Left, 4), Some(2));
    assert_eq!(diff.next_hunk(Side::Left, 5), None);
    assert_eq!(diff.next_hunk(Side::Left, 7), None);
    assert_eq!(diff.next_hunk(Side::Right, 0), Some(0));
    assert_eq!(diff.next_hunk(Side::Right, 2), Some(1));
    assert_eq!(diff.next_hunk(Side::Right, 3), Some(2));
    assert_eq!(diff.next_hunk(Side::Right, 5), None);

    assert_eq!(diff.prev_hunk(Side::Left, 0), None);
    assert_eq!(diff.prev_hunk(Side::Left, 1), None);
    assert_eq!(diff.prev_hunk(Side::Left, 2), Some(0));
    assert_eq!(diff.prev_hunk(Side::Left, 5), Some(1));
    assert_eq!(diff.prev_hunk(Side::Left, 7), Some(2));
    assert_eq!(diff.prev_hunk(Side::Right, 3), Some(0));
    assert_eq!(diff.prev_hunk(Side::Right, 4), Some(1));
    assert_eq!(diff.prev_hunk(Side::Right, 7), Some(2));

    // The addition's insertion row on the left, the deletion's on the right.
    assert_eq!(diff.hunk_at(Side::Left, 0), None);
    assert_eq!(diff.hunk_at(Side::Left, 1), Some(0));
    assert_eq!(diff.hunk_at(Side::Left, 2), Some(1));
    assert_eq!(diff.hunk_at(Side::Left, 3), None);
    assert_eq!(diff.hunk_at(Side::Left, 5), Some(2));
    assert_eq!(diff.hunk_at(Side::Left, 7), None);
    assert_eq!(diff.hunk_at(Side::Right, 0), None);
    assert_eq!(diff.hunk_at(Side::Right, 1), Some(0));
    assert_eq!(diff.hunk_at(Side::Right, 2), None);
    assert_eq!(diff.hunk_at(Side::Right, 3), Some(1));
    assert_eq!(diff.hunk_at(Side::Right, 5), Some(2));
    assert_eq!(diff.hunk_at(Side::Right, 7), None);

    assert_eq!(diff.removed_above(), [(3, 2..3), (5, 5..6)]);

    // Hunks on the first row and past the last left row.
    let diff = TextDiff::new("a\nb", "x\nb\nc");
    assert_eq!(diff.hunks(), [hunk(0..1, 0..1), hunk(2..2, 2..3)]);
    assert_eq!(diff.hunk_at(Side::Left, 0), Some(0));
    assert_eq!(diff.hunk_at(Side::Left, 1), None);
    assert_eq!(diff.hunk_at(Side::Left, 2), Some(1));
    assert_eq!(diff.hunk_at(Side::Right, 2), Some(1));
    assert_eq!(diff.next_hunk(Side::Left, 0), Some(1));
    assert_eq!(diff.next_hunk(Side::Right, 2), None);
    assert_eq!(diff.prev_hunk(Side::Right, 0), None);
    assert_eq!(diff.prev_hunk(Side::Right, 2), Some(0));
    assert_eq!(diff.removed_above(), [(0, 0..1)]);
}

#[test]
fn apply() {
    let diff = TextDiff::new(LEFT, RIGHT);
    assert_eq!(diff.apply(LEFT, RIGHT, &[0, 1, 2]), RIGHT);
    assert_eq!(diff.apply(LEFT, RIGHT, &[]), LEFT);
    assert_eq!(diff.apply(LEFT, RIGHT, &[1]), "a\nb\nd\ne\nf\ng\n");

    // Part of a 3-into-3 modification: only the chosen pairs.
    let (left, right) = ("a\nb\nc\nd\ne\n", "a\nB\nC\nD\ne\n");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(1..4, 1..4)]);
    assert_eq!(diff.apply_rows(left, right, 2..3), "a\nb\nC\nd\ne\n");
    assert_eq!(diff.apply_rows(left, right, 1..3), "a\nB\nC\nd\ne\n");

    // 2 into 4: unpaired chosen rows go after the last left row.
    let (left, right) = ("a\nb\nc\nz\n", "a\nB\nC\nD\nE\nz\n");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(1..3, 1..5)]);
    assert_eq!(diff.apply_rows(left, right, 2..4), "a\nb\nC\nD\nz\n");
    assert_eq!(diff.apply_rows(left, right, 4..5), "a\nb\nc\nE\nz\n");
    assert_eq!(diff.apply_rows(left, right, 0..6), right);

    // 4 into 2: unpaired left rows go only with the last right row.
    let (left, right) = ("a\nb\nc\nd\ne\nz\n", "a\nB\nC\nz\n");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(1..5, 1..3)]);
    assert_eq!(diff.apply_rows(left, right, 1..2), "a\nB\nc\nd\ne\nz\n");
    assert_eq!(diff.apply_rows(left, right, 2..3), "a\nb\nC\nz\n");

    // A removal staged from its insertion row or the row above it.
    let diff = TextDiff::new(LEFT, RIGHT);
    let without_c = "a\nb\nd\ne\nf\ng\n";
    assert_eq!(diff.apply_rows(LEFT, RIGHT, 2..3), without_c);
    assert_eq!(diff.apply_rows(LEFT, RIGHT, 3..4), without_c);
    assert_eq!(diff.apply_rows(LEFT, RIGHT, 4..5), LEFT);
    assert_eq!(
        diff.apply_rows(LEFT, RIGHT, 1..2),
        "a\nX\nb\nc\nd\ne\nf\ng\n"
    );

    // CRLF left text keeps CRLF, inserted rows included.
    let (left, right) = ("a\r\nb\r\nc\r\n", "a\nB\nc\nd\n");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(1..2, 1..2), hunk(3..3, 3..4)]);
    assert_eq!(diff.apply(left, right, &[0, 1]), "a\r\nB\r\nc\r\nd\r\n");
    assert_eq!(diff.apply_rows(left, right, 3..4), "a\r\nb\r\nc\r\nd\r\n");

    // The final line break follows the right when a chosen hunk reaches the
    // last row, and stays the left's otherwise.
    let (left, right) = ("a\nb", "a\nc\n");
    assert_eq!(TextDiff::new(left, right).apply(left, right, &[0]), right);
    let (left, right) = ("a\nb\n", "a\nb");
    assert_eq!(TextDiff::new(left, right).apply(left, right, &[0]), right);
    let (left, right) = ("a\nb", "A\nb\n");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(0..1, 0..1), hunk(2..2, 2..3)]);
    assert_eq!(diff.apply(left, right, &[0]), "A\nb");
    assert_eq!(diff.apply(left, right, &[0, 1]), right);

    // Through apply_rows too, selecting only part of a hunk at the end.
    let (left, right) = ("a\nb", "a\nc\n");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(1..2, 1..3)]);
    assert_eq!(diff.apply_rows(left, right, 1..2), right);
    let (left, right) = ("a\nb\n", "a\nB\nC");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(1..3, 1..3)]);
    assert_eq!(diff.apply_rows(left, right, 1..2), "a\nB");
    assert_eq!(diff.apply_rows(left, right, 0..1), left);
    let (left, right) = ("a\nb", "a\nb\nc\nd\n");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(2..2, 2..5)]);
    assert_eq!(diff.apply_rows(left, right, 2..3), "a\nb\nc\n");

    // A lone empty row has no line break; every trailing empty row goes
    // when the right has none.
    let (left, right) = ("a", "\n");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(0..1, 0..2)]);
    assert_eq!(diff.apply_rows(left, right, 0..1), right);
    assert_eq!(diff.apply(left, right, &[0]), right);
    let (left, right) = ("a\nb\n\n", "a\nB\nC");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(1..4, 1..3)]);
    assert_eq!(diff.apply_rows(left, right, 1..2), "a\nB");

    // An empty or reversed selection inside an end hunk changes nothing.
    for (left, right) in [("a\nb", "a\nc\n"), ("a\nb\n", "a\nB\nC")] {
        let diff = TextDiff::new(left, right);
        for chosen in [2..2, 1..1, 3..3] {
            assert_eq!(
                diff.apply_rows(left, right, chosen.clone()),
                left,
                "{left:?} {chosen:?}"
            );
        }
        #[allow(clippy::reversed_empty_ranges)]
        let reversed = 2..1;
        assert_eq!(diff.apply_rows(left, right, reversed), left, "{left:?}");
    }
}

#[test]
fn apply_keeps_rows_between_bare_cr_and_lf() {
    // Deleting b leaves a's `\r` before the empty row's `\n`.
    let (left, right) = ("a\rb\n\nz", "a\n\nz");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(1..2, 1..1)]);
    for staged in [
        diff.apply(left, right, &[0]),
        diff.apply_rows(left, right, 0..1),
    ] {
        assert_eq!(staged, "a\r\n\nz");
    }

    // An empty row replacing c, after b's `\r`, takes `\r` too.
    let (left, right) = ("a\nb\rc\rd", "a\nb\r\rd");
    let diff = TextDiff::new(left, right);
    assert_eq!(diff.hunks(), [hunk(2..3, 2..3)]);
    for staged in [
        diff.apply(left, right, &[0]),
        diff.apply_rows(left, right, 2..3),
    ] {
        assert_eq!(staged, "a\nb\r\rd");
    }
}

#[test]
fn apply_many_hunks() {
    // Every other row changes: 2,000 one-row hunks.
    let rows = 4_000;
    let left: String = (0..rows).map(|i| format!("{i}\n")).collect();
    let right: String = (0..rows)
        .map(|i| {
            if i % 2 == 0 {
                format!("{i}\n")
            } else {
                format!("x{i}\n")
            }
        })
        .collect();
    let diff = TextDiff::new(&left, &right);
    let count = diff.hunks().len();
    assert_eq!(count, rows / 2);

    let all: Vec<usize> = (0..count).rev().collect();
    assert_eq!(diff.apply(&left, &right, &all), right);
    // Duplicate and out-of-range indexes are ignored.
    let mut picked = vec![count + 5, 1, 1, usize::MAX];
    picked.extend(0..1);
    let staged = diff.apply(&left, &right, &picked);
    let expected = left
        .replacen("1\n", "x1\n", 1)
        .replacen("\n3\n", "\nx3\n", 1);
    assert_eq!(staged, expected);
}
