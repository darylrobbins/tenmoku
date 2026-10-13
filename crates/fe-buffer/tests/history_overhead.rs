//! The undo history cap counts each batch's bookkeeping, not just its text.

use fe_buffer::{Buffer, Origin};

fn type_char(buffer: &mut Buffer) {
    let end = buffer.len();
    buffer.edit([(end..end, "a")], Origin::User).unwrap();
}

fn undo_user(buffer: &mut Buffer) -> Option<fe_buffer::TransactionId> {
    buffer.undo(|origin| *origin == Origin::User).unwrap()
}

#[test]
fn one_character_transactions_are_capped_by_their_bookkeeping() {
    const LIMIT: usize = 64 << 10;
    let mut buffer = Buffer::new("");
    buffer.set_history_limit(LIMIT);
    for _ in 0..20_000 {
        type_char(&mut buffer);
        assert!(buffer.history_bytes(&Origin::User) <= LIMIT);
    }
    let mut undone = 0;
    while undo_user(&mut buffer).is_some() {
        undone += 1;
    }
    assert!(undone > 0 && undone < 2_000, "{undone} undoable");
}

#[test]
fn charge_grows_with_edits() {
    let many = {
        let mut buffer = Buffer::new(&"x".repeat(1000));
        let edits: Vec<_> = (0..100).map(|i| (i..i, "a")).collect();
        buffer.edit(edits, Origin::User).unwrap();
        buffer.history_bytes(&Origin::User)
    };
    let one = {
        let mut buffer = Buffer::new(&"x".repeat(1000));
        buffer
            .edit([(0..0, "a".repeat(100))], Origin::User)
            .unwrap();
        buffer.history_bytes(&Origin::User)
    };
    assert!(many > one, "{many} vs {one}");
}

#[test]
fn default_limit_keeps_typing_undoable() {
    let mut buffer = Buffer::new("");
    for i in 0..10_001 {
        let end = buffer.len();
        let ch = if i == 0 { "F" } else { "a" };
        buffer.edit([(end..end, ch)], Origin::User).unwrap();
    }
    for _ in 0..10_000 {
        assert!(undo_user(&mut buffer).is_some());
    }
    assert_eq!(buffer.text().to_string(), "F");
}
