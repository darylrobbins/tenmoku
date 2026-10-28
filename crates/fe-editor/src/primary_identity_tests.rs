//! The primary selection is the one Select Next Occurrence (cmd-d) or Move
//! Last Selection to Next Find Match (cmd-k cmd-d) last added or moved, not
//! the bottommost, until something else replaces the selections.

use std::ops::Range;

use fe_buffer::Origin;
use fe_command::Invocation;

use crate::{
    EditorModel, Movement,
    commands::{self, EditorTarget, HeadlessHost},
    tests::{parse, render},
};

const NEXT: &str = "select_next_occurrence";
const MOVE: &str = "move_last_selection_to_next_find_match";

fn run(model: &mut EditorModel, name: &str) {
    let mut host = HeadlessHost::new();
    let invocation = Invocation::of(&format!("editor.{name}"))
        .unwrap()
        .context(commands::context());
    commands::registry()
        .dispatch(&mut EditorTarget::new(model, &mut host), &invocation)
        .unwrap();
}

/// A change to the selections, by name.
type Change = (&'static str, fn(&mut EditorModel));

fn primary(model: &EditorModel) -> Range<usize> {
    model.primary_selection().range
}

fn tracked(model: &EditorModel) -> bool {
    model
        .selections
        .primary_index_in(model.snapshot())
        .is_some()
}

/// Carets on the last two words, then two cmd-d: the second wraps to the
/// first `foo`, leaving the one at 6 unselected.
fn wrapped() -> EditorModel {
    let mut model = parse("foo c foo a |foo b fo|o");
    run(&mut model, NEXT);
    assert_eq!(render(&model), "foo c foo a [foo] b [foo]");
    run(&mut model, NEXT);
    model
}

#[test]
fn cmd_d_makes_the_added_match_primary() {
    let model = wrapped();
    assert_eq!(render(&model), "[foo] c foo a [foo] b [foo]");
    assert_eq!(primary(&model), 0..3);
    assert_eq!(model.selections().len(), 3);
    // Without a wrap, the added match is also the bottommost.
    let mut model = parse("[foo] foo");
    run(&mut model, NEXT);
    assert_eq!(primary(&model), 4..7);
}

#[test]
fn cmd_d_searches_from_the_primary() {
    let mut model = wrapped();
    // A match past the bottommost selection: searching from there would
    // take it, from the primary takes the one after the primary.
    model.buffer_mut().edit([(21..21, " foo")], Origin::User);
    run(&mut model, NEXT);
    assert_eq!(render(&model), "[foo] c [foo] a [foo] b [foo] foo");
    assert_eq!(primary(&model), 6..9);
    // From there the selected ones are skipped.
    run(&mut model, NEXT);
    assert_eq!(render(&model), "[foo] c [foo] a [foo] b [foo] [foo]");
    assert_eq!(primary(&model), 22..25);
    // None left: nothing changes.
    run(&mut model, NEXT);
    assert_eq!(model.selections().len(), 5);
    assert_eq!(primary(&model), 22..25);
}

#[test]
fn move_makes_the_moved_selection_primary() {
    // The primary is the bottommost to start with: moved by wrapping.
    let mut model = parse("foo a [foo] b foo c [foo]");
    run(&mut model, MOVE);
    assert_eq!(render(&model), "[foo] a [foo] b foo c foo");
    assert_eq!(primary(&model), 0..3);
    // Again: the primary at the top moves on, past the selected one, and
    // the bottommost stays.
    run(&mut model, MOVE);
    assert_eq!(render(&model), "foo a [foo] b [foo] c foo");
    assert_eq!(primary(&model), 12..15);
    run(&mut model, MOVE);
    assert_eq!(render(&model), "foo a [foo] b foo c [foo]");
    assert_eq!(primary(&model), 18..21);

    // After a wrapped cmd-d, the primary at the top is the one moved.
    let mut model = wrapped();
    run(&mut model, MOVE);
    assert_eq!(render(&model), "foo c [foo] a [foo] b [foo]");
    assert_eq!(primary(&model), 6..9);
}

#[test]
fn identity_clears_on_other_changes() {
    let changes: [Change; 5] = [
        ("set_cursor", |model| model.set_cursor(5)),
        ("select_range", |model| model.select_range(4..5, false)),
        ("move_cursors", |model| {
            model.move_cursors(Movement::Right, false)
        }),
        ("set_selections", |model| {
            let same = model.selections();
            model.set_selections(same);
        }),
        ("extend_primary_to", |model| model.extend_primary_to(5)),
    ];
    for (name, change) in changes {
        let mut model = wrapped();
        assert!(tracked(&model), "{name}");
        change(&mut model);
        assert!(!tracked(&model), "{name}");
        let last = model.selections().pop().unwrap();
        assert_eq!(model.primary_selection(), last, "{name}");
    }
    // The arrow moves every selection: the last one is primary again.
    let mut model = wrapped();
    model.move_cursors(Movement::Right, false);
    assert_eq!(render(&model), "foo| c foo a foo| b foo|");
    assert_eq!(primary(&model), 21..21);
    // Shift-click extends from the tracked primary's tail.
    let mut model = wrapped();
    model.extend_primary_to(5);
    assert_eq!(render(&model), "[foo c] foo a foo b foo");

    // Escape keeps the tracked primary alone, which is then just the last.
    let mut model = wrapped();
    run(&mut model, "single_selection");
    assert_eq!(render(&model), "[foo] c foo a foo b foo");
    assert!(!tracked(&model));

    // Typing at the carets keeps it on the same selection.
    let mut model = wrapped();
    model.insert("X");
    assert_eq!(render(&model), "X| c foo a X| b X|");
    assert_eq!(primary(&model), 1..1);
    assert!(tracked(&model));

    // An edit merging it into another makes the merged one primary.
    // (Not whole words, so the second match's text finds the first.)
    let mut model = parse("aa[aa] b aaa");
    run(&mut model, NEXT);
    assert_eq!(primary(&model), 7..9);
    run(&mut model, NEXT);
    assert_eq!(render(&model), "[aa][aa] b [aa]a");
    assert_eq!(primary(&model), 0..2);
    model.backspace();
    assert_eq!(render(&model), "| b |a");
    assert_eq!(primary(&model), 0..0);
    // Also when the edits overlap and merge, leaving fewer carets.
    let mut model = parse("aa[aa] b aaa");
    run(&mut model, NEXT);
    run(&mut model, NEXT);
    model.insert("X");
    assert_eq!(render(&model), "X|X| b X|a");
    assert_eq!(primary(&model), 1..1);
    model.delete_word_left();
    assert_eq!(render(&model), "| b |a");
    assert_eq!(primary(&model), 0..0);
}

#[test]
fn cursor_undo_restores_identity() {
    let mut model = wrapped();
    run(&mut model, MOVE);
    assert_eq!(render(&model), "foo c [foo] a [foo] b [foo]");
    // No text-undo entry.
    assert_eq!(model.undo(), Ok(None));

    // Each command one step back, with its primary.
    model.cursor_undo();
    assert_eq!(render(&model), "[foo] c foo a [foo] b [foo]");
    assert_eq!(primary(&model), 0..3);
    model.cursor_undo();
    assert_eq!(render(&model), "foo c foo a [foo] b [foo]");
    assert_eq!(primary(&model), 18..21);
    assert!(!tracked(&model));
    model.cursor_redo();
    assert_eq!(render(&model), "[foo] c foo a [foo] b [foo]");
    assert_eq!(primary(&model), 0..3);
    model.cursor_redo();
    assert_eq!(render(&model), "foo c [foo] a [foo] b [foo]");
    assert_eq!(primary(&model), 6..9);

    // Only the primary changing is a step too.
    let mut model = wrapped();
    let same = model.selections();
    model.set_selections(same);
    assert_eq!(primary(&model), 18..21);
    model.cursor_undo();
    assert_eq!(render(&model), "[foo] c foo a [foo] b [foo]");
    assert_eq!(primary(&model), 0..3);
    model.cursor_redo();
    assert_eq!(primary(&model), 18..21);

    // An entry that edits leave with the same selections as now but
    // another primary is still stepped to.
    let mut model = parse("foo [foo] x [foo]");
    run(&mut model, NEXT);
    assert_eq!(render(&model), "[foo] [foo] x [foo]");
    assert_eq!(primary(&model), 0..3);
    model.buffer_mut().edit([(0..7, "")], Origin::User);
    assert_eq!(render(&model), "| x [foo]");
    assert_eq!(primary(&model), 0..0);
    model.cursor_undo();
    assert_eq!(render(&model), "| x [foo]");
    assert_eq!(primary(&model), 3..6);
}
