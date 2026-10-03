//! Properties that must hold whatever sequence of operations runs.

use std::{ops::Range, sync::Arc};

use proptest::prelude::*;
use rand::{Rng, SeedableRng, rngs::StdRng};

use super::{ai, disjoint_ranges, is_user, random_insert, random_range};
use crate::*;

/// Random edits in a few batches, some grouped into transactions.
fn random_user_session(buffer: &mut Buffer, rng: &mut impl Rng, steps: usize) {
    for _ in 0..steps {
        let grouped = rng.random_bool(0.3);
        if grouped {
            buffer.start_transaction(Origin::User);
        }
        for _ in 0..if grouped { 3 } else { 1 } {
            let text = buffer.text().to_string();
            let edits: Vec<(Range<usize>, String)> = disjoint_ranges(rng, &text, 3)
                .into_iter()
                .map(|range| (range, random_insert(rng)))
                .collect();
            buffer.edit(edits, Origin::User);
        }
        if grouped {
            buffer.end_transaction();
        }
    }
}

fn text_strategy() -> impl Strategy<Value = String> {
    proptest::collection::vec(
        prop_oneof![
            4 => proptest::char::range('a', 'f'),
            1 => Just('\n'),
            1 => Just('é'),
            1 => Just('😀'),
        ],
        0..80,
    )
    .prop_map(|chars| chars.into_iter().collect())
}

proptest! {
    /// Undoing everything puts every anchor back exactly where it was, and
    /// redoing everything puts it where the edits left it.
    #[test]
    fn undo_all_returns_anchors_home(source in text_strategy(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(&source);
        let snapshot = buffer.snapshot();
        let anchors: Vec<Anchor> = source
            .char_indices()
            .map(|(ix, _)| ix)
            .chain([source.len()])
            .flat_map(|offset| [snapshot.anchor_before(offset), snapshot.anchor_after(offset)])
            .collect();
        let home: Vec<usize> = anchors.iter().map(|anchor| snapshot.resolve(anchor)).collect();

        let steps = rng.random_range(1..12);
        random_user_session(&mut buffer, &mut rng, steps);
        let edited_text = buffer.text().to_string();
        let edited = buffer.current().resolve_many(&anchors);

        while buffer.undo(is_user).unwrap().is_some() {}
        prop_assert_eq!(buffer.text().to_string(), source);
        prop_assert_eq!(buffer.current().resolve_many(&anchors), home);

        while buffer.redo(is_user).unwrap().is_some() {}
        prop_assert_eq!(buffer.text().to_string(), edited_text);
        prop_assert_eq!(buffer.current().resolve_many(&anchors), edited);
    }

    /// Events form an unbroken chain of versions, and replaying their edit
    /// ranges accounts for every change in length.
    #[test]
    fn events_describe_every_change(source in text_strategy(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(&source);
        let events = buffer.subscribe();
        let mut len = buffer.len();
        let mut version = buffer.version();
        for _ in 0..30 {
            match rng.random_range(0..4) {
                0 => {
                    let _ = buffer.undo(|_| true);
                }
                1 => {
                    let _ = buffer.redo(|_| true);
                }
                _ => {
                    let text = buffer.text().to_string();
                    let edits: Vec<(Range<usize>, String)> = disjoint_ranges(&mut rng, &text, 3)
                        .into_iter()
                        .map(|range| (range, random_insert(&mut rng)))
                        .collect();
                    let origin = if rng.random_bool(0.5) { Origin::User } else { ai() };
                    buffer.edit(edits, origin);
                }
            }
            for event in events.drain() {
                let BufferEvent::Edited { from, to, edits, .. } = event else { continue };
                prop_assert_eq!(from, version, "events skipped a version");
                prop_assert_eq!(to.0, from.0 + 1);
                version = to;
                let mut delta = 0isize;
                let mut last_end = 0;
                for edit in edits.iter() {
                    prop_assert!(edit.old.start >= last_end, "edits overlap or are unsorted");
                    prop_assert!(edit.old.end <= len);
                    prop_assert_eq!(edit.new.start as isize, edit.old.start as isize + delta);
                    delta += edit.new.len() as isize - edit.old.len() as isize;
                    last_end = edit.old.end;
                }
                len = len.checked_add_signed(delta).unwrap();
            }
            prop_assert_eq!(version, buffer.version());
            prop_assert_eq!(len, buffer.len());
        }
    }

    /// With unique characters, an `edit_from` that succeeds replaced exactly
    /// the text it targeted and nothing else; one that fails changed nothing.
    #[test]
    fn edit_from_never_hits_the_wrong_text(seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut next = 0x4E00u32;
        let mut unique = |len: usize| -> String {
            (0..len)
                .map(|_| {
                    next += 1;
                    char::from_u32(next).unwrap()
                })
                .collect()
        };
        let mut buffer = Buffer::new(&unique(30));
        let base = buffer.snapshot();
        let base_text = base.text().to_string();

        // Non-empty targets, so "the text it targeted" is well defined.
        let targets: Vec<Range<usize>> = disjoint_ranges(&mut rng, &base_text, 3)
            .into_iter()
            .filter(|range| !range.is_empty())
            .collect();

        // Meanwhile the user edits anywhere, possibly over the targets.
        for _ in 0..rng.random_range(0..5) {
            let text = buffer.text().to_string();
            let range = random_range(&mut rng, &text);
            buffer.edit([(range, unique(rng.random_range(0..3)))], Origin::User);
        }

        let before = buffer.text().to_string();
        let replacements: Vec<String> = targets.iter().map(|_| unique(2)).collect();
        let edits: Vec<_> = targets.iter().cloned().zip(replacements.iter().cloned()).collect();
        match buffer.edit_from(&base, edits, ai()) {
            Ok(_) => {
                let mut expected = before.clone();
                for (target, replacement) in targets.iter().zip(&replacements) {
                    let old = &base_text[target.clone()];
                    prop_assert_eq!(expected.matches(old).count(), 1, "target text was intact");
                    expected = expected.replacen(old, replacement, 1);
                }
                prop_assert_eq!(buffer.text().to_string(), expected);
            }
            Err(_) => prop_assert_eq!(buffer.text().to_string(), before),
        }
    }

    /// Selection sets stay sorted and non-overlapping through any edits, and
    /// refreshing them never moves them.
    #[test]
    fn selections_stay_well_formed(source in text_strategy(), seed in any::<u64>()) {
        let mut rng = StdRng::seed_from_u64(seed);
        let mut buffer = Buffer::new(&source);
        let selections = (0..rng.random_range(1..6))
            .map(|_| ResolvedSelection {
                range: random_range(&mut rng, &source),
                reversed: rng.random_bool(0.5),
            })
            .collect::<Vec<_>>();
        let mut set = SelectionSet::from_resolved(buffer.current(), selections);
        for _ in 0..10 {
            let text = buffer.text().to_string();
            let edits: Vec<(Range<usize>, String)> = disjoint_ranges(&mut rng, &text, 3)
                .into_iter()
                .map(|range| (range, random_insert(&mut rng)))
                .collect();
            buffer.edit(edits, if rng.random_bool(0.5) { Origin::User } else { ai() });
            let snapshot = buffer.current();
            let resolved = set.resolve(snapshot);
            prop_assert!(!resolved.is_empty());
            for selection in &resolved {
                prop_assert!(selection.range.start <= selection.range.end);
                prop_assert!(selection.range.end <= snapshot.len());
                prop_assert!(snapshot.text().is_char_boundary(selection.range.start));
                prop_assert!(snapshot.text().is_char_boundary(selection.range.end));
            }
            for pair in resolved.windows(2) {
                prop_assert!(pair[0].range.end <= pair[1].range.start, "overlap: {:?}", pair);
                prop_assert!(pair[0].range != pair[1].range, "duplicate caret");
            }
            let mut refreshed = set.clone();
            refreshed.refresh(snapshot);
            prop_assert_eq!(refreshed.resolve(snapshot), resolved);
            set = refreshed;
        }
    }
}

/// Snapshots read on other threads stay frozen while the buffer changes.
#[test]
fn snapshots_are_stable_across_threads() {
    let mut buffer = Buffer::new(&"line of text\n".repeat(2_000));
    let mut rng = StdRng::seed_from_u64(7);
    let mut readers = Vec::new();
    for round in 0..8 {
        let snapshot = buffer.snapshot();
        let expected: Arc<str> = snapshot.text().to_string().into();
        let anchors: Vec<Anchor> = (0..50)
            .map(|ix| snapshot.anchor_after((ix * 397) % snapshot.len()))
            .collect();
        readers.push(std::thread::spawn(move || {
            for _ in 0..20 {
                assert_eq!(&*snapshot.text().to_string(), &*expected, "round {round}");
                for anchor in &anchors {
                    assert!(snapshot.resolve(anchor) <= snapshot.len());
                }
            }
        }));
        for _ in 0..200 {
            let text = buffer.text().to_string();
            let range = random_range(&mut rng, &text);
            buffer.edit([(range, "edit")], Origin::User);
        }
    }
    for reader in readers {
        reader.join().expect("reader saw a snapshot change");
    }
}

/// Every public query keeps working on an empty buffer.
#[test]
fn empty_buffer_edge_cases() {
    let mut buffer = Buffer::new("");
    let snapshot = buffer.snapshot();
    let anchor = snapshot.anchor_after(10);
    assert_eq!(snapshot.resolve(&anchor), 0);
    assert_eq!(buffer.edit([(0..0, "")], Origin::User), None);
    assert_eq!(buffer.undo(|_| true), Ok(None));
    assert_eq!(buffer.redo(|_| true), Ok(None));
    assert!(
        buffer.edit([(5..9, "x")], Origin::User).is_some(),
        "ranges clip"
    );
    assert_eq!(buffer.text().to_string(), "x");
    assert_eq!(buffer.current().resolve(&anchor), 1);
    let selections = SelectionSet::caret(buffer.current(), 99);
    assert_eq!(selections.resolve(buffer.current())[0].range, 1..1);
}
