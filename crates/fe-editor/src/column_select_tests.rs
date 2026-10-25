use super::*;
use crate::tests::{parse, render};

fn run(model: &mut EditorModel, directions: &[Direction]) {
    for &direction in directions {
        model.column_select(direction);
    }
}

#[test]
fn down_grows_a_box() {
    let mut model = parse("abcdef\nabc|def\nabcdef\nabcdef\nabcdef");
    run(&mut model, &[Direction::Down, Direction::Down]);
    assert_eq!(render(&model), "abcdef\nabc|def\nabc|def\nabc|def\nabcdef");
    model.column_select(Direction::Right);
    let mut expected = parse("abcdef\nabcdef\nabcdef\nabcdef\nabcdef");
    expected.select_box((1, 3), (3, 4));
    assert_eq!(model.selections(), expected.selections());
    assert_eq!(
        render(&model),
        "abcdef\nabc[d]ef\nabc[d]ef\nabc[d]ef\nabcdef"
    );
}

#[test]
fn short_rows() {
    let mut model = parse("abcdef\nab\nabc|def");
    run(&mut model, &[Direction::Up, Direction::Up]);
    // The short row gets no selection; the box keeps its columns.
    assert_eq!(render(&model), "abc|def\nab\nabc|def");
    model.column_select(Direction::Right);
    assert_eq!(render(&model), "abc[d]ef\nab\nabc[d]ef");
}

#[test]
fn box_ends_on_other_moves() {
    let mut model = parse("abcdef\nabcdef\nabc|def\nabcdef");
    model.column_select(Direction::Up);
    model.move_cursors(crate::Movement::Left, false);
    assert_eq!(render(&model), "abcdef\nab|cdef\nab|cdef\nabcdef");
    // The next box starts at the new primary head, not the old anchor.
    model.column_select(Direction::Down);
    assert_eq!(render(&model), "abcdef\nabcdef\nab|cdef\nab|cdef");

    model.column_select(Direction::Down);
    model.set_cursor(1);
    model.column_select(Direction::Up);
    assert_eq!(render(&model), "a|bcdef\nabcdef\nabcdef\nabcdef");
}

#[test]
fn left_stops_at_the_first_column() {
    let mut model = parse("a|b\nab");
    run(
        &mut model,
        &[Direction::Left, Direction::Left, Direction::Down],
    );
    assert_eq!(render(&model), "[a]b\n[a]b");
}

#[test]
fn box_ends_on_edits_and_other_selection_paths() {
    // Typing at the carets leaves selections elsewhere than the box made.
    let mut model = parse("abcdef\nabc|def\nabcdef");
    model.column_select(Direction::Down);
    model.insert("X");
    model.column_select(Direction::Down);
    assert_eq!(render(&model), "abcdef\nabcXdef\nabcX|def");

    // Undo puts the text back; the box does not come back with it.
    let mut model = parse("abcdef\nabc|def\nabcdef");
    model.column_select(Direction::Down);
    model.insert("X");
    model.undo().unwrap();
    model.set_cursor(1);
    model.column_select(Direction::Down);
    assert_eq!(render(&model), "a|bcdef\na|bcdef\nabcdef");
}
