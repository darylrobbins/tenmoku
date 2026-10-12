//! `EditorModel::revert_changes`: putting the diff base's rows back.

use std::sync::Arc;

use fe_buffer::Buffer;
use fe_editor::EditorModel;

const BASE: &str = "a\nb\nc\nd\ne\nf\ng\nh\ni\n";

fn model(text: &str, base: Option<&str>) -> EditorModel {
    let mut model = EditorModel::new(Buffer::new(text), None);
    model.set_diff_base(base.map(Arc::from));
    model
}

fn text(model: &EditorModel) -> String {
    model.text().to_string()
}

fn caret_row(model: &EditorModel) -> u32 {
    let head = model.selections()[0].range.start;
    model.text().offset_to_point(head).row
}

/// Three separate changes: b -> B, e -> E, h -> H.
const THREE: &str = "a\nB\nc\nd\nE\nf\ng\nH\ni\n";

#[test]
fn reverts_hunks_in_range() {
    let mut model = model(THREE, Some(BASE));
    assert_eq!(model.row_changes().len(), 3);
    assert_eq!(model.revert_changes(4..5), 1);
    assert_eq!(text(&model), "a\nB\nc\nd\ne\nf\ng\nH\ni\n");
    assert_eq!(model.row_changes().len(), 2);
}

#[test]
fn one_undo_step() {
    let mut model = model(THREE, Some(BASE));
    assert_eq!(model.revert_changes(0..9), 3);
    assert_eq!(text(&model), BASE);
    assert_eq!(caret_row(&model), 1);
    model.undo().unwrap();
    assert_eq!(text(&model), THREE);
    model.redo().unwrap();
    assert_eq!(text(&model), BASE);

    let mut model = self::model(THREE, Some(BASE));
    assert_eq!(model.revert_changes(4..9), 2);
    assert_eq!(caret_row(&model), 4);
    model.undo().unwrap();
    assert_eq!(text(&model), THREE);
    model.redo().unwrap();
    assert_eq!(text(&model), "a\nB\nc\nd\ne\nf\ng\nh\ni\n");
}

#[test]
fn edges() {
    // c deleted: the insertion row is 2.
    let deleted = "a\nb\nd\ne\nf\ng\nh\ni\n";
    for rows in [2..3, 1..2] {
        let mut model = model(deleted, Some(BASE));
        assert_eq!(model.revert_changes(rows), 1);
        assert_eq!(text(&model), BASE);
    }
    let mut model = model(deleted, Some(BASE));
    assert_eq!(model.revert_changes(5..7), 0);
    assert_eq!(model.revert_changes(3..4), 0);
    assert_eq!(text(&model), deleted);
    assert!(model.undo().unwrap().is_none());

    let mut none = self::model(THREE, None);
    assert_eq!(none.revert_changes(0..9), 0);
    assert_eq!(text(&none), THREE);
    assert!(none.undo().unwrap().is_none());
}

#[test]
fn reordered_lines_revert_the_forward_hunk() {
    let mut model = model("z\nb\na\nq\n", Some("z\na\nb\nq\n"));
    assert_eq!(model.revert_changes(2..3), 1);
    assert_eq!(text(&model), "z\nb\nq\n");
}

#[test]
fn eof_separators() {
    for (base, current) in [
        ("a\nb", "a"),
        ("a", "a\nb"),
        ("a\n", "a"),
        ("a", "a\n"),
        ("a\nb\n", "a\nb"),
        ("a\nb", "a\nb\n"),
        ("a\nb\nc", "a\nx"),
        ("a\nb", "x"),
    ] {
        let mut model = model(current, Some(base));
        assert!(model.revert_changes(0..10) >= 1, "{base:?} {current:?}");
        assert_eq!(text(&model), base);
        model.undo().unwrap();
        assert_eq!(text(&model), current);
        model.redo().unwrap();
        assert_eq!(text(&model), base);
    }
}
