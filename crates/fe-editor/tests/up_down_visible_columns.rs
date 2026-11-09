//! Up/down keep the visible column (a wide char counts two) and land only on
//! grapheme boundaries.

use std::sync::Arc;

use fe_buffer::{Buffer, ResolvedSelection, Text};
use fe_editor::{EditorModel, Movement, ViewerModel};

fn editor(text: &str) -> EditorModel {
    EditorModel::new(Buffer::new(text), None)
}

fn editor_at(model: &mut EditorModel, offset: usize) {
    model.set_selections(vec![ResolvedSelection {
        range: offset..offset,
        reversed: false,
    }]);
}

fn editor_move(model: &mut EditorModel, movement: Movement) -> usize {
    model.move_cursors(movement, false);
    model.primary_selection().head()
}

fn viewer(text: &str) -> ViewerModel {
    ViewerModel::new(Arc::new(Text::new(text)), 4)
}

fn viewer_move(model: &mut ViewerModel, movement: Movement) -> usize {
    model.move_cursor(movement, false);
    model.selection().end
}

const WIDE: &str = "abcdefghij\n一二三四五六七八\nabcdefghij";
const CLUSTERS: &str = "abcdef\ne\u{301}xyz\n👨\u{200d}👩\u{200d}👧xyz";

#[test]
fn down_keeps_the_visible_column_over_wide_chars() {
    let mut model = editor(WIDE);
    editor_at(&mut model, 6);
    assert_eq!(editor_move(&mut model, Movement::Down), 20);
    assert_eq!(editor_move(&mut model, Movement::Down), 42);
    assert_eq!(editor_move(&mut model, Movement::Up), 20);
    assert_eq!(editor_move(&mut model, Movement::Up), 6);
}

#[test]
fn down_lands_on_cluster_boundaries() {
    let mut model = editor(CLUSTERS);
    editor_at(&mut model, 1);
    assert_eq!(editor_move(&mut model, Movement::Down), 10);
    let mut model = editor(CLUSTERS);
    editor_at(&mut model, 3);
    assert_eq!(editor_move(&mut model, Movement::Down), 12);
    assert_eq!(editor_move(&mut model, Movement::Down), 33);
}

#[test]
fn viewer_down_keeps_the_visible_column_and_cluster_boundaries() {
    let mut model = viewer(WIDE);
    model.set_selection(6, 6);
    assert_eq!(viewer_move(&mut model, Movement::Down), 20);
    assert_eq!(viewer_move(&mut model, Movement::Down), 42);
    assert_eq!(viewer_move(&mut model, Movement::Up), 20);
    assert_eq!(viewer_move(&mut model, Movement::Up), 6);

    let mut model = viewer(CLUSTERS);
    model.set_selection(1, 1);
    assert_eq!(viewer_move(&mut model, Movement::Down), 10);
    let mut model = viewer(CLUSTERS);
    model.set_selection(3, 3);
    assert_eq!(viewer_move(&mut model, Movement::Down), 12);
    assert_eq!(viewer_move(&mut model, Movement::Down), 33);
}
