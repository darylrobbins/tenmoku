//! Undo and redo of a line-ending change refuse when another origin has
//! changed the ending since: they never overwrite its setting.

use fe_buffer::{Buffer, LineEnding, Origin, UndoError};

fn host() -> Origin {
    Origin::Ai {
        agent: "host".into(),
        session: "s".into(),
    }
}

#[test]
fn undo_refuses_after_another_origin_changed_the_ending() {
    let mut buffer = Buffer::new("a\n");
    buffer.change_line_ending(LineEnding::CrLf, Origin::User);
    buffer.change_line_ending(LineEnding::Lf, host());
    let user = |origin: &Origin| *origin == Origin::User;
    assert!(matches!(buffer.undo(user), Err(UndoError::Conflict(_))));
    assert_eq!(buffer.line_ending(), LineEnding::Lf);
}

#[test]
fn redo_refuses_after_another_origin_changed_the_ending() {
    let mut buffer = Buffer::new("a\n");
    buffer.change_line_ending(LineEnding::CrLf, Origin::User);
    let user = |origin: &Origin| *origin == Origin::User;
    buffer.undo(user).unwrap().unwrap();
    buffer.change_line_ending(LineEnding::CrLf, host());
    assert!(matches!(buffer.redo(user), Err(UndoError::Conflict(_))));
    assert_eq!(buffer.line_ending(), LineEnding::CrLf);
}
