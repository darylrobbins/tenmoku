//! `MarkerSet` against naive models: markers follow edits as
//! `BufferSnapshot::map_offset` maps their ends, and queries return what a
//! linear filter finds.

use std::{collections::HashSet, ops::Range};

use fe_buffer::{Bias, Buffer, BufferSnapshot, MarkerError, MarkerId, MarkerSet, Origin, Version};
use proptest::prelude::*;

/// Whether `item` overlaps `range`, an empty one touching the other
/// included: as `DecorationSet` queries count them.
fn overlaps(item: &Range<usize>, range: &Range<usize>) -> bool {
    if item.is_empty() || range.is_empty() {
        item.start <= range.end && range.start <= item.end
    } else {
        item.start < range.end && range.start < item.end
    }
}

fn listed<T: Clone>(set: &MarkerSet<T>, range: Range<usize>) -> Vec<(MarkerId, Range<usize>, T)> {
    set.iter_in(range)
        .map(|(id, range, value)| (id, range, value.clone()))
        .collect()
}

/// `model`'s markers overlapping `range`, by start then insertion.
fn naive<T: Clone>(
    model: &[(MarkerId, Range<usize>, T)],
    range: Range<usize>,
) -> Vec<(MarkerId, Range<usize>, T)> {
    let mut hits: Vec<_> = (model.iter())
        .filter(|(_, item, _)| overlaps(item, &range))
        .cloned()
        .collect();
    hits.sort_by_key(|(id, range, _)| (range.start, *id));
    hits
}

/// A range of a text `len` bytes long from a raw start and length.
fn clip(len: usize, (start, length): (usize, usize)) -> Range<usize> {
    let start = start % (len + 1);
    start..(start + length).min(len)
}

fn raw_range() -> impl Strategy<Value = (usize, usize)> {
    (
        any::<usize>(),
        prop_oneof![Just(0), 0..4usize, 0..40usize, 0..400usize],
    )
}

fn text(max: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(prop_oneof![Just('a'), Just('b'), Just('\n')], 0..max)
        .prop_map(|chars| chars.into_iter().collect())
}

fn bias() -> impl Strategy<Value = Bias> {
    prop_oneof![Just(Bias::Left), Just(Bias::Right)]
}

/// Raw edits for one batch: a range and the text to put there.
fn batch() -> impl Strategy<Value = Vec<((usize, usize), String)>> {
    proptest::collection::vec((raw_range(), text(6)), 1..4)
}

fn apply_batch(buffer: &mut Buffer, batch: &[((usize, usize), String)]) {
    let len = buffer.len();
    let edits: Vec<(Range<usize>, &str)> = (batch.iter())
        .map(|(range, text)| (clip(len, *range), text.as_str()))
        .collect();
    buffer.edit(edits, Origin::User);
}

/// A marker as inserted: its clipped range at the version it went in at.
struct Inserted {
    id: MarkerId,
    range: Range<usize>,
    version: Version,
}

/// Where `marker` is in `snapshot`, mapping its inserted ends.
fn mapped(snapshot: &BufferSnapshot, biases: (Bias, Bias), marker: &Inserted) -> Range<usize> {
    let start = snapshot.map_offset(marker.range.start, biases.0, marker.version);
    let end = snapshot.map_offset(marker.range.end, biases.1, marker.version);
    start..end.max(start)
}

fn check_synced(
    set: &MarkerSet<usize>,
    snapshot: &BufferSnapshot,
    biases: (Bias, Bias),
    inserted: &[Inserted],
) {
    let mut expected: Vec<(MarkerId, Range<usize>, usize)> = (inserted.iter().enumerate())
        .map(|(ix, marker)| (marker.id, mapped(snapshot, biases, marker), ix))
        .collect();
    expected.sort_by_key(|(id, range, _)| (range.start, *id));
    assert_eq!(set.version(), snapshot.version());
    assert_eq!(set.len(), inserted.len());
    assert_eq!(listed(set, 0..snapshot.len()), expected);
}

#[derive(Clone, Debug)]
struct Round {
    markers: Vec<(usize, usize)>,
    batch: Vec<((usize, usize), String)>,
    sync: bool,
}

fn round() -> impl Strategy<Value = Round> {
    (
        proptest::collection::vec(raw_range(), 0..=4),
        batch(),
        any::<bool>(),
    )
        .prop_map(|(markers, batch, sync)| Round {
            markers,
            batch,
            sync,
        })
}

#[derive(Clone, Debug)]
enum Op {
    Insert((usize, usize)),
    Splice((usize, usize), Vec<(usize, usize)>),
    Retain(u32),
    /// Removes the marker at this index of the model, or one already gone.
    Remove(usize),
    Clear,
    Edit(Vec<((usize, usize), String)>),
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        8 => raw_range().prop_map(Op::Insert),
        3 => (raw_range(), proptest::collection::vec(raw_range(), 0..6))
            .prop_map(|(range, items)| Op::Splice(range, items)),
        1 => (2..5u32).prop_map(Op::Retain),
        3 => any::<usize>().prop_map(Op::Remove),
        1 => Just(Op::Clear),
        3 => batch().prop_map(Op::Edit),
    ]
}

proptest! {
    // The slowest property here; the others keep 256 cases below.
    #![proptest_config(ProptestConfig { cases: 48, ..ProptestConfig::default() })]

    /// C1: markers inserted along the way, synced after each batch or
    /// several, sit where `map_offset` puts their inserted ends.
    #[test]
    fn sync_follows_map_offset(
        initial in text(200),
        start_bias in bias(),
        end_bias in bias(),
        first in proptest::collection::vec(raw_range(), 0..=80),
        rounds in proptest::collection::vec(round(), 0..=30),
    ) {
        let biases = (start_bias, end_bias);
        let mut buffer = Buffer::new(&initial);
        let mut set = MarkerSet::with_biases(&buffer.snapshot(), start_bias, end_bias);
        let mut inserted = Vec::new();
        // The text's length at the set's version, which inserts are bytes of.
        let mut text_len = buffer.len();
        for raw in first {
            let range = clip(text_len, raw);
            let id = set.insert(range.clone(), inserted.len());
            inserted.push(Inserted { id, range, version: set.version() });
        }
        for round in rounds {
            for raw in round.markers {
                let range = clip(text_len, raw);
                let id = set.insert(range.clone(), inserted.len());
                inserted.push(Inserted { id, range, version: set.version() });
            }
            apply_batch(&mut buffer, &round.batch);
            if round.sync {
                let snapshot = buffer.snapshot();
                set.sync(&snapshot).unwrap();
                text_len = snapshot.len();
                check_synced(&set, &snapshot, biases, &inserted);
            }
        }
        let snapshot = buffer.snapshot();
        set.sync(&snapshot).unwrap();
        check_synced(&set, &snapshot, biases, &inserted);
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    /// C2: `iter_in` finds exactly the markers a linear filter does.
    #[test]
    fn iter_in_matches_filter(
        initial in text(300),
        markers in proptest::collection::vec(raw_range(), 0..200),
        queries in proptest::collection::vec(raw_range(), 1..20),
    ) {
        let buffer = Buffer::new(&initial);
        let len = buffer.len();
        let mut set = MarkerSet::new(&buffer.snapshot());
        let mut model = Vec::new();
        for (ix, raw) in markers.into_iter().enumerate() {
            let range = clip(len, raw);
            let id = set.insert(range.clone(), ix);
            model.push((id, range, ix));
        }
        prop_assert_eq!(listed(&set, 0..len), naive(&model, 0..len));
        for raw in queries {
            let range = clip(len, raw);
            prop_assert_eq!(listed(&set, range.clone()), naive(&model, range));
        }
    }

    /// C3: splice, retain, remove and clear, between edits, keep the set
    /// agreeing with a naive model, and ids are never reused.
    #[test]
    fn edits_match_model(
        initial in text(200),
        ops in proptest::collection::vec(op(), 0..60),
        queries in proptest::collection::vec(raw_range(), 1..4),
    ) {
        let mut buffer = Buffer::new(&initial);
        let mut set = MarkerSet::new(&buffer.snapshot());
        let mut model: Vec<(MarkerId, Range<usize>, u32)> = Vec::new();
        let mut ids = HashSet::new();
        let mut gone = Vec::new();
        let mut next_value = 0u32;
        let mut value = || {
            next_value += 1;
            next_value
        };
        for op in ops {
            let len = buffer.len();
            match op {
                Op::Insert(raw) => {
                    let range = clip(len, raw);
                    let value = value();
                    let id = set.insert(range.clone(), value);
                    prop_assert!(ids.insert(id));
                    model.push((id, range, value));
                }
                Op::Splice(raw, items) => {
                    let range = clip(len, raw);
                    let items: Vec<(Range<usize>, u32)> = (items.into_iter())
                        .map(|raw| (clip(len, raw), value()))
                        .collect();
                    let added = set.splice(range.clone(), items.clone());
                    prop_assert_eq!(added.len(), items.len());
                    model.retain(|(id, item, _)| {
                        let removed = range.contains(&item.start);
                        if removed {
                            gone.push(*id);
                        }
                        !removed
                    });
                    for (id, (item, value)) in added.into_iter().zip(items) {
                        prop_assert!(ids.insert(id));
                        model.push((id, item, value));
                    }
                }
                Op::Retain(modulus) => {
                    let keep = |value: u32| !value.is_multiple_of(modulus);
                    let mut seen = Vec::new();
                    set.retain(|id, range, value| {
                        seen.push((id, range, *value));
                        keep(*value)
                    });
                    prop_assert_eq!(seen, naive(&model, 0..len));
                    model.retain(|(id, _, value)| {
                        if !keep(*value) {
                            gone.push(*id);
                        }
                        keep(*value)
                    });
                }
                Op::Remove(pick) => {
                    if !model.is_empty() && pick % 4 != 0 {
                        let (id, _, value) = model.remove(pick % model.len());
                        prop_assert_eq!(set.remove(id), Some(value));
                        gone.push(id);
                    } else if !gone.is_empty() {
                        prop_assert_eq!(set.remove(gone[pick % gone.len()]), None);
                    }
                }
                Op::Clear => {
                    set.clear();
                    gone.extend(model.drain(..).map(|(id, _, _)| id));
                }
                Op::Edit(batch) => {
                    let from = buffer.version();
                    apply_batch(&mut buffer, &batch);
                    let snapshot = buffer.snapshot();
                    set.sync(&snapshot).unwrap();
                    for (_, range, _) in &mut model {
                        let start = snapshot.map_offset(range.start, Bias::Right, from);
                        let end = snapshot.map_offset(range.end, Bias::Left, from);
                        *range = start..end.max(start);
                    }
                }
            }
            let len = buffer.len();
            prop_assert_eq!(set.len(), model.len());
            prop_assert_eq!(set.is_empty(), model.is_empty());
            prop_assert_eq!(listed(&set, 0..len), naive(&model, 0..len));
            for raw in &queries {
                let range = clip(len, *raw);
                prop_assert_eq!(listed(&set, range.clone()), naive(&model, range));
            }
        }
    }
}

/// C4: a set whose version was compacted away can't sync, and is left as it
/// was.
#[test]
fn sync_below_the_floor_is_compacted() {
    let mut buffer = Buffer::new("hello world");
    let mut set = MarkerSet::new(&buffer.snapshot());
    let id = set.insert(6..11, "world");
    let version = set.version();
    buffer.edit([(0..0, ">> ")], Origin::User);
    buffer.edit([(0..0, "> ")], Origin::User);
    buffer.compact_log(buffer.version());
    let snapshot = buffer.snapshot();
    assert!(set.version() < snapshot.oldest_version());

    assert_eq!(set.sync(&snapshot), Err(MarkerError::Compacted));
    assert_eq!(set.version(), version);
    assert_eq!(listed(&set, 0..11), vec![(id, 6..11, "world")]);
}

/// C4: a snapshot of another buffer is refused, even at the same version.
#[test]
fn sync_with_another_buffer_is_refused() {
    let buffer = Buffer::new("hello world");
    let other = Buffer::new("hello world");
    let mut set = MarkerSet::new(&buffer.snapshot());
    let id = set.insert(0..5, ());
    assert_eq!(set.sync(&other.snapshot()), Err(MarkerError::OtherBuffer));
    assert_eq!(listed(&set, 0..11), vec![(id, 0..5, ())]);
    assert_eq!(set.sync(&buffer.snapshot()), Ok(()));
}

#[test]
fn sync_with_an_older_snapshot_is_refused() {
    let mut buffer = Buffer::new("hello world");
    let old = buffer.snapshot();
    buffer.edit([(0..0, "> ")], Origin::User);
    let mut set = MarkerSet::new(&buffer.snapshot());
    set.insert(2..7, ());
    assert_eq!(set.sync(&old), Err(MarkerError::Older));
    assert_eq!(set.version(), buffer.version());
}

/// Inserted ranges are clipped to the text, and an inverted one is stored
/// empty at its start.
#[test]
fn insert_clips() {
    let buffer = Buffer::new("hello");
    let mut set = MarkerSet::new(&buffer.snapshot());
    let past = set.insert(3..99, 'a');
    #[allow(clippy::reversed_empty_ranges)]
    let inverted = set.insert(4..2, 'b');
    let beyond = set.insert(70..80, 'c');
    assert_eq!(
        listed(&set, 0..5),
        vec![
            (past, 3..5, 'a'),
            (inverted, 4..4, 'b'),
            (beyond, 5..5, 'c')
        ]
    );
}

/// Text typed at a marker's ends stays outside it by default, and markers
/// meeting at one start keep insertion order.
#[test]
fn default_biases_stay_inside_neither() {
    let mut buffer = Buffer::new("abcdef");
    let mut set = MarkerSet::new(&buffer.snapshot());
    let later = set.insert(4..6, 2);
    let word = set.insert(2..4, 0);
    let point = set.insert(4..4, 1);
    buffer.edit([(2..2, "<"), (4..4, ">")], Origin::User);
    set.sync(&buffer.snapshot()).unwrap();
    assert_eq!(
        listed(&set, 0..buffer.len()),
        vec![(word, 3..5, 0), (later, 6..8, 2), (point, 6..6, 1)]
    );
}

/// An edit and its undo, synced together, leave markers where they were, as
/// `map_offset` skips the pair.
#[test]
fn sync_skips_an_undone_batch() {
    let mut buffer = Buffer::new("hello world");
    let mut set = MarkerSet::new(&buffer.snapshot());
    let id = set.insert(6..11, ());
    buffer.edit([(4..8, "")], Origin::User);
    buffer.end_transaction();
    buffer.undo(|_| true).unwrap();
    set.sync(&buffer.snapshot()).unwrap();
    assert_eq!(listed(&set, 0..11), vec![(id, 6..11, ())]);
}
