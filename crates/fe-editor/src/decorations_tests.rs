#![allow(clippy::single_range_in_vec_init)]

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use fe_buffer::{Buffer, Origin};

use super::*;
use crate::EditorModel;

thread_local! {
    /// Entries a [`DecorationSet`] query looked at on this thread.
    pub(super) static EXAMINED: Cell<usize> = const { Cell::new(0) };
}

fn deco(range: Range<usize>) -> Decoration {
    Decoration::new(range, DecorationKind::Highlight, Tint::Accent)
}

fn word(text: &str, word: &str) -> Range<usize> {
    let start = text.find(word).unwrap();
    start..start + word.len()
}

fn edit(buffer: &mut Buffer, range: Range<usize>, text: &str) {
    buffer.edit([(range, text)], Origin::User).unwrap();
}

/// Every decoration's range in `snapshot`, by start.
fn ranges(set: &DecorationSet, snapshot: &BufferSnapshot) -> Vec<Range<usize>> {
    query(set, snapshot, 0..snapshot.len())
}

fn query(set: &DecorationSet, snapshot: &BufferSnapshot, range: Range<usize>) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    set.decorations(snapshot, range, &mut out);
    out.into_iter().map(|item| item.range).collect()
}

/// What each decoration covers in `snapshot`'s text.
fn words(set: &DecorationSet, snapshot: &BufferSnapshot) -> Vec<String> {
    (ranges(set, snapshot).into_iter())
        .map(|range| snapshot.text().slice_to_string(range))
        .collect()
}

#[test]
fn follows_edits_before_inside_and_over() {
    let mut buffer = Buffer::new("one two three\n");
    let base = buffer.snapshot();
    let text = base.text().to_string();
    let mut set = DecorationSet::new();
    let items = vec![deco(word(&text, "two")), deco(word(&text, "three"))];
    assert!(set.apply(&base, base.version(), items));
    assert_eq!(words(&set, &base), ["two", "three"]);

    // Before.
    edit(&mut buffer, 0..0, ">> ");
    assert_eq!(words(&set, &buffer.snapshot()), ["two", "three"]);
    // Inside.
    let at = buffer.text().to_string().find("two").unwrap() + 1;
    edit(&mut buffer, at..at, "--");
    assert_eq!(words(&set, &buffer.snapshot()), ["t--wo", "three"]);
    // Refreshed, then deleted over: an empty range at the deletion point.
    set.refresh(&buffer.snapshot());
    let gone = word(&buffer.text().to_string(), "three");
    edit(&mut buffer, gone.start - 1..gone.end, "");
    let now = buffer.snapshot();
    assert_eq!(ranges(&set, &now)[1], gone.start - 1..gone.start - 1);
    assert_eq!(words(&set, &now), ["t--wo", ""]);
    assert_eq!(set.len(), 2);
}

#[test]
fn applied_from_an_older_version_lands_rebased() {
    let mut buffer = Buffer::new("alpha beta\n");
    let old = buffer.snapshot();
    let text = old.text().to_string();
    edit(&mut buffer, 0..0, "// x\n");
    edit(&mut buffer, 11..11, "!");
    let now = buffer.snapshot();
    let mut set = DecorationSet::new();
    let items = vec![deco(word(&text, "beta")), deco(word(&text, "alpha"))];
    assert!(set.apply(&now, old.version(), items));
    assert_eq!(words(&set, &now), ["alpha", "beta"]);
}

#[test]
fn typing_at_either_end_does_not_grow() {
    let mut buffer = Buffer::new("a word here\n");
    let base = buffer.snapshot();
    let mut set = DecorationSet::new();
    let range = word(&base.text().to_string(), "word");
    assert!(set.apply(&base, base.version(), vec![deco(range.clone())]));
    edit(&mut buffer, range.end..range.end, "s");
    edit(&mut buffer, range.start..range.start, "x");
    assert_eq!(words(&set, &buffer.snapshot()), ["word"]);
    set.refresh(&buffer.snapshot());
    let range = word(&buffer.text().to_string(), "word");
    edit(&mut buffer, range.end..range.end, "y");
    edit(&mut buffer, range.start..range.start, "z");
    assert_eq!(words(&set, &buffer.snapshot()), ["word"]);
}

#[test]
fn stale_or_future_versions_change_nothing() {
    let mut buffer = Buffer::new("one two three\n");
    let v0 = buffer.snapshot();
    edit(&mut buffer, 0..0, "x");
    let v1 = buffer.snapshot();
    let mut set = DecorationSet::new();
    assert!(set.apply(&v1, v1.version(), vec![deco(1..4)]));

    // Older than the last applied.
    assert!(!set.apply(&v1, v0.version(), vec![deco(0..1)]));
    assert!(!set.splice(&v1, v0.version(), 0..14, vec![deco(0..1)]));
    assert_eq!(ranges(&set, &v1), [1..4]);

    // Newer than the snapshot.
    edit(&mut buffer, 0..0, "y");
    let v2 = buffer.snapshot();
    assert!(!set.apply(&v1, v2.version(), vec![deco(0..1)]));
    assert!(!set.splice(&v1, v2.version(), 0..14, vec![deco(0..1)]));
    assert_eq!(ranges(&set, &v2), [2..5]);

    // Below the snapshot's edit-log floor.
    set.refresh(&v2);
    buffer.compact_log(v2.version());
    let compacted = buffer.snapshot();
    let mut fresh = DecorationSet::new();
    assert!(!fresh.apply(&compacted, v1.version(), vec![deco(0..1)]));
    assert!(!fresh.splice(&compacted, v1.version(), 0..1, vec![deco(0..1)]));
    assert!(fresh.is_empty());
    assert_eq!(ranges(&set, &compacted), [2..5]);

    // The last version applied is still fine.
    assert!(set.splice(&v2, v1.version(), 1..1, vec![deco(1..1)]));
    assert_eq!(ranges(&set, &v2), [2..2, 2..5]);
}

#[test]
fn a_snapshot_older_than_the_anchors_sees_nothing() {
    let mut buffer = Buffer::new("one two\n");
    let old = buffer.snapshot();
    edit(&mut buffer, 0..0, "zero ");
    let now = buffer.snapshot();
    let mut set = DecorationSet::new();
    assert!(set.apply(&now, now.version(), vec![deco(5..8)]));
    assert!(query(&set, &old, 0..old.len()).is_empty());
    set.refresh(&old);
    assert_eq!(ranges(&set, &now), [5..8]);
    // A splice can't rebase what it keeps back onto it either.
    assert!(!set.splice(&old, old.version(), 0..1, vec![deco(0..1)]));
    assert_eq!(ranges(&set, &now), [5..8]);
}

#[test]
fn compacted_past_is_dropped_on_refresh() {
    let mut buffer = Buffer::new("one two\n");
    let old = buffer.snapshot();
    let mut set = DecorationSet::new();
    assert!(set.apply(&old, old.version(), vec![deco(0..3)]));
    edit(&mut buffer, 0..0, "zero ");
    let now = buffer.snapshot();
    buffer.compact_log(now.version());
    let compacted = buffer.snapshot();
    assert!(ranges(&set, &compacted).is_empty());
    set.refresh(&compacted);
    assert!(set.is_empty());
}

#[test]
fn splice_over_anchors_compacted_past_changes_nothing() {
    let mut buffer = Buffer::new("one two\n");
    let old = buffer.snapshot();
    let mut set = DecorationSet::new();
    assert!(set.apply(&old, old.version(), vec![deco(0..3), deco(4..7)]));
    edit(&mut buffer, 0..0, "zero ");
    let now = buffer.snapshot();
    buffer.compact_log(now.version());
    let compacted = buffer.snapshot();
    assert!(!set.splice(&compacted, compacted.version(), 0..4, vec![deco(0..4)]));
    assert_eq!(set.len(), 2);
    // Not compacted in this snapshot's log, the anchors still resolve.
    assert_eq!(ranges(&set, &now), [5..8, 9..12]);
    // A whole apply still lands.
    assert!(set.apply(&compacted, compacted.version(), vec![deco(0..4)]));
    assert_eq!(ranges(&set, &compacted), [0..4]);
}

#[test]
fn query_returns_exactly_what_overlaps() {
    let buffer = Buffer::new(&"x".repeat(100));
    let snapshot = buffer.snapshot();
    let mut set = DecorationSet::new();
    let items = [
        0..50,
        10..12,
        20..20,
        30..40,
        39..45,
        40..41,
        60..60,
        70..90,
        95..100,
    ];
    let items: Vec<Decoration> = items.into_iter().map(deco).collect();
    assert!(set.apply(&snapshot, snapshot.version(), items.clone()));
    for range in [
        30..40,
        40..60,
        20..20,
        0..100,
        41..41,
        45..59,
        91..95,
        0..0,
        100..100,
    ] {
        let expected: Vec<Range<usize>> = (items.iter())
            .filter(|item| overlaps(&item.range, &range))
            .map(|item| item.range.clone())
            .collect();
        assert_eq!(query(&set, &snapshot, range.clone()), expected, "{range:?}");
    }
    // One starting before the range and spanning it is in.
    assert_eq!(query(&set, &snapshot, 46..47), [0..50]);
    // Empty ones at either end are in; ones only touching an end aren't.
    assert_eq!(query(&set, &snapshot, 60..70), [60..60]);
    assert_eq!(query(&set, &snapshot, 20..30), [0..50, 20..20]);
}

#[test]
fn a_query_examines_only_entries_near_the_range() {
    const ROWS: usize = 100_000;
    let text = "row text\n".repeat(ROWS);
    let mut buffer = Buffer::new(&text);
    let snapshot = buffer.snapshot();
    let items = (0..ROWS).map(|row| {
        let start = row * 9;
        Decoration::new(start..start + 8, DecorationKind::RowBackground, Tint::Added)
    });
    let mut set = DecorationSet::new();
    assert!(set.apply(&snapshot, snapshot.version(), items.collect()));

    let rows = |set: &DecorationSet, snapshot: &BufferSnapshot, first: usize| {
        let range =
            snapshot.text().row_start(first as u32)..snapshot.text().row_start(first as u32 + 50);
        EXAMINED.with(|count| count.set(0));
        let found = query(set, snapshot, range);
        (found.len(), EXAMINED.with(Cell::get))
    };
    let (found, examined) = rows(&set, &snapshot, 50_000);
    assert_eq!(found, 50);
    assert!(examined <= 100, "examined {examined}");

    // Still so with edits since the anchors' version.
    edit(&mut buffer, 0..0, "new\n");
    let (found, examined) = rows(&set, &buffer.snapshot(), 70_000);
    assert_eq!(found, 50);
    assert!(examined <= 100, "examined {examined}");
}

#[test]
fn splice_replaces_only_inside_its_range() {
    let mut buffer = Buffer::new("aa bb cc dd ee\n");
    let v0 = buffer.snapshot();
    let text = v0.text().to_string();
    let mut set = DecorationSet::new();
    let items = ["aa", "bb", "cc", "dd", "ee"].map(|w| deco(word(&text, w)));
    assert!(set.apply(&v0, v0.version(), items.to_vec()));

    // A splice computed at v0 lands after an edit, rebased from v0.
    edit(&mut buffer, 0..0, "__ ");
    let now = buffer.snapshot();
    let within = word(&text, "bb").start..word(&text, "dd").end;
    let new = vec![
        deco(word(&text, "cc")).with_priority(1),
        deco(word(&text, "bb").start..word(&text, "cc").end),
    ];
    assert!(set.splice(&now, v0.version(), within, new));
    assert_eq!(words(&set, &now), ["aa", "bb cc", "cc", "ee"]);
    let mut out = Vec::new();
    set.decorations(&now, word(&now.text().to_string(), "cc"), &mut out);
    assert_eq!(
        out.iter().map(|item| item.priority).collect::<Vec<_>>(),
        [0, 1]
    );

    // One crossing the splice's edge stays.
    let v1 = buffer.snapshot();
    let text = v1.text().to_string();
    let within = word(&text, "bb").start + 1..word(&text, "ee").end;
    assert!(set.splice(&v1, v1.version(), within, Vec::new()));
    assert_eq!(words(&set, &v1), ["aa", "bb cc"]);
    set.clear();
    assert!(set.is_empty());
}

#[test]
fn covered_rows_skip_an_end_at_a_row_start() {
    let buffer = Buffer::new("ab\ncd\nef\n");
    let text = buffer.snapshot().text().clone();
    assert_eq!(deco(0..3).covered_rows(&text), 0..=0);
    assert_eq!(deco(0..4).covered_rows(&text), 0..=1);
    assert_eq!(deco(3..3).covered_rows(&text), 1..=1);
    assert_eq!(deco(1..7).covered_rows(&text), 0..=2);
}

/// Computes its decorations on demand and records what it was refreshed
/// to.
#[derive(Default)]
struct Computed {
    priority: i32,
    refreshed: Vec<Version>,
}

impl DecorationProvider for Computed {
    fn decorations(
        &self,
        snapshot: &BufferSnapshot,
        range: Range<usize>,
        out: &mut Vec<Decoration>,
    ) {
        let item = Decoration::new(
            0..snapshot.len(),
            DecorationKind::RowBackground,
            Tint::Muted,
        );
        if overlaps(&item.range, &range) {
            out.push(item.with_priority(self.priority));
        }
    }

    fn refresh(&mut self, snapshot: &BufferSnapshot) {
        self.refreshed.push(snapshot.version());
    }
}

fn kinds(model: &EditorModel, range: Range<usize>) -> Vec<(i32, Tint, Range<usize>)> {
    (model.decorations(range).into_iter())
        .map(|item| (item.priority, item.tint, item.range))
        .collect()
}

#[test]
fn model_merges_providers_in_paint_order() {
    let mut model = EditorModel::new(Buffer::new("one two three\n"), None);
    let snapshot = model.snapshot().clone();
    let set = Rc::new(RefCell::new(DecorationSet::new()));
    let items = vec![
        Decoration::new(8..13, DecorationKind::Highlight, Tint::Error).with_priority(1),
        Decoration::new(0..3, DecorationKind::Highlight, Tint::Accent),
        Decoration::new(4..7, DecorationKind::Highlight, Tint::Warning).with_priority(1),
    ];
    assert!(set.borrow_mut().apply(&snapshot, snapshot.version(), items));
    model.add_decoration_provider(set.clone());
    let computed = Rc::new(RefCell::new(Computed::default()));
    let computed_id = model.add_decoration_provider(computed.clone());
    assert_eq!(
        kinds(&model, 0..14),
        [
            (0, Tint::Accent, 0..3),
            (0, Tint::Muted, 0..14),
            (1, Tint::Warning, 4..7),
            (1, Tint::Error, 8..13),
        ]
    );
    // Only what overlaps the range.
    assert_eq!(
        kinds(&model, 4..5),
        [(0, Tint::Muted, 0..14), (1, Tint::Warning, 4..7)]
    );

    // A provider registered later paints over earlier ones at its priority.
    computed.borrow_mut().priority = 1;
    assert_eq!(
        kinds(&model, 0..14),
        [
            (0, Tint::Accent, 0..3),
            (1, Tint::Warning, 4..7),
            (1, Tint::Error, 8..13),
            (1, Tint::Muted, 0..14),
        ]
    );

    assert!(model.remove_decoration_provider(computed_id));
    assert!(!model.remove_decoration_provider(computed_id));
    assert_eq!(kinds(&model, 0..14).len(), 3);
}

#[test]
fn a_split_shares_providers_and_its_edits_refresh_them() {
    let mut model = EditorModel::new(Buffer::new("one two\n"), None);
    let snapshot = model.snapshot().clone();
    let set = Rc::new(RefCell::new(DecorationSet::new()));
    assert!(
        set.borrow_mut()
            .apply(&snapshot, snapshot.version(), vec![deco(4..7)])
    );
    model.add_decoration_provider(set.clone());
    let computed = Rc::new(RefCell::new(Computed::default()));
    let computed_id = model.add_decoration_provider(computed.clone());

    let mut split = model.split();
    assert_eq!(kinds(&split, 0..8), kinds(&model, 0..8));

    // An edit through the split reaches the providers when the original
    // catches up.
    split.set_cursor(0);
    split.insert("zero ");
    split.sync_display();
    let version = split.snapshot().version();
    model.sync_display();
    assert_eq!(model.snapshot().version(), version);
    assert_eq!(computed.borrow().refreshed.last(), Some(&version));
    assert_eq!(model.decorations(0..13)[0].range, 9..12);
    assert_eq!(split.decorations(0..13)[0].range, 9..12);

    // One added after the split stays with the model it was added to;
    // removing a shared one from one leaves it in the other.
    let later = Rc::new(RefCell::new(Computed::default()));
    model.add_decoration_provider(later);
    assert_eq!(model.decorations(0..13).len(), 3);
    assert_eq!(split.decorations(0..13).len(), 2);
    assert!(split.remove_decoration_provider(computed_id));
    assert_eq!(split.decorations(0..13).len(), 1);
    assert_eq!(model.decorations(0..13).len(), 3);
}

#[test]
fn a_split_behind_the_shared_set_sees_nothing_until_it_catches_up() {
    let mut model = EditorModel::new(Buffer::new("one two\n"), None);
    let set = Rc::new(RefCell::new(DecorationSet::new()));
    model.add_decoration_provider(set.clone());
    let mut split = model.split();
    split.set_cursor(0);
    split.insert("zero ");
    split.sync_display();
    let now = split.snapshot().clone();
    assert!(
        set.borrow_mut()
            .apply(&now, now.version(), vec![deco(9..12)])
    );
    // `model` hasn't caught up: its snapshot is older than the anchors.
    assert!(model.decorations(0..8).is_empty());
    model.sync_display();
    assert_eq!(model.decorations(0..13)[0].range, 9..12);
}
