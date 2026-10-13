//! `SelectionSet::from_resolved` clips selections to the text before merging them.

use fe_buffer::{Buffer, Origin, ResolvedSelection, SelectionSet};

fn sel(range: std::ops::Range<usize>) -> ResolvedSelection {
    ResolvedSelection {
        range,
        reversed: false,
    }
}

fn ranges(set: &SelectionSet, buffer: &Buffer) -> Vec<std::ops::Range<usize>> {
    set.resolve(&buffer.snapshot())
        .into_iter()
        .map(|selection| selection.range)
        .collect()
}

#[test]
fn carets_clipped_to_one_offset_are_one_selection() {
    let buffer = Buffer::new("abc");
    let snap = buffer.snapshot();
    let set = SelectionSet::from_resolved(&snap, [sel(5..5), sel(9..9)]);
    assert_eq!(set.len(), 1);
    assert_eq!(set, SelectionSet::caret(&snap, 3));

    let buffer = Buffer::new("é");
    let set = SelectionSet::from_resolved(&buffer.snapshot(), [sel(0..0), sel(1..1)]);
    assert_eq!(set.len(), 1);
}

#[test]
fn range_clipped_to_empty_is_a_caret() {
    let mut buffer = Buffer::new("aé");
    let snap = buffer.snapshot();
    let range = SelectionSet::from_resolved(&snap, [sel(2..3)]);
    let caret = SelectionSet::caret(&snap, 3);
    buffer.edit([(3..3, "X")], Origin::User);
    assert_eq!(ranges(&range, &buffer), ranges(&caret, &buffer));
    assert_eq!(ranges(&range, &buffer), vec![3..3]);
}

#[test]
fn ranges_inside_the_text_keep_their_shape() {
    let mut buffer = Buffer::new("abcdef");
    let set = SelectionSet::from_resolved(&buffer.snapshot(), [sel(4..6), sel(1..3)]);
    assert_eq!(set.len(), 2);
    buffer.edit([(6..6, "!")], Origin::User);
    buffer.edit([(4..4, "?")], Origin::User);
    assert_eq!(ranges(&set, &buffer), vec![1..3, 5..7]);
}
