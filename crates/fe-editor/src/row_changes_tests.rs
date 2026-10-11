use std::cell::Cell;

use fe_buffer::Buffer;

use super::RowChangeKind::*;
use super::*;
use crate::EditorModel;

thread_local! {
    /// Diffs run on this thread.
    pub(super) static DIFFS: Cell<usize> = const { Cell::new(0) };
}

fn model(text: &str, base: Option<&str>) -> EditorModel {
    let mut model = EditorModel::new(Buffer::new(text), None);
    model.set_diff_base(base.map(Into::into));
    model
}

fn changes(model: &EditorModel) -> Vec<(Range<u32>, RowChangeKind)> {
    model
        .row_changes()
        .iter()
        .map(|c| (c.rows.clone(), c.kind))
        .collect()
}

#[test]
fn added_modified_and_deleted_rows() {
    let base = "a\nb\nc\n";
    assert_eq!(changes(&model("a\nb\nx\nc\n", Some(base))), [(2..3, Added)]);
    assert_eq!(changes(&model("a\nx\nc\n", Some(base))), [(1..2, Modified)]);
    assert_eq!(changes(&model("a\nc\n", Some(base))), [(1..1, Deleted)]);
    assert_eq!(changes(&model("a\nb\n", Some(base))), [(2..2, Deleted)]);
}

#[test]
fn no_base_or_equal_base_is_no_changes() {
    assert!(model("a\nb\n", None).row_changes().is_empty());
    assert!(model("a\nb\n", Some("a\nb\n")).row_changes().is_empty());
}

#[test]
fn line_endings_are_not_changes() {
    assert!(model("a\nb\n", Some("a\r\nb\r\n")).row_changes().is_empty());
    let crlf = EditorModel::new(Buffer::from_file_contents("a\r\nb\r\n"), None);
    let mut crlf = crlf;
    crlf.set_diff_base(Some("a\nb\n".into()));
    assert!(crlf.row_changes().is_empty());
}

#[test]
fn follows_edits_and_diffs_once_per_version() {
    let mut model = model("a\nb\n", Some("a\nb\n"));
    let before = DIFFS.with(Cell::get);
    assert!(model.row_changes().is_empty());
    model.set_cursor(2);
    model.insert("x\n");
    model.sync_display();
    for _ in 0..3 {
        assert_eq!(changes(&model), [(1..2, Added)]);
    }
    assert_eq!(DIFFS.with(Cell::get) - before, 2);
    model.set_diff_base(Some("a\nb\n".into()));
    changes(&model);
    assert_eq!(DIFFS.with(Cell::get) - before, 2);
    model.set_diff_base(None);
    assert!(model.row_changes().is_empty());
}

#[test]
fn over_five_mib_is_no_changes() {
    let big = "x\n".repeat(MAX_DIFF_BYTES / 2 + 1);
    assert!(model("y\n", Some(&big)).row_changes().is_empty());
    assert!(model(&big, Some("y\n")).row_changes().is_empty());
}
