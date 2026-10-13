//! Undo history is capped per origin, counting text and bookkeeping.

use super::{ai, is_ai, is_user};
use crate::*;

/// What a closed one-edit entry appending `n` text bytes is charged,
/// measured on a scratch buffer.
pub(super) fn entry(n: usize) -> usize {
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "x".repeat(n))], Origin::User).unwrap();
    buffer.history_bytes(&Origin::User)
}

/// What an open, still empty transaction is charged: the entry alone.
fn empty_entry() -> usize {
    let mut buffer = Buffer::new("");
    buffer.start_transaction(Origin::User);
    buffer.history_bytes(&Origin::User)
}

/// The same entry once undone: its batch counts a second time (the entry
/// minus its own overhead), derived from the charge for a fresh entry
/// rather than from undo's accounting.
pub(super) fn undone(n: usize) -> usize {
    entry(n) + entry(n) - empty_entry()
}

#[test]
fn empty_transactions_are_not_charged() {
    let mut buffer = Buffer::new("");
    for _ in 0..100 {
        buffer.start_transaction(Origin::User);
        buffer.end_transaction();
    }
    assert_eq!(buffer.history_bytes(&Origin::User), 0);
    type_chunk(&mut buffer, "a", Origin::User);
    assert_eq!(buffer.history_bytes(&Origin::User), entry(1));
    // An empty transaction closed by undo is released too.
    buffer.start_transaction(Origin::User);
    buffer.undo(is_user).unwrap();
    assert_eq!(buffer.history_bytes(&Origin::User), undone(1));
}

#[test]
fn undo_charges_the_inverse_batch_and_redo_releases_it() {
    let mut buffer = Buffer::new("");
    type_chunk(&mut buffer, "0123456789", Origin::User);
    let before = buffer.history_bytes(&Origin::User);
    assert_eq!(before, entry(10));
    buffer.undo(is_user).unwrap();
    let batch = entry(10) - empty_entry();
    assert_eq!(buffer.history_bytes(&Origin::User), before + batch);
    buffer.redo(is_user).unwrap();
    assert_eq!(buffer.history_bytes(&Origin::User), before);
}

/// Appends `chunk` as one user transaction.
fn type_chunk(buffer: &mut Buffer, chunk: &str, origin: Origin) -> TransactionId {
    let end = buffer.len();
    buffer.edit([(end..end, chunk)], origin).unwrap()
}

#[test]
fn history_stays_under_the_cap() {
    let mut buffer = Buffer::new("");
    let limit = 10 * entry(10);
    buffer.set_history_limit(limit);
    for _ in 0..50 {
        type_chunk(&mut buffer, "0123456789", Origin::User);
        assert!(buffer.history_bytes(&Origin::User) <= limit);
    }
    // Undone batches count too, and grouped edits are trimmed when the
    // transaction closes.
    buffer.undo(is_user).unwrap();
    buffer.start_transaction(Origin::User);
    for _ in 0..3 {
        type_chunk(&mut buffer, "abcdefghij", Origin::User);
    }
    buffer.end_transaction();
    assert!(buffer.history_bytes(&Origin::User) <= limit);

    // The newest transaction is kept even when it alone exceeds the cap.
    let big = "x".repeat(limit);
    let id = type_chunk(&mut buffer, &big, Origin::User);
    assert_eq!(buffer.history_bytes(&Origin::User), entry(limit));
    assert_eq!(buffer.undo(is_user), Ok(Some(id)));
    assert_eq!(buffer.history_bytes(&Origin::User), undone(limit));
    assert_eq!(buffer.redo(is_user), Ok(Some(id)));
    assert_eq!(buffer.undo(is_user), Ok(Some(id)));
    assert_eq!(buffer.undo(is_user), Ok(None));

    // Lowering the limit trims right away.
    let mut buffer = Buffer::new("");
    for _ in 0..10 {
        type_chunk(&mut buffer, "0123456789", Origin::User);
    }
    assert_eq!(buffer.history_bytes(&Origin::User), 10 * entry(10));
    buffer.set_history_limit(3 * entry(10));
    assert_eq!(buffer.history_bytes(&Origin::User), 3 * entry(10));
}

#[test]
fn dropped_transactions_cannot_be_undone_but_newer_ones_can() {
    let mut buffer = Buffer::new("");
    buffer.set_history_limit(3 * entry(10));
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
    // Undone entries count both their batch lists: b, plus c and d undone.
    let d = buffer.undo(is_user).unwrap().unwrap();
    buffer.undo(is_user).unwrap();
    assert_eq!(
        buffer.history_bytes(&Origin::User),
        entry(10) + 2 * undone(10)
    );
    // Room for d once redone, plus the one-byte entry typed after it.
    buffer.set_history_limit(entry(10) + entry(1));
    assert_eq!(buffer.history_bytes(&Origin::User), undone(10));
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
    buffer.set_history_limit(2 * entry(10));
    let ai_first = type_chunk(&mut buffer, "AAAAAAAAAA", ai());
    for _ in 0..10 {
        type_chunk(&mut buffer, "uuuuuuuuuu", Origin::User);
    }
    assert_eq!(buffer.history_bytes(&Origin::User), 2 * entry(10));
    assert_eq!(buffer.history_bytes(&ai()), entry(10));
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
    assert_eq!(buffer.history_bytes(&other), 2 * entry(10));
    assert_eq!(buffer.history_bytes(&ai()), entry(10));
    assert_eq!(buffer.undo(|origin| *origin == ai()), Ok(Some(ai_first)));
}
