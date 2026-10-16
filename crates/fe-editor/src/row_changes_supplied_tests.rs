//! Row changes a model owns apart from its splits, and supplied by the
//! host rather than diffed: a diff view's side against the other side,
//! while the file's editors compare against git.

use fe_buffer::Buffer;

use super::RowChangeKind::*;
use super::*;
use crate::EditorModel;

fn changes(model: &EditorModel) -> Vec<(Range<u32>, RowChangeKind)> {
    model
        .row_changes()
        .iter()
        .map(|c| (c.rows.clone(), c.kind))
        .collect()
}

fn supplied(rows: Range<u32>, kind: RowChangeKind) -> Arc<[RowChange]> {
    Arc::new([RowChange { rows, kind }])
}

/// A file's model, its base `HEAD`'s text: row 1 changed.
fn file() -> EditorModel {
    let mut model = EditorModel::new(Buffer::new("a\nB\nc\n"), None);
    model.set_diff_base(Some("a\nb\nc\n".into()));
    model
}

#[test]
fn own_state() {
    let file = file();
    let earlier = file.split();
    let mut side = file.split();
    side.own_row_changes();
    // Its own state has no base yet.
    assert_eq!(changes(&side), []);
    assert!(side.decorations(0..side.text().len()).is_empty());

    let version = side.snapshot().version();
    side.supply_row_changes("x\n".into(), version, supplied(0..3, Modified));
    assert_eq!(changes(&side), [(0..3, Modified)]);
    assert!(!side.decorations(0..side.text().len()).is_empty());
    // The file's models keep `HEAD`.
    for model in [&file, &earlier] {
        assert_eq!(changes(model), [(1..2, Modified)]);
        assert_eq!(model.row_changes.base().as_deref(), Some("a\nb\nc\n"));
    }

    // A later split of the side shares what was supplied, and what's
    // supplied to it.
    let mut later = side.split();
    assert_eq!(changes(&later), [(0..3, Modified)]);
    later.supply_row_changes("y\n".into(), version, supplied(2..3, Added));
    assert_eq!(changes(&side), [(2..3, Added)]);
    assert_eq!(changes(&file), [(1..2, Modified)]);
}

#[test]
fn never_diffs() {
    let mut side = file().split();
    side.own_row_changes();
    let base: Arc<str> = "a\nc\n".into();
    let version = side.snapshot().version();
    side.supply_row_changes(base.clone(), version, supplied(1..2, Added));
    let before = diff_count();

    // An edit shows the last changes supplied until its own arrive.
    side.set_cursor(0);
    side.insert("new\n");
    side.sync_display();
    let edited = side.snapshot().version();
    assert_ne!(edited, version);
    assert_eq!(changes(&side), [(1..2, Added)]);
    side.supply_row_changes(base.clone(), edited, Arc::new([]));
    side.supply_row_changes(base, edited, supplied(0..1, Added));
    assert_eq!(changes(&side), [(0..1, Added)]);
    assert!(!side.decorations(0..side.text().len()).is_empty());
    assert_eq!(diff_count(), before);

    // Reverting puts the supplied base's rows back: "B" goes, "new" goes.
    assert_eq!(side.revert_changes(0..side.text().row_count()), 2);
    assert_eq!(side.text().to_string(), "a\nc\n");
    assert_eq!(diff_count(), before);
    // The revert's version isn't supplied: still the last ones supplied.
    assert_eq!(changes(&side), [(0..1, Added)]);

    // A base set by hand is diffed again, even the same text.
    side.set_diff_base(Some("a\nc\n".into()));
    assert_eq!(changes(&side), []);
    assert_eq!(diff_count(), before + 1);
    side.set_diff_base(Some("a\nb\nc\n".into()));
    assert_eq!(changes(&side), [(1..1, Deleted)]);
}
