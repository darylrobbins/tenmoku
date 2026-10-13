//! The edit log drops entries below a floor the buffer's owner picks.

use std::ops::Range;

use rand::{Rng, SeedableRng, rngs::StdRng};

use super::history_cap::entry;

use super::{disjoint_ranges, is_user, random_insert};
use crate::*;

/// One random batch of edits, or an undo or redo, applied to both buffers.
/// The floor stays a few steps back, so undo and redo only reach
/// transactions both buffers still have.
fn random_step(rng: &mut StdRng, buffers: [&mut Buffer; 2]) {
    let choice = rng.random_range(0..10);
    let text = buffers[0].text().to_string();
    let count = rng.random_range(1..3);
    let edits: Vec<(Range<usize>, String)> = disjoint_ranges(rng, &text, count)
        .into_iter()
        .map(|range| (range, random_insert(rng)))
        .collect();
    let [a, b] = buffers;
    match choice {
        0 => assert_eq!(a.undo(is_user), b.undo(is_user)),
        1 => assert_eq!(a.redo(is_user), b.redo(is_user)),
        _ => assert_eq!(
            a.edit(edits.clone(), Origin::User),
            b.edit(edits, Origin::User)
        ),
    }
}

#[test]
fn anchors_at_or_above_the_floor_resolve_as_before() {
    let mut rng = StdRng::seed_from_u64(0x10c_c0de);
    let source = "fn main() {\n    println!(\"héllo 😀\");\n}\n";
    let mut full = Buffer::new(source);
    let mut compacted = Buffer::new(source);
    let mut anchors = Vec::new();
    let mut floor = Version::default();
    for round in 0..20 {
        for _ in 0..10 {
            random_step(&mut rng, [&mut full, &mut compacted]);
            let snapshot = full.snapshot();
            let offset = rng.random_range(0..=snapshot.len());
            let bias = if rng.random_bool(0.5) {
                Bias::Left
            } else {
                Bias::Right
            };
            anchors.push(snapshot.anchor_at(offset, bias));
        }
        // Compact below the last few rounds' anchors.
        if round % 3 == 2 {
            floor = anchors[anchors.len() - 25].version();
            let version = compacted.version();
            compacted.compact_log(floor);
            assert_eq!(compacted.version(), version);
        }
        assert_eq!(compacted.text().to_string(), full.text().to_string());
        assert_eq!(compacted.current().oldest_version(), floor);

        let (full, compacted) = (full.snapshot(), compacted.snapshot());
        let live: Vec<Anchor> = anchors
            .iter()
            .copied()
            .filter(|anchor| anchor.version() >= floor)
            .collect();
        for anchor in &live {
            assert_eq!(compacted.resolve(anchor), full.resolve(anchor));
        }
        assert_eq!(compacted.resolve_many(&live), full.resolve_many(&live));
        assert!(
            compacted.edits_since(floor).eq(full.edits_since(floor)),
            "edits_since the floor differ"
        );
    }
    assert!(floor > Version::default());
}

#[test]
fn compaction_keeps_old_snapshots_and_refuses_older_bases() {
    let mut buffer = Buffer::new("");
    let base = buffer.snapshot();
    let anchor = base.anchor_after(0);
    buffer.edit([(0..0, "ab")], Origin::User);
    buffer.edit([(1..1, "X")], Origin::User);
    let before = buffer.snapshot();
    buffer.compact_log(Version(1));
    // Clamped to the current version.
    buffer.compact_log(Version(100));
    assert_eq!(buffer.current().oldest_version(), buffer.version());

    // A snapshot taken before compacting keeps its own log.
    assert_eq!(before.oldest_version(), Version::default());
    assert_eq!(before.resolve(&anchor), 3);
    assert_eq!(
        buffer.edit_from(&base, [(0..0, "!")], Origin::User),
        Err(EditError::ForeignSnapshot)
    );
    let current = buffer.snapshot();
    let resolved = std::panic::catch_unwind(|| current.resolve(&anchor));
    assert!(resolved.is_err(), "anchors below the floor don't resolve");
}

#[test]
fn undo_below_the_floor_is_refused_without_changing_text() {
    // Closed transactions below the floor are dropped from the history.
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "a")], Origin::User);
    let b = buffer.edit([(1..1, "b")], Origin::User).unwrap();
    assert_eq!(buffer.history_bytes(&Origin::User), 2 * entry(1));
    buffer.compact_log(Version(1));
    assert_eq!(buffer.history_bytes(&Origin::User), entry(1));
    assert_eq!(buffer.undo(is_user), Ok(Some(b)));
    assert_eq!(buffer.undo(is_user), Ok(None));
    assert_eq!(buffer.text().to_string(), "a");

    // An open transaction that straddles the floor is kept, but undoing it
    // is refused.
    let mut buffer = Buffer::new("");
    let id = buffer.start_transaction(Origin::User);
    buffer.edit([(0..0, "x")], Origin::User);
    buffer.compact_log(buffer.version());
    buffer.edit([(1..1, "y")], Origin::User);
    let version = buffer.version();
    assert_eq!(buffer.undo(is_user), Err(UndoError::Conflict(id)));
    assert_eq!(buffer.text().to_string(), "xy");
    assert_eq!(buffer.version(), version);
    // Once closed, the next compaction drops it.
    buffer.compact_log(version);
    assert_eq!(buffer.undo(is_user), Ok(None));

    // An undone transaction whose original batch is below the floor is
    // dropped too, so it can't be redone.
    let mut buffer = Buffer::new("");
    let a = buffer.edit([(0..0, "a")], Origin::User).unwrap();
    assert_eq!(buffer.undo(is_user), Ok(Some(a)));
    buffer.compact_log(Version(1));
    assert_eq!(buffer.redo(is_user), Ok(None));
    assert_eq!(buffer.text().to_string(), "");
    assert_eq!(buffer.history_bytes(&Origin::User), 0);
}

#[test]
fn undo_that_depends_on_an_edit_below_the_floor_is_refused() {
    let is_lsp = |origin: &Origin| *origin == Origin::Lsp;

    // The transaction's batch is a redo of an edit below the floor.
    let mut buffer = Buffer::new("");
    let a = buffer.edit([(0..0, "a")], Origin::User).unwrap();
    assert_eq!(buffer.undo(is_user), Ok(Some(a)));
    assert_eq!(buffer.redo(is_user), Ok(Some(a)));
    buffer.compact_log(Version(2));
    assert_eq!(buffer.undo(is_user), Err(UndoError::Conflict(a)));
    assert_eq!(buffer.text().to_string(), "a");

    // The transaction maps through an undo of an edit below the floor.
    let mut buffer = Buffer::new("");
    let a = buffer.edit([(0..0, "a")], Origin::User).unwrap();
    let c = buffer.edit([(1..1, "c")], Origin::Lsp).unwrap();
    assert_eq!(buffer.undo(is_user), Ok(Some(a)));
    buffer.compact_log(Version(1));
    let version = buffer.version();
    assert_eq!(buffer.undo(is_lsp), Err(UndoError::Conflict(c)));
    assert_eq!(buffer.text().to_string(), "c");
    assert_eq!(buffer.version(), version);
}
