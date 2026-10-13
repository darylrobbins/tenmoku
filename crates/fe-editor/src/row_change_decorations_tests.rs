use fe_buffer::Buffer;

use crate::EditorModel;
use crate::decorations::{Decoration, DecorationKind::*, GutterMark::*, Tint};

fn model(text: &str, base: Option<&str>) -> EditorModel {
    let mut model = EditorModel::new(Buffer::new(text), None);
    model.set_diff_base(base.map(Into::into));
    model
}

fn all(model: &EditorModel) -> Vec<Decoration> {
    model.decorations(0..model.text().len())
}

fn summary(items: &[Decoration]) -> Vec<(std::ops::Range<usize>, Tint, bool)> {
    items
        .iter()
        .map(|d| (d.range.clone(), d.tint, matches!(d.kind, RowBackground)))
        .collect()
}

#[test]
fn added_modified_and_deleted_rows() {
    let base = "a\nb\nc\n";
    let added = all(&model("a\nb\nx\nc\n", Some(base)));
    assert_eq!(
        added
            .iter()
            .map(|d| (&d.kind, d.tint, d.range.clone(), d.priority))
            .collect::<Vec<_>>(),
        [(&Gutter(Bar), Tint::Added, 4..6, -100)]
    );
    let modified = all(&model("a\nx\nc\n", Some(base)));
    assert_eq!(summary(&modified), [(2..4, Tint::Modified, false)]);
    assert_eq!(modified[0].kind, Gutter(Bar));
    let deleted = all(&model("a\nc\n", Some(base)));
    assert_eq!(deleted.len(), 1);
    assert_eq!(deleted[0].kind, Gutter(Deletion));
    assert_eq!(
        (deleted[0].tint, deleted[0].range.clone()),
        (Tint::Removed, 2..2)
    );
    let last = all(&model("a\nb\n", Some(base)));
    assert_eq!(last.len(), 1);
    assert_eq!(last[0].range, 4..4);
}

#[test]
fn queries_return_only_overlapping_changes() {
    let base = "a\nb\nc\nd\ne\n";
    let model = model("a\nB\nc\nd\nE\n", Some(base));
    assert_eq!(model.decorations(0..1).len(), 0);
    assert_eq!(summary(&model.decorations(2..3))[0].0, 2..4);
    assert_eq!(model.decorations(2..3).len(), 1);
    assert_eq!(model.decorations(4..6).len(), 0);
    assert_eq!(model.decorations(8..9).len(), 1);
}

#[test]
fn follows_edits_and_diffs_once_per_version() {
    let mut model = model("a\nb\n", Some("a\nb\n"));
    assert!(all(&model).is_empty());
    model.set_cursor(2);
    model.insert("x\n");
    model.sync_display();
    let before = crate::row_changes::diff_count();
    for _ in 0..3 {
        assert_eq!(all(&model).len(), 1);
        assert_eq!(model.row_changes().len(), 1);
    }
    assert_eq!(crate::row_changes::diff_count() - before, 1);
}

#[test]
fn provider_follows_the_base() {
    let mut model = model("a\nx\n", Some("a\nb\n"));
    assert_eq!(all(&model).len(), 1);
    model.set_diff_base(Some("a\nb\n".into()));
    model.set_diff_base(Some("a\nx\n".into()));
    model.set_diff_base(Some("a\ny\n".into()));
    assert_eq!(all(&model).len(), 1);
    model.set_diff_base(None);
    assert!(all(&model).is_empty());
    model.set_diff_base(Some("a\nb\n".into()));
    assert_eq!(all(&model).len(), 1);
}

#[test]
fn splits_share_the_base_and_provider() {
    let mut model = model("a\nx\n", Some("a\nb\n"));
    let mut split = model.split();
    assert_eq!(all(&split).len(), 1);
    split.set_diff_base(Some("a\nx\n".into()));
    assert!(all(&split).is_empty());
    assert!(all(&model).is_empty());
    model.set_diff_base(Some("a\nb\n".into()));
    assert_eq!(all(&split).len(), 1);
}

#[test]
fn over_five_mib_or_no_base_is_no_decorations() {
    assert!(all(&model("a\n", None)).is_empty());
    let big = "x\n".repeat(crate::row_changes::MAX_DIFF_BYTES / 2 + 1);
    assert!(all(&model("y\n", Some(&big))).is_empty());
    assert!(all(&model(&big, Some("y\n"))).is_empty());
}

#[test]
fn end_of_text_changes_are_found() {
    let deleted = model("a", Some("a\nb"));
    let items = all(&deleted);
    assert_eq!(items.len(), 1);
    assert_eq!(
        (items[0].kind.clone(), items[0].range.clone()),
        (Gutter(Deletion), 1..1)
    );
    assert_eq!(deleted.decorations(1..1).len(), 1);
    let added = model("a\n", Some("a"));
    let items = all(&added);
    assert_eq!(items.len(), 1);
    assert!(
        items
            .iter()
            .all(|d| d.range == (2..2) && d.tint == Tint::Added)
    );
}

#[test]
fn a_provider_is_registered_whichever_split_sets_the_base() {
    let mut model = EditorModel::new(Buffer::new("a\nx\n"), None);
    let mut split = model.split();
    split.set_diff_base(Some("a\nb\n".into()));
    assert_eq!(all(&split).len(), 1);
    assert_eq!(all(&model).len(), 1);
    model.set_diff_base(None);
    assert!(all(&model).is_empty());
    assert!(all(&split).is_empty());
    split.sync_display();
    assert!(all(&split).is_empty());
    split.set_diff_base(Some("a\nb\n".into()));
    assert_eq!(all(&split).len(), 1);
    assert_eq!(all(&model).len(), 1);
    model.set_diff_base(Some("a\nb\n".into()));
    assert_eq!(all(&model).len(), 1);
    model.sync_display();
    split.sync_display();
    assert_eq!(all(&model).len(), 1);
    assert_eq!(all(&split).len(), 1);
}

#[test]
fn splits_at_different_versions_diff_each_version_once() {
    let mut model = model("a\nb\n", Some("a\nb\n"));
    let mut split = model.split();
    model.set_cursor(0);
    model.insert("x\n");
    model.sync_display();
    let before = crate::row_changes::diff_count();
    for _ in 0..3 {
        assert_eq!(all(&model).len(), 1);
        assert!(all(&split).is_empty());
    }
    assert_eq!(crate::row_changes::diff_count() - before, 2);
    split.sync_display();
    assert_eq!(all(&split).len(), 1);
}

#[test]
fn a_stale_split_keeps_its_diff_across_many_edits() {
    let mut model = model("a\nb\n", Some("a\nb\n"));
    let split = model.split();
    assert!(all(&split).is_empty());
    let before = crate::row_changes::diff_count();
    for i in 0..8 {
        model.set_cursor(0);
        model.insert(&format!("{i}\n"));
        model.sync_display();
        assert!(!all(&model).is_empty());
        assert!(all(&split).is_empty());
    }
    assert_eq!(crate::row_changes::diff_count() - before, 8);
}

#[test]
fn a_base_outlives_the_split_that_set_it() {
    let model = EditorModel::new(Buffer::new("a\nx\n"), None);
    let mut split = model.split();
    split.set_diff_base(Some("a\nb\n".into()));
    drop(split);
    assert_eq!(all(&model).len(), 1);
}

#[test]
fn buffer_mut_catch_ups_count_as_live_versions() {
    use fe_buffer::Origin;
    let mut a = model("a\nb\n", Some("a\nb\n"));
    let mut b = a.split();
    a.buffer_mut().edit([(0..0, "x\n")], Origin::User).unwrap();
    assert_eq!(all(&a).len(), 1);
    let before = crate::row_changes::diff_count();
    b.buffer_mut().edit([(0..0, "y\n")], Origin::User).unwrap();
    for _ in 0..6 {
        b.buffer_mut().edit([(0..0, "z\n")], Origin::User).unwrap();
        assert!(!all(&b).is_empty());
    }
    assert!(!all(&a).is_empty());
    assert_eq!(crate::row_changes::diff_count() - before, 6);
}
