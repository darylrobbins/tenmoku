//! The wall-clock time each history entry carries, and the history as of
//! each save: what fe-vim's `:earlier` and `:later` step by.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use fe_buffer::{Buffer, HistoryTime, HistoryWrite, Origin, TransactionId};

/// A buffer whose clock reads `now`.
fn clocked(text: &str, now: &Arc<AtomicU64>) -> Buffer {
    let mut buffer = Buffer::new(text);
    let clock = now.clone();
    buffer.set_clock(move || clock.load(Ordering::Relaxed));
    buffer
}

/// The saves `history_writes` lists for `origin`: number and transaction.
fn writes(buffer: &Buffer, origin: &Origin) -> Vec<(u64, Option<TransactionId>)> {
    buffer
        .history_writes(|o| o == origin)
        .into_iter()
        .map(
            |HistoryWrite {
                 number,
                 transaction,
             }| (number, transaction),
        )
        .collect()
}

fn times(buffer: &Buffer, origin: &Origin) -> Vec<(TransactionId, u64, bool)> {
    buffer
        .history_times(|o| o == origin)
        .into_iter()
        .map(
            |HistoryTime {
                 transaction,
                 at_unix_ms,
                 undone,
             }| (transaction, at_unix_ms, undone),
        )
        .collect()
}

#[test]
fn entries_carry_a_creation_time_and_saves_are_marked() {
    let now = Arc::new(AtomicU64::new(1_000));
    let mut buffer = clocked("", &now);
    let user = Origin::User;
    let ai = Origin::Ai {
        agent: "a".into(),
        session: "s".into(),
    };
    assert_eq!(times(&buffer, &user), []);
    assert_eq!(writes(&buffer, &user), []);

    // Saved before any change: none applied.
    buffer.did_save();
    let one = buffer.edit([(0..0, "one")], user.clone()).unwrap();
    now.store(5_000, Ordering::Relaxed);
    // A transaction is timed when it opens, not when it closes.
    let two = buffer.start_transaction(user.clone());
    now.store(6_000, Ordering::Relaxed);
    buffer.edit([(3..3, " two")], user.clone());
    buffer.end_transaction();
    now.store(7_000, Ordering::Relaxed);
    let other = buffer.edit([(0..0, "> ")], ai.clone()).unwrap();
    buffer.did_save();
    now.store(9_000, Ordering::Relaxed);
    let three = buffer.edit([(0..0, "3")], user.clone()).unwrap();
    assert_eq!(
        times(&buffer, &user),
        [
            (one, 1_000, false),
            (two, 5_000, false),
            (three, 9_000, false)
        ]
    );
    assert_eq!(times(&buffer, &ai), [(other, 7_000, false)]);

    // Undone entries follow, in the order redo reapplies them; they keep
    // their times.
    buffer.undo(|o| o == &user).unwrap().unwrap();
    buffer.undo(|o| o == &user).unwrap().unwrap();
    assert_eq!(
        times(&buffer, &user),
        [
            (one, 1_000, false),
            (two, 5_000, true),
            (three, 9_000, true)
        ]
    );
    // A save while undone marks what's applied then; a reload is no save.
    buffer.did_save();
    let snapshot = buffer.save_snapshot();
    buffer.redo(|o| o == &user).unwrap().unwrap();
    buffer.did_save_snapshot(&snapshot, None);
    assert_eq!(
        writes(&buffer, &user),
        [(1, None), (2, Some(two)), (3, Some(one)), (4, Some(one))]
    );
    assert_eq!(
        writes(&buffer, &ai),
        [
            (1, None),
            (2, Some(other)),
            (3, Some(other)),
            (4, Some(other))
        ]
    );
    assert_eq!(
        buffer
            .history_writes(|o| o != &user && o != &ai)
            .into_iter()
            .map(|write| write.transaction)
            .collect::<Vec<_>>(),
        [None, None, None, None]
    );

    // A new change discards the redos, keeping the rest, and the save
    // that had one applied can't be reached any more.
    buffer.undo(|o| o == &user).unwrap().unwrap();
    now.store(12_000, Ordering::Relaxed);
    let four = buffer.edit([(0..0, "4")], user.clone()).unwrap();
    assert_eq!(
        times(&buffer, &user),
        [(one, 1_000, false), (four, 12_000, false)]
    );
    assert_eq!(
        writes(&buffer, &user),
        [(1, None), (3, Some(one)), (4, Some(one))]
    );
}

#[test]
fn a_save_is_unreachable_once_its_transactions_grow() {
    let user = Origin::User;
    // A direct save inside an open transaction that then grows.
    let mut buffer = Buffer::new("");
    let first = buffer.edit([(0..0, "x")], user.clone()).unwrap();
    buffer.did_save();
    buffer.start_transaction(user.clone());
    buffer.edit([(1..1, "a")], user.clone());
    buffer.did_save();
    assert_eq!(writes(&buffer, &user).len(), 2);
    buffer.edit([(2..2, "b")], user.clone());
    buffer.end_transaction();
    assert_eq!(writes(&buffer, &user), [(1, Some(first))]);

    // A snapshot save, then a transaction up to its newest extended.
    let mut buffer = Buffer::new("");
    let first = buffer.edit([(0..0, "a")], user.clone()).unwrap();
    let second = buffer.edit([(1..1, "b")], user.clone()).unwrap();
    let snapshot = buffer.save_snapshot();
    buffer.did_save_snapshot(&snapshot, None);
    buffer.did_save();
    assert_eq!(
        writes(&buffer, &user),
        [(1, Some(second)), (2, Some(second))]
    );
    buffer.edit_in(first, [(0..0, "<")]).unwrap().unwrap();
    assert_eq!(writes(&buffer, &user), []);
    // A snapshot whose transaction grew before the save was reported.
    let snapshot = buffer.save_snapshot();
    buffer.edit_in(second, [(0..0, "!")]).unwrap().unwrap();
    buffer.did_save_snapshot(&snapshot, None);
    assert_eq!(writes(&buffer, &user), []);
    // Undo and redo keep a save reachable.
    buffer.did_save();
    buffer.undo(|o| o == &user).unwrap().unwrap();
    buffer.redo(|o| o == &user).unwrap().unwrap();
    assert_eq!(writes(&buffer, &user), [(4, Some(second))]);
}

#[test]
fn a_save_is_unreachable_once_the_history_limit_drops_its_origins_changes() {
    let user = Origin::User;
    let ai = Origin::Ai {
        agent: "a".into(),
        session: "s".into(),
    };
    let mut buffer = Buffer::new("");
    buffer.did_save();
    buffer.edit([(0..0, "a")], user.clone());
    buffer.did_save();
    // Another origin's history going doesn't matter to the user's saves;
    // to that origin's, from before its first change, it does.
    buffer.set_history_limit(1);
    buffer.edit([(0..0, "b")], ai.clone());
    buffer.edit([(0..0, "c")], ai.clone());
    assert_eq!(writes(&buffer, &user).len(), 2);
    assert_eq!(writes(&buffer, &ai), []);
    let last = buffer.edit([(0..0, "d")], user.clone()).unwrap();
    buffer.did_save();
    assert_eq!(writes(&buffer, &user), [(3, Some(last))]);
}

#[test]
fn an_open_transaction_counts_once_it_changes_something() {
    let now = Arc::new(AtomicU64::new(3));
    let mut buffer = clocked("", &now);
    let open = buffer.start_transaction(Origin::User);
    assert_eq!(times(&buffer, &Origin::User), []);
    buffer.did_save();
    buffer.edit([(0..0, "x")], Origin::User);
    assert_eq!(times(&buffer, &Origin::User), [(open, 3, false)]);
    buffer.did_save();
    assert_eq!(writes(&buffer, &Origin::User), [(1, None), (2, Some(open))]);
}

#[test]
fn the_default_clock_is_the_systems() {
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let mut buffer = Buffer::new("");
    buffer.edit([(0..0, "x")], Origin::User);
    let at = buffer.history_times(|_| true)[0].at_unix_ms;
    assert!(at >= before && at < before + 60_000, "{at} vs {before}");
}

#[test]
fn the_history_revision_follows_every_step_but_saves() {
    use fe_buffer::LineEnding;

    let mut buffer = Buffer::new("");
    let mut seen = vec![buffer.history_revision()];
    let mut changed = |buffer: &Buffer, step: &str| {
        let revision = buffer.history_revision();
        assert!(
            !seen.contains(&revision),
            "{step}: {revision} seen in {seen:?}"
        );
        seen.push(revision);
    };
    let one = buffer.edit([(0..0, "a")], Origin::User).unwrap();
    changed(&buffer, "edit");
    buffer.edit_in(one, [(1..1, "b")]).unwrap().unwrap();
    changed(&buffer, "edit_in");
    let version = buffer.version();
    buffer.change_line_ending(LineEnding::CrLf, Origin::User);
    changed(&buffer, "line ending");
    assert_eq!(buffer.version(), version);
    buffer.undo(|_| true).unwrap().unwrap();
    changed(&buffer, "undo of the line ending");
    buffer.redo(|_| true).unwrap().unwrap();
    changed(&buffer, "redo of the line ending");
    assert_eq!(buffer.version(), version);
    let before = buffer.history_revision();
    buffer.did_save();
    assert_eq!(buffer.history_revision(), before, "a save");
    buffer.set_history_limit(1);
    changed(&buffer, "history dropped");
}
