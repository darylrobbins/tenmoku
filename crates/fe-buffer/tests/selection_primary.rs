//! `SelectionSet` tracks a primary selection through sorting, merging and
//! edits, and otherwise takes the last.

use std::ops::Range;

use fe_buffer::{Buffer, Origin, ResolvedSelection, SelectionSet};

fn sel(range: Range<usize>) -> ResolvedSelection {
    ResolvedSelection {
        range,
        reversed: false,
    }
}

fn primary(set: &SelectionSet, buffer: &Buffer) -> Range<usize> {
    set.primary_in(&buffer.snapshot()).unwrap().range
}

#[test]
fn merge_and_fallback() {
    let mut buffer = Buffer::new("one two three four");
    let snap = buffer.snapshot();

    // None tracked: the last, whatever the order given.
    let set = SelectionSet::from_resolved(&snap, [sel(8..13), sel(0..3)]);
    assert_eq!(primary(&set, &buffer), 8..13);
    assert_eq!(set.primary_index_in(&snap), None);

    // Kept through sorting.
    let set = SelectionSet::from_resolved_with_primary(&snap, [sel(8..13), sel(0..3)], 1);
    assert_eq!(primary(&set, &buffer), 0..3);
    assert_eq!(set.primary_index_in(&snap), Some(0));
    assert_eq!(set.resolve(&snap).len(), 2);

    // Merged into an overlapping selection, that one is primary.
    let set = SelectionSet::from_resolved_with_primary(
        &snap,
        [sel(0..3), sel(4..9), sel(6..7), sel(14..18)],
        2,
    );
    assert_eq!(primary(&set, &buffer), 4..9);
    let set = SelectionSet::from_resolved_with_primary(&snap, [sel(5..5), sel(5..5), sel(9..9)], 0);
    assert_eq!(primary(&set, &buffer), 5..5);

    // Edits that merge it into another (carets only, ranges just touch):
    // followed, and through a refresh.
    let mut set =
        SelectionSet::from_resolved_with_primary(&snap, [sel(3..3), sel(4..4), sel(14..18)], 1);
    buffer.edit([(3..4, "")], Origin::User);
    let snap = buffer.snapshot();
    assert_eq!(set.resolve(&snap).len(), 2);
    assert_eq!(primary(&set, &buffer), 3..3);
    set.refresh(&snap);
    assert_eq!(primary(&set, &buffer), 3..3);
    assert_eq!(set.primary_index_in(&snap), Some(0));

    // Edits elsewhere leave it be.
    buffer.edit([(0..0, "zero ")], Origin::User);
    assert_eq!(primary(&set, &buffer), 8..8);

    // An empty set has none.
    let set = SelectionSet::from_resolved(&buffer.snapshot(), []);
    assert_eq!(set.primary_in(&buffer.snapshot()), None);
}
