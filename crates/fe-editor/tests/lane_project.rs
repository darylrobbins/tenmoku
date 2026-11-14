//! `lane_project`: a lane's base→lane hunks over the user's current text,
//! placed only by the placement rule (`docs/designs/agent-lanes.md`
//! § Detached hunks), and the whole-file preconditions of new and deleted
//! files.

use std::ops::Range;

use fe_buffer::{Buffer, HunkStatus, Origin, ProposalError};
use fe_editor::lane_project::{Placement, project};
use fe_editor::{FilePrecondition, PreconditionCheck, TextDiff, lane_project};
use proptest::prelude::*;

fn origin() -> Origin {
    Origin::Host("lane-test".into())
}

/// Each hunk's status and old text, as projected onto `current`.
fn statuses(base: &str, lane: &str, current: &str) -> Vec<(HunkStatus, String)> {
    let buffer = Buffer::new(current);
    let change = lane_project(base, lane, buffer.current(), origin());
    change
        .hunks(buffer.current())
        .into_iter()
        .map(|hunk| (hunk.status, hunk.old_text.to_string()))
        .collect()
}

fn pending(old: &str) -> (HunkStatus, String) {
    (HunkStatus::Pending, old.to_owned())
}

fn stale(old: &str) -> (HunkStatus, String) {
    (HunkStatus::Stale, old.to_owned())
}

#[test]
fn unchanged_current_places_every_hunk() {
    // Repeated rows: no window is unique, but with nothing changed there's
    // no alignment to choose.
    let base = "x\nx\nx\ny\nx\nx\n";
    let lane = "x\nX\nx\ny\nx\nnew\nx\nx";
    let mut buffer = Buffer::new(base);
    let mut change = lane_project(base, lane, buffer.current(), origin());
    let hunks = change.hunks(buffer.current());
    assert!(hunks.len() >= 3, "{hunks:?}");
    assert!(
        hunks.iter().all(|hunk| hunk.status == HunkStatus::Pending),
        "{hunks:?}"
    );
    let (_, skipped) = change.accept_all(&mut buffer);
    assert!(skipped.is_empty());
    assert_eq!(buffer.text().to_string(), lane);

    // Each placed where it is in the base.
    for hunk in project(base, lane, base) {
        assert_eq!(hunk.hunk.at, Some(hunk.base.clone()));
        assert!(!hunk.hunk.stale);
    }
}

#[test]
fn base_xx_to_current_x_leaves_a_hunk_on_either_row_stale() {
    let (base, current) = ("x\nx\n", "x\n");
    for lane in ["y\nx\n", "x\ny\n", "x\nx\ny\n", "y\nx\nx\n", "x\nmid\nx\n"] {
        let rows = statuses(base, lane, current);
        assert!(!rows.is_empty(), "{lane:?}");
        assert!(
            rows.iter().all(|(status, _)| *status == HunkStatus::Stale),
            "{lane:?}: {rows:?}"
        );
    }
}

#[test]
fn deleted_target_with_an_identical_row_elsewhere_is_stale() {
    let base = "a\nx\nb\nc\nd\ne\nx\nf\n";
    let lane = "a\nX\nb\nc\nd\ne\nx\nf\n";
    // The user deletes the first `x`; the second, and the rows the hunk's
    // window would need, are still there.
    let current = "a\nb\nc\nd\ne\nx\nf\n";
    let mut buffer = Buffer::new(current);
    let mut change = lane_project(base, lane, buffer.current(), origin());
    let hunks = change.hunks(buffer.current());
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].status, HunkStatus::Stale);
    assert_eq!(&*hunks[0].old_text, "x\n");
    assert_eq!(
        change.accept(&mut buffer, hunks[0].id),
        Err(ProposalError::Stale)
    );
    assert_eq!(buffer.text().to_string(), current);
    assert_eq!(project(base, lane, current)[0].hunk.at, None);
}

#[test]
fn moved_text_is_stale() {
    let base = "a\nb\nc\nd\ne\nf\ng\nh\n";
    // The lane edits c, and a row away from it, g.
    let lane = "a\nb\nC\nd\ne\nf\nG\nh\n";
    // The user moves b..d below f.
    let current = "a\ne\nf\nb\nc\nd\ng\nh\n";
    assert_eq!(statuses(base, lane, current), [stale("c\n"), stale("g\n")]);
    // Moving it back makes nothing stale.
    assert_eq!(statuses(base, lane, base), [pending("c\n"), pending("g\n")]);
}

#[test]
fn insertions_beside_an_edit_are_stale() {
    let base = "a\nb\nc\nd\ne\nf\ng\nh\n";
    // The user edits d.
    let current = "a\nb\nc\nD\ne\nf\ng\nh\n";
    let cases = [
        // Right above and right below the edit.
        ("a\nb\nc\nnew\nd\ne\nf\ng\nh\n", false),
        ("a\nb\nc\nd\nnew\ne\nf\ng\nh\n", false),
        // Its window (c, d, e) holds the edit.
        ("a\nb\nnew\nc\nd\ne\nf\ng\nh\n", false),
        // Its window (e, f, g) borders it.
        ("a\nb\nc\nd\ne\nnew\nf\ng\nh\n", false),
        // Well away from it.
        ("a\nb\nc\nd\ne\nf\nnew\ng\nh\n", true),
        ("new\na\nb\nc\nd\ne\nf\ng\nh\n", true),
    ];
    for (lane, placed) in cases {
        let rows = statuses(base, lane, current);
        let want = if placed { pending("") } else { stale("") };
        assert_eq!(rows, [want], "{lane:?}");
    }
    // An insertion beside a row the user deleted, or where the user inserted.
    let deleted = "a\nb\nc\ne\nf\ng\nh\n";
    let inserted = "a\nb\nc\nuser\nd\ne\nf\ng\nh\n";
    for current in [deleted, inserted] {
        for lane in [
            "a\nb\nc\nnew\nd\ne\nf\ng\nh\n",
            "a\nb\nc\nd\nnew\ne\nf\ng\nh\n",
        ] {
            assert_eq!(
                statuses(base, lane, current),
                [stale("")],
                "{current:?} {lane:?}"
            );
        }
    }
}

#[test]
fn placed_hunks_follow_the_users_edits_elsewhere_and_accept() {
    let base = "fn one() {}\n\nfn two() {\n    2\n}\n\nfn three() {}\n";
    let lane = "fn one() {}\n\nfn two() {\n    20\n}\n\nfn three() {}\n";
    let current = "// top\n// more\n\nfn one() {}\n\nfn two() {\n    2\n}\n\nfn three() {}\n";
    let mut buffer = Buffer::new(current);
    let mut change = lane_project(base, lane, buffer.current(), origin());
    let hunks = change.hunks(buffer.current());
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].status, HunkStatus::Pending);
    assert_eq!(&current[hunks[0].range.clone()], "    2\n");
    assert!(change.accept(&mut buffer, hunks[0].id).is_ok());
    assert_eq!(
        buffer.text().to_string(),
        "// top\n// more\n\nfn one() {}\n\nfn two() {\n    20\n}\n\nfn three() {}\n"
    );
}

#[test]
fn last_row_without_a_line_break() {
    // Appending to, and replacing, a last row with no line break.
    for (base, lane) in [
        ("a\nb", "a\nb\nc"),
        ("a\nb", "a\nc"),
        ("a", "b\n"),
        ("", "a\n"),
    ] {
        let mut buffer = Buffer::new(base);
        let mut change = lane_project(base, lane, buffer.current(), origin());
        let (_, skipped) = change.accept_all(&mut buffer);
        assert!(skipped.is_empty());
        assert_eq!(buffer.text().to_string(), lane, "{base:?} to {lane:?}");
    }
}

#[test]
fn line_endings_read_as_a_buffer_reads_them() {
    let base = "a\r\nb\r\nc\r\n";
    let lane = "a\r\nB\r\nc\r\n";
    let mut buffer = Buffer::from_file_contents(base);
    let mut change = lane_project(base, lane, buffer.current(), origin());
    let hunks = change.hunks(buffer.current());
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].status, HunkStatus::Pending);
    assert_eq!(&*hunks[0].new_text, "B\n");
    assert!(change.accept(&mut buffer, hunks[0].id).is_ok());
    assert_eq!(buffer.text().to_string(), "a\nB\nc\n");
}

#[test]
fn no_change_no_hunks() {
    let buffer = Buffer::new("a\nb\n");
    let change = lane_project("a\nb\n", "a\nb\n", buffer.current(), origin());
    assert_eq!(change.hunk_count(), 0);
}

#[test]
fn file_preconditions() {
    use PreconditionCheck::*;
    assert_eq!(
        FilePrecondition::of(None, Some("new")),
        Some(FilePrecondition::Absent)
    );
    assert_eq!(
        FilePrecondition::of(Some("old"), None),
        Some(FilePrecondition::Text("old".into()))
    );
    assert_eq!(FilePrecondition::of(Some("a"), Some("b")), None);
    assert_eq!(FilePrecondition::of(None, None), None);

    // New file: add/add is stale.
    let new = FilePrecondition::Absent;
    assert_eq!(new.check(None), Holds);
    assert_eq!(new.check(Some("")), Stale);
    assert_eq!(new.check(Some("the user's")), Stale);

    // Deletion: delete/modify is stale; already deleted is done.
    let deletion = FilePrecondition::Text("a\nb\n".into());
    assert_eq!(deletion.check(Some("a\nb\n")), Holds);
    assert_eq!(deletion.check(Some("a\r\nb\r\n")), Holds);
    assert_eq!(deletion.check(Some("a\nb")), Stale);
    assert_eq!(deletion.check(Some("a\nB\n")), Stale);
    assert_eq!(deletion.check(Some("")), Stale);
    assert_eq!(deletion.check(None), AlreadyDone);
}

// The placement rule written out naively, as the oracle for the property
// tests: rows split at `\n`, occurrences counted at every byte.

fn row_starts(text: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(text.match_indices('\n').map(|(at, _)| at + 1))
        .collect()
}

fn row_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count()
}

fn count(text: &str, needle: &str) -> usize {
    (0..=text.len())
        .filter(|&at| text.as_bytes()[at..].starts_with(needle.as_bytes()))
        .count()
}

/// The window's rows and bytes in `base`.
fn window(base: &str, range: &Range<usize>) -> (Range<usize>, Range<usize>) {
    let starts = row_starts(base);
    let first = row_of(base, range.start);
    let last = if range.is_empty() {
        first
    } else {
        row_of(base, range.end - 1)
    };
    let rows = first.saturating_sub(1)..(last + 2).min(starts.len());
    let byte = |row: usize| starts.get(row).copied().unwrap_or(base.len());
    let bytes = byte(rows.start)..byte(rows.end);
    (rows, bytes)
}

/// Where the rule places `range`, by the definition.
fn oracle(base: &str, current: &str, range: Range<usize>) -> Option<Range<usize>> {
    if base == current {
        return Some(range);
    }
    let (rows, bytes) = window(base, &range);
    let diff = TextDiff::new(base, current);
    let mut shift = 0i64;
    for hunk in diff.hunks() {
        let (start, end) = (hunk.left.start as usize, hunk.left.end as usize);
        if start <= rows.end && end >= rows.start {
            return None;
        }
        if end < rows.start {
            shift += hunk.right.len() as i64 - hunk.left.len() as i64;
        }
    }
    let image_row = (rows.start as i64 + shift) as usize;
    let image = row_starts(current)
        .get(image_row)
        .copied()
        .unwrap_or(current.len());
    let needle = &base[bytes.clone()];
    if count(base, needle) != 1 || count(current, needle) != 1 {
        return None;
    }
    if !current.as_bytes()[image..].starts_with(needle.as_bytes()) {
        return None;
    }
    let offset = range.start - bytes.start;
    Some(image + offset..image + offset + range.len())
}

/// Rows from a small alphabet, so windows repeat, with some rows of their
/// own, so others are unique.
fn rows() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(
        prop_oneof![
            3 => prop::sample::select(vec!["x", "y", "", "x y"]).prop_map(str::to_owned),
            2 => (0u32..40).prop_map(|n| format!("r{n}")),
        ],
        0..24,
    )
}

/// Rows with some deleted, replaced, inserted or moved.
fn edit(rows: Vec<String>) -> impl Strategy<Value = Vec<String>> {
    let len = rows.len();
    prop::collection::vec((0..=len, 0u8..4, 0u32..40), 0..4).prop_map(move |edits| {
        let mut rows = rows.clone();
        for (at, kind, n) in edits {
            let at = at.min(rows.len());
            match kind {
                0 if at < rows.len() => {
                    rows.remove(at);
                }
                1 if at < rows.len() => rows[at] = format!("e{n}"),
                2 if at < rows.len() => {
                    let row = rows.remove(at);
                    let to = (n as usize) % (rows.len() + 1);
                    rows.insert(to, row);
                }
                _ => rows.insert(
                    at,
                    if n % 2 == 0 {
                        "x".into()
                    } else {
                        format!("i{n}")
                    },
                ),
            }
        }
        rows
    })
}

fn join(rows: &[String], line_break_at_end: bool) -> String {
    let mut text = rows.join("\n");
    if line_break_at_end && !rows.is_empty() {
        text.push('\n');
    }
    text
}

fn three_texts() -> impl Strategy<Value = (String, String, String)> {
    (rows(), any::<[bool; 3]>()).prop_flat_map(|(base, ends)| {
        (edit(base.clone()), edit(base.clone())).prop_map(move |(lane, current)| {
            (
                join(&base, ends[0]),
                join(&lane, ends[1]),
                join(&current, ends[2]),
            )
        })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// A placed hunk's window occurs once in each text and lands where the
    /// base→current diff maps it, holding the hunk's old text; every hunk
    /// the rule would place is placed.
    #[test]
    fn placed_window_is_unique_in_both_texts_and_lands_at_the_diffs_image(
        (base, lane, current) in three_texts()
    ) {
        let hunks = project(&base, &lane, &current);
        for hunk in &hunks {
            let at = hunk.hunk.at.clone();
            prop_assert_eq!(&at, &oracle(&base, &current, hunk.base.clone()));
            prop_assert_eq!(hunk.hunk.stale, at.is_none());
            prop_assert_eq!(&*hunk.hunk.old_text, &base[hunk.base.clone()]);
            if let Some(at) = at {
                prop_assert_eq!(&current[at.clone()], &*hunk.hunk.old_text);
                if base != current {
                    let (_, bytes) = window(&base, &hunk.base);
                    let needle = &base[bytes];
                    prop_assert_eq!(count(&base, needle), 1);
                    prop_assert_eq!(count(&current, needle), 1);
                }
            }
        }
        // Accepting every placed hunk applies just those.
        let mut buffer = Buffer::new(&current);
        let mut change = lane_project(&base, &lane, buffer.current(), origin());
        let (_, skipped) = change.accept_all(&mut buffer);
        prop_assert_eq!(skipped.len(), hunks.iter().filter(|h| h.hunk.stale).count());
        if base == current {
            prop_assert_eq!(buffer.text().to_string(), lane);
        }
    }

    /// Any range of the base, row-aligned or not, is placed as the rule says.
    #[test]
    fn placement_of_any_range_follows_the_rule(
        (base, _, current) in three_texts(),
        ends in any::<(usize, usize)>(),
    ) {
        let placement = Placement::new(&base, &current);
        let (a, b) = (ends.0 % (base.len() + 1), ends.1 % (base.len() + 1));
        let range = a.min(b)..a.max(b);
        prop_assert_eq!(placement.place(range.clone()), oracle(&base, &current, range));
    }
}
