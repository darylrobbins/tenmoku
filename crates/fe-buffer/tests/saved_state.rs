//! The unsaved indicator follows the text, not the edit count.

use fe_buffer::{Buffer, Origin, ProposedChange};
use fe_text::LineEnding;

fn user(buffer: &mut Buffer, range: std::ops::Range<usize>, text: &str) {
    buffer.edit([(range, text)], Origin::User);
    buffer.end_transaction();
}

fn undo(buffer: &mut Buffer) {
    buffer.undo(|o| *o == Origin::User).unwrap().unwrap();
}

#[test]
fn undo_to_the_save_point_is_clean() {
    let mut buffer = Buffer::new("hello world");
    buffer.did_save();
    user(&mut buffer, 0..0, "abc");
    user(&mut buffer, 5..6, "XY");
    user(&mut buffer, 0..1, "");
    assert!(buffer.is_dirty());
    undo(&mut buffer);
    undo(&mut buffer);
    assert!(buffer.is_dirty());
    undo(&mut buffer);
    assert!(!buffer.is_dirty());
    buffer.redo(|o| *o == Origin::User).unwrap().unwrap();
    assert!(buffer.is_dirty());
    undo(&mut buffer);
    assert!(!buffer.is_dirty());

    let mut loaded = Buffer::new("loaded");
    assert!(!loaded.is_dirty());
    user(&mut loaded, 0..1, "L");
    user(&mut loaded, 6..6, "!");
    assert!(loaded.is_dirty());
    undo(&mut loaded);
    undo(&mut loaded);
    assert!(!loaded.is_dirty());
}

#[test]
fn retyping_the_saved_text_is_clean() {
    let mut buffer = Buffer::new("one two three");
    user(&mut buffer, 4..7, "");
    assert!(buffer.is_dirty());
    user(&mut buffer, 4..4, "two");
    assert!(!buffer.is_dirty());

    user(&mut buffer, 0..1, "O");
    assert!(buffer.is_dirty());
    user(&mut buffer, 0..1, "o");
    assert!(!buffer.is_dirty());

    user(&mut buffer, 0..1, "O");
    user(&mut buffer, 12..13, "E");
    assert!(buffer.is_dirty());
    user(&mut buffer, 0..1, "o");
    assert!(buffer.is_dirty());
    user(&mut buffer, 12..13, "e");
    assert!(!buffer.is_dirty());
}

#[test]
fn ai_proposal_edits_track_the_save() {
    let ai = Origin::Ai {
        agent: "claude".into(),
        session: "s1".into(),
    };
    let mut buffer = Buffer::new("one\ntwo\n");
    buffer.did_save();
    let base = buffer.snapshot();
    let mut proposal = ProposedChange::from_text(&base, 0..base.len(), "ONE\ntwo\n", ai.clone());
    let hunk = proposal.hunks(buffer.current())[0].id;
    proposal.accept(&mut buffer, hunk).unwrap();
    assert!(buffer.is_dirty());
    buffer.undo(|o| *o == ai).unwrap().unwrap();
    assert!(!buffer.is_dirty());
}

#[test]
fn save_moves_the_baseline() {
    let mut buffer = Buffer::new("abc");
    user(&mut buffer, 0..0, "x");
    buffer.did_save();
    assert!(!buffer.is_dirty());
    undo(&mut buffer);
    assert!(buffer.is_dirty());

    let mut buffer = Buffer::new("a\nb");
    assert_eq!(buffer.line_ending(), LineEnding::Lf);
    buffer.set_line_ending(LineEnding::CrLf);
    assert!(buffer.is_dirty());
    buffer.set_line_ending(LineEnding::Lf);
    assert!(!buffer.is_dirty());
}
