//! Undo history is capped by text bytes per origin.

use super::{ai, is_ai, is_user};
use crate::*;

/// Appends `chunk` as one user transaction.
fn type_chunk(buffer: &mut Buffer, chunk: &str, origin: Origin) -> TransactionId {
    let end = buffer.len();
    buffer.edit([(end..end, chunk)], origin).unwrap()
}

#[test]
fn history_stays_under_the_cap() {
    let mut buffer = Buffer::new("");
    buffer.set_history_limit(100);
    for _ in 0..50 {
        type_chunk(&mut buffer, "0123456789", Origin::User);
        assert!(buffer.history_bytes(&Origin::User) <= 100);
    }
    // Undone batches count too, and grouped edits are trimmed when the
    // transaction closes.
    buffer.undo(is_user).unwrap();
    buffer.start_transaction(Origin::User);
    for _ in 0..3 {
        type_chunk(&mut buffer, "abcdefghij", Origin::User);
    }
    buffer.end_transaction();
    assert!(buffer.history_bytes(&Origin::User) <= 100);

    // The newest transaction is kept even when it alone exceeds the cap.
    let big = "x".repeat(500);
    let id = type_chunk(&mut buffer, &big, Origin::User);
    assert_eq!(buffer.history_bytes(&Origin::User), 500);
    assert_eq!(buffer.undo(is_user), Ok(Some(id)));
    assert_eq!(buffer.history_bytes(&Origin::User), 1000);
    assert_eq!(buffer.redo(is_user), Ok(Some(id)));
    assert_eq!(buffer.undo(is_user), Ok(Some(id)));
    assert_eq!(buffer.undo(is_user), Ok(None));

    // Lowering the limit trims right away.
    let mut buffer = Buffer::new("");
    for _ in 0..10 {
        type_chunk(&mut buffer, "0123456789", Origin::User);
    }
    assert_eq!(buffer.history_bytes(&Origin::User), 100);
    buffer.set_history_limit(30);
    assert_eq!(buffer.history_bytes(&Origin::User), 30);
}

#[test]
fn dropped_transactions_cannot_be_undone_but_newer_ones_can() {
    let mut buffer = Buffer::new("");
    buffer.set_history_limit(30);
    for chunk in ["aaaaaaaaaa", "bbbbbbbbbb", "cccccccccc", "dddddddddd"] {
        type_chunk(&mut buffer, chunk, Origin::User);
    }
    // "aaaaaaaaaa" was dropped; the other three are undoable.
    for _ in 0..3 {
        assert!(buffer.undo(is_user).unwrap().is_some());
    }
    assert_eq!(buffer.text().to_string(), "aaaaaaaaaa");
    assert_eq!(buffer.undo(is_user), Ok(None));
    assert_eq!(buffer.text().to_string(), "aaaaaaaaaa");

    for _ in 0..3 {
        assert!(buffer.redo(is_user).unwrap().is_some());
    }
    assert_eq!(buffer.redo(is_user), Ok(None));

    // Undone transactions can be dropped too, and redo stops past them.
    // Undone entries hold their text twice: b 10 + c 20 + d 20.
    let d = buffer.undo(is_user).unwrap().unwrap();
    buffer.undo(is_user).unwrap();
    assert_eq!(buffer.history_bytes(&Origin::User), 50);
    buffer.set_history_limit(20);
    assert_eq!(buffer.history_bytes(&Origin::User), 20);
    assert_eq!(buffer.text().to_string(), "aaaaaaaaaabbbbbbbbbb");
    assert_eq!(buffer.redo(is_user), Ok(Some(d)));
    assert_eq!(buffer.redo(is_user), Ok(None));
    assert_eq!(buffer.text().to_string(), "aaaaaaaaaabbbbbbbbbbdddddddddd");
    let e = type_chunk(&mut buffer, "e", Origin::User);
    assert_eq!(buffer.undo(is_user), Ok(Some(e)));
    assert_eq!(buffer.undo(is_user), Ok(Some(d)));
    assert_eq!(buffer.undo(is_user), Ok(None));
    assert_eq!(buffer.text().to_string(), "aaaaaaaaaabbbbbbbbbb");

    // The edit log is untouched: every version is still reachable.
    let snapshot = buffer.snapshot();
    assert_eq!(
        snapshot.edits_since(Version::default()).count(),
        snapshot.version().0
    );
}

#[test]
fn cap_is_per_origin() {
    let mut buffer = Buffer::new("");
    buffer.set_history_limit(20);
    let ai_first = type_chunk(&mut buffer, "AAAAAAAAAA", ai());
    for _ in 0..10 {
        type_chunk(&mut buffer, "uuuuuuuuuu", Origin::User);
    }
    assert_eq!(buffer.history_bytes(&Origin::User), 20);
    assert_eq!(buffer.history_bytes(&ai()), 10);
    // The user's edits don't push the AI's out.
    assert_eq!(buffer.undo(is_ai), Ok(Some(ai_first)));
    assert_eq!(buffer.redo(is_ai), Ok(Some(ai_first)));

    // Each AI session is its own origin.
    let other = Origin::Ai {
        agent: "agent".into(),
        session: "s2".into(),
    };
    for _ in 0..5 {
        type_chunk(&mut buffer, "SSSSSSSSSS", other.clone());
    }
    assert_eq!(buffer.history_bytes(&other), 20);
    assert_eq!(buffer.history_bytes(&ai()), 10);
    assert_eq!(buffer.undo(|origin| *origin == ai()), Ok(Some(ai_first)));
}
