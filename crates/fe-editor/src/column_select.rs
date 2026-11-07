//! Keyboard column (box) selection, as VS Code's `cursorColumnSelect*`.
//!
//! The commands grow a box from the primary selection's head one column or
//! row at a time. The box is kept as an anchor and a head, each a row and a
//! char column of the expanded row; the head's column may lie past a row's
//! end, as VS Code's virtual space does for the box's width. Any other
//! selection change ends the box (see [`EditorModel::set_selections`]).

use fe_buffer::{ResolvedSelection, Version};
use fe_command::{Effect, Registry};

use crate::{
    EditorModel,
    commands::{Editor, add},
};

/// A box selection under way: both corners as (row, char column), with
/// the buffer version and selections it made. It only stands while the
/// model still has exactly those, whichever way they were replaced.
#[derive(Clone, Debug)]
pub(crate) struct ColumnBox {
    anchor: (u32, usize),
    head: (u32, usize),
    version: Version,
    selections: Vec<ResolvedSelection>,
}

impl ColumnBox {
    /// The box as of `version`, for a buffer whose text came back to what
    /// it was when the box was made.
    pub(crate) fn at_version(self, version: Version) -> Self {
        Self { version, ..self }
    }
}

/// Which way a command moves the box's head.
#[derive(Clone, Copy)]
enum Direction {
    Left,
    Right,
    Up,
    Down,
}

/// Registers the column select commands. None edits the buffer.
pub(crate) fn register(registry: &mut Registry<Editor>) {
    let commands = [
        ("column_select_left", "Column Select Left", Direction::Left),
        (
            "column_select_right",
            "Column Select Right",
            Direction::Right,
        ),
        ("column_select_up", "Column Select Up", Direction::Up),
        ("column_select_down", "Column Select Down", Direction::Down),
    ];
    for (name, title, direction) in commands {
        add(registry, name, title, false, move |model, _| {
            model.column_select(direction);
            Ok(Effect::Changed)
        });
    }
}

impl EditorModel {
    /// Moves the box's head one column or row, starting a box at the
    /// primary selection's head if none is under way, and selects it.
    fn column_select(&mut self, direction: Direction) {
        self.sync_display();
        let current = self.selections();
        let version = self.snapshot.version();
        let active = (self.column_box.take())
            .filter(|column_box| column_box.version == version && column_box.selections == current);
        let mut column_box = active.unwrap_or_else(|| {
            let head = self.primary_selection().head();
            let text = self.text();
            let row = text.offset_to_point(head).row;
            let expanded = self.expanded_row(row);
            let display = expanded.to_display(head - text.row_start(row));
            let point = (row, expanded.char_column(display));
            ColumnBox {
                anchor: point,
                head: point,
                version,
                selections: Vec::new(),
            }
        });
        let last_row = self.text().row_count() - 1;
        let (row, column) = &mut column_box.head;
        match direction {
            Direction::Left => *column = column.saturating_sub(1),
            Direction::Right => *column += 1,
            Direction::Up => *row = row.saturating_sub(1),
            Direction::Down => *row = (*row + 1).min(last_row),
        }
        self.select_box(column_box.anchor, column_box.head);
        column_box.selections = self.selections();
        self.column_box = Some(column_box);
    }
}

#[cfg(test)]
#[path = "column_select_tests.rs"]
mod tests;

/// A box selection under way goes on after an undo history import, whose
/// edits take the text away and back (T483).
#[cfg(test)]
mod import_tests {
    use fe_buffer::{Buffer, Origin};

    use super::Direction;
    use crate::EditorModel;

    #[test]
    fn a_box_goes_on_after_an_undo_history_import() {
        let mut typed = Buffer::new("abcdef\nabcdef\nabcdef\n");
        typed.edit([(21..21, "abcdef\n")], Origin::User);
        let record = typed.export_undo().unwrap();
        let mut model = EditorModel::new(Buffer::new(&typed.text().to_string()), None);
        model.set_cursor(10);
        model.column_select(Direction::Down);
        assert_eq!(model.selections().len(), 2);
        let state = model.selection_state();
        model.buffer_mut().import_undo(record).unwrap();
        model.restore_selection_state(state);
        model.column_select(Direction::Down);
        let heads: Vec<usize> = (model.selections().iter())
            .map(|selection| selection.head())
            .collect();
        assert_eq!(heads, [10, 17, 24], "the box grew a row");
    }
}
