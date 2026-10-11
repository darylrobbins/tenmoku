//! `Buffer::edit_in` grows an earlier transaction while other origins edit.

use fe_buffer::{Buffer, BufferEvent, EditError, EditKind, Origin, TransactionId};

fn ai() -> Origin {
    Origin::Ai {
        agent: "a".into(),
        session: "s".into(),
    }
}

fn text(buffer: &Buffer) -> String {
    buffer.text().slice_to_string(0..buffer.len())
}

fn ai_group(buffer: &mut Buffer) -> TransactionId {
    let id = buffer.edit([(0..0, "A")], ai()).unwrap();
    buffer.end_transaction();
    id
}

#[test]
fn grows_one_group() {
    let mut buffer = Buffer::new("hello");
    let id = ai_group(&mut buffer);
    buffer.edit([(buffer.len()..buffer.len(), "!")], Origin::User);
    buffer.end_transaction();
    assert_eq!(buffer.edit_in(id, [(1..1, "B")]), Ok(Some(id)));
    buffer.edit([(buffer.len()..buffer.len(), "?")], Origin::User);
    buffer.end_transaction();
    assert_eq!(buffer.edit_in(id, [(2..2, "C")]), Ok(Some(id)));
    assert_eq!(text(&buffer), "ABChello!?");

    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), "hello!?");
    assert_eq!(buffer.redo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), "ABChello!?");
    buffer.undo(|o| o.is_ai()).unwrap();
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(None));
}

#[test]
fn leaves_the_open_transaction_alone() {
    let mut buffer = Buffer::new("");
    let id = ai_group(&mut buffer);
    let user = buffer.start_transaction(Origin::User);
    buffer.edit([(1..1, "u")], Origin::User);
    assert_eq!(buffer.edit_in(id, [(0..0, "B")]), Ok(Some(id)));
    assert_eq!(buffer.edit([(0..0, "v")], Origin::User), Some(user));
    assert_eq!(buffer.end_transaction(), Some(user));
    assert_eq!(text(&buffer), "vBAu");

    // The AI group holds only its own batches; user's holds both edits.
    buffer.undo(|o| *o == Origin::User).unwrap();
    assert_eq!(text(&buffer), "BA");
    buffer.undo(|o| o.is_ai()).unwrap();
    assert_eq!(text(&buffer), "");
}

#[test]
fn refuses_gone_transactions() {
    let mut buffer = Buffer::new("x");
    let id = ai_group(&mut buffer);
    let other = buffer.edit([(0..0, "o")], Origin::Lsp).unwrap();
    buffer.undo(|o| o.is_ai()).unwrap();
    let sub = buffer.subscribe();
    let before = text(&buffer);
    assert_eq!(
        buffer.edit_in(id, [(0..0, "Z")]),
        Err(EditError::NoSuchTransaction(id))
    );
    assert_eq!(text(&buffer), before);
    assert!(sub.drain().is_empty());
    // The redo is still there.
    assert_eq!(buffer.redo(|o| o.is_ai()), Ok(Some(id)));

    buffer.undo(|o| o.is_ai()).unwrap();
    let unknown = buffer.start_transaction(Origin::Lsp);
    buffer.end_transaction(); // empty: dropped from the history
    assert_eq!(
        buffer.edit_in(unknown, [(0..0, "Z")]),
        Err(EditError::NoSuchTransaction(unknown))
    );
    assert_eq!(buffer.redo(|o| o.is_ai()), Ok(Some(id)));

    // Compacted away.
    let version = buffer.version();
    buffer.compact_log(version);
    assert_eq!(
        buffer.edit_in(other, [(0..0, "Z")]),
        Err(EditError::NoSuchTransaction(other))
    );
    assert_eq!(
        buffer.edit_in(id, [(0..0, "Z")]),
        Err(EditError::NoSuchTransaction(id))
    );
}

#[test]
fn bookkeeping_matches_edit() {
    let mut buffer = Buffer::new("abc");
    buffer.did_save();
    let id = ai_group(&mut buffer);
    let bytes = buffer.history_bytes(&ai());
    let sub = buffer.subscribe();
    assert_eq!(buffer.edit_in(id, [(0..0, "")]), Ok(None));
    assert!(sub.drain().is_empty());

    buffer.edit_in(id, [(0..1, "XY")]).unwrap();
    assert!(buffer.history_bytes(&ai()) > bytes);
    let events = sub.drain();
    assert_eq!(events.len(), 1);
    assert!(matches!(
        &events[0],
        BufferEvent::Edited { transaction, kind: EditKind::Edit, origin, .. }
            if *transaction == id && *origin == ai()
    ));
    assert!(buffer.is_dirty());
    // Reverting by hand to the saved text is clean again.
    buffer.edit_in(id, [(0..2, "")]).unwrap();
    assert_eq!(text(&buffer), "abc");
    assert!(!buffer.is_dirty());

    // Discards the origin's redos.
    buffer.edit([(0..0, "2")], ai()).unwrap();
    buffer.end_transaction();
    buffer.undo(|o| o.is_ai()).unwrap();
    buffer.edit_in(id, [(0..0, "3")]).unwrap();
    assert_eq!(buffer.redo(|o| o.is_ai()), Ok(None));
}

#[test]
fn extending_an_older_transaction_keeps_it_over_the_limit() {
    let mut buffer = Buffer::new("");
    buffer.set_history_limit(100);
    let id = ai_group(&mut buffer);
    let newer = buffer.edit([(1..1, "B")], ai()).unwrap();
    buffer.end_transaction();
    // Pushes the origin over its limit; the older entry would be the one
    // trimmed if it weren't protected.
    buffer.edit_in(id, [(0..0, "x".repeat(200))]).unwrap();
    assert!(buffer.history_bytes(&ai()) > 100);
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(Some(newer)));
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(Some(id)));
    assert_eq!(text(&buffer), "");
}
