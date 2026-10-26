use super::*;
use crate::tests::parse;

/// A model of `rows` rows, with its history empty and a caret at the start.
fn model(rows: usize) -> EditorModel {
    let text: String = (0..rows).map(|row| format!("row {row}\n")).collect();
    let mut model = parse(&text);
    model.cursor_history = CursorHistory::default();
    model
}

fn row(model: &EditorModel) -> u32 {
    let head = model.primary_selection().head();
    model.text().offset_to_point(head).row
}

fn go_to_row(model: &mut EditorModel, row: u32) {
    let offset = model.text().row_start(row);
    model.set_cursor(offset);
}

#[test]
fn click_then_undo() {
    let mut model = model(30);
    go_to_row(&mut model, 1);
    go_to_row(&mut model, 8);
    go_to_row(&mut model, 20);
    let text = model.text().to_string();
    let dirty = model.is_dirty();

    model.cursor_undo();
    assert_eq!(row(&model), 8);
    model.cursor_undo();
    assert_eq!(row(&model), 1);
    model.cursor_redo();
    assert_eq!(row(&model), 8);
    model.cursor_redo();
    assert_eq!(row(&model), 20);
    assert_eq!(model.text().to_string(), text);
    assert_eq!(model.is_dirty(), dirty);
}

#[test]
fn typing_not_recorded() {
    let mut model = model(30);
    go_to_row(&mut model, 3);
    let before = model.cursor_history.undo.len();
    for ch in "abcdefghij".chars() {
        model.insert_typed(ch, &[]);
    }
    assert_eq!(model.cursor_history.undo.len(), before);
    model.select_all();
    assert_eq!(model.cursor_history.undo.len(), before + 1);
    go_to_row(&mut model, 5);
    let before = model.cursor_history.undo.len();
    model.add_caret_below();
    assert_eq!(model.cursor_history.undo.len(), before + 1);
}

#[test]
fn redo_cleared_and_clamped() {
    let mut model = model(30);
    go_to_row(&mut model, 10);
    go_to_row(&mut model, 25);
    model.cursor_undo();
    assert_eq!(row(&model), 10);
    go_to_row(&mut model, 15);
    model.cursor_redo();
    assert_eq!(row(&model), 15, "a new change cleared redo");

    // An entry past the end of a text the edits shortened is clamped.
    let mut model = self::model(30);
    go_to_row(&mut model, 28);
    go_to_row(&mut model, 0);
    let start = model.text().row_start(5);
    let end = model.text().len();
    model.select_range(start..end, false);
    model.insert("");
    model.set_cursor(0);
    // Only the entries from before the edit.
    model.cursor_history.undo.truncate(2);
    let len = model.text().len();
    assert_eq!(len, start);
    model.cursor_undo();
    assert_eq!(model.primary_selection().range, len..len);

    let mut model = self::model(200);
    for row in 1..=120 {
        go_to_row(&mut model, row);
    }
    assert_eq!(model.cursor_history.undo.len(), MAX_ENTRIES);
}

#[test]
fn select_all_in_the_same_row_is_recorded() {
    let mut model = parse("one\ntwo|");
    model.cursor_history = CursorHistory::default();
    go_to_row(&mut model, 0);
    go_to_row(&mut model, 1);
    let before = model.cursor_history.undo.len();
    model.select_all();
    assert_eq!(model.cursor_history.undo.len(), before + 1);
}

#[test]
fn selecting_commands_are_recorded_in_a_single_row() {
    let mut model = parse("on|e two");
    model.cursor_history = CursorHistory::default();
    model.select_all();
    assert_eq!(model.cursor_history.undo.len(), 1);
    model.cursor_undo();
    assert_eq!(model.primary_selection().range, 2..2);

    // A selection grown from a fixed tail is its own step too.
    let mut model = parse("[o]ne two");
    model.cursor_history = CursorHistory::default();
    model.select_range(0..3, false);
    assert_eq!(model.cursor_history.undo.len(), 1);
    model.cursor_undo();
    assert_eq!(model.primary_selection().range, 0..1);

    // Keyboard moves along a row are one step.
    let mut model = parse("|one two");
    model.cursor_history = CursorHistory::default();
    go_to_row(&mut model, 0);
    for _ in 0..3 {
        model.move_cursors(crate::Movement::Right, false);
    }
    assert_eq!(model.cursor_history.undo.len(), 1, "only the first move");
}
