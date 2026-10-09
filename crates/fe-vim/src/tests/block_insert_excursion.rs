//! A block session's text, typed by `Esc` on a row the session typed on
//! during a cursor excursion (T174 F26). Every expected text and cursor is
//! nvim 0.12.4's; the keys are in `docs/decisions/T206-block-insert-excursion.md`.

use fe_buffer::Origin;

use crate::{HeadlessVim, Splits};

const ROWS: &str = "abcd\nabcd\nabcd";

/// Feeds `keys`, running `<Down>` and `<Up>` as the host runs its cursor keys.
fn feed_moving(vim: &mut HeadlessVim, mut keys: &str) {
    use fe_editor::Movement;
    let moves = [("<Down>", Movement::Down), ("<Up>", Movement::Up)];
    while let Some((at, name, movement)) = moves
        .iter()
        .filter_map(|&(name, movement)| Some((keys.find(name)?, name, movement)))
        .min_by_key(|&(at, ..)| at)
    {
        vim.feed(&keys[..at]).unwrap();
        vim.split_session(Splits::Moves, |model| model.move_cursors(movement, false));
        keys = &keys[at + name.len()..];
    }
    vim.feed(keys).unwrap();
}

/// Types `keys` over `text` from `(0, column)` and checks the text and cursor,
/// then that undoing back to the start restores `text` and redoing the result.
fn check(text: &str, column: usize, keys: &str, want: &str, cursor: (u32, usize)) {
    let mut vim = HeadlessVim::new(text, (0, column));
    feed_moving(&mut vim, keys);
    assert_eq!(
        (vim.text().as_str(), vim.cursor()),
        (want, cursor),
        "{keys}"
    );
    for _ in 0..8 {
        vim.feed("u").unwrap();
    }
    assert_eq!(vim.text(), text, "undone: {keys}");
    for _ in 0..8 {
        vim.feed("<C-r>").unwrap();
    }
    assert_eq!(vim.text(), want, "redone: {keys}");
}

#[test]
fn change_types_on_a_row_the_session_typed_on() {
    check(
        ROWS,
        1,
        "<C-v>jjlcXY<Down>Z<Up>Q<Esc>",
        "aXYQd\naXYQdZ\naXYQd",
        (0, 3),
    );
}

#[test]
fn insert_types_on_a_row_the_session_typed_on() {
    check(
        ROWS,
        1,
        "<C-v>jjIXY<Down>Z<Up><Esc>",
        "aXYbcd\naXYbcZd\naXYbcd",
        (0, 1),
    );
}

#[test]
fn append_types_on_a_row_the_session_typed_on() {
    check(
        ROWS,
        1,
        "<C-v>jjAXY<Down>Z<Up><Esc>",
        "abXYcd\nabXYcdZ\nabXYcd",
        (0, 1),
    );
}

#[test]
fn append_types_on_an_empty_row_the_session_typed_on() {
    check(
        "abcd\n\nabcd",
        1,
        "<C-v>jjAXY<Down>Z<Up><Esc>",
        "abXYcd\nZ XY\nabXYcd",
        (0, 1),
    );
}

#[test]
fn another_origins_edit_still_costs_a_row_its_text() {
    let ai = Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    // The rows are "aXYQd", "adZ" and "ad" (at 0, 6 and 10) when another
    // origin edits the second: on its text, across its line breaks. The first
    // and the last row (if it's not joined to the second) get the text; the
    // second gets none.
    for (edit, rows) in [
        ((7..7, "!"), 2),
        ((9..9, "!"), 2),
        ((5..6, ""), 2),
        ((9..10, ""), 1),
    ] {
        let mut vim = HeadlessVim::new(ROWS, (0, 1));
        feed_moving(&mut vim, "<C-v>jjlcXY<Down>Z<Up>Q");
        vim.model()
            .split()
            .buffer_mut()
            .edit([edit.clone()], ai.clone());
        vim.feed("<Esc>").unwrap();
        let text = vim.text();
        assert_eq!(text.matches("XYQ").count(), rows, "{edit:?}: {text:?}");
    }
}

#[test]
fn append_to_the_end_types_at_the_end_of_a_row_the_session_typed_on() {
    check(
        ROWS,
        1,
        "<C-v>jj$AXY<Down>Z<Up><Esc>",
        "abcdXY\nabcdZXY\nabcdXY",
        (0, 1),
    );
}

#[test]
fn rows_put_in_or_taken_out_above_a_row_it_typed_on_leave_it_its_text() {
    let ai = Origin::Ai {
        agent: "agent".into(),
        session: "session".into(),
    };
    let mut vim = HeadlessVim::new(ROWS, (0, 1));
    feed_moving(&mut vim, "<C-v>jjlcXY<Down>Z<Up>Q");
    let mut model = vim.model().split();
    model.buffer_mut().edit([(6..6, "new\n")], ai.clone());
    model.buffer_mut().edit([(6..10, "")], ai.clone());
    vim.feed("<Esc>").unwrap();
    let text = vim.text();
    assert_eq!(text.matches("XYQ").count(), 3, "{text:?}");
}
