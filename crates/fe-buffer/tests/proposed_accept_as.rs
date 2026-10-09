//! Accepting a `ProposedChange`'s hunks under the caller's origin.

use fe_buffer::{Buffer, HunkStatus, Origin, ProposedChange};

fn ai() -> Origin {
    Origin::Ai {
        agent: "claude".into(),
        session: "s1".into(),
    }
}

fn proposal(buffer: &Buffer) -> ProposedChange {
    let base = buffer.snapshot();
    ProposedChange::from_text(&base, 0..base.len(), "ONE\ntwo\nTHREE\n", ai())
}

#[test]
fn user_undo_reverts_only_the_hunk() {
    let mut buffer = Buffer::new("one\ntwo\nthree\n");
    let mut proposal = proposal(&buffer);
    let first = proposal.hunks(buffer.current())[0].id;
    let end = buffer.text().len();
    buffer.edit([(end..end, "tail\n")], Origin::User);
    let typed = buffer.text().to_string();

    let id = proposal
        .accept_as(&mut buffer, first, Origin::User)
        .unwrap();
    assert_eq!(buffer.text().to_string(), typed.replacen("one", "ONE", 1));
    assert_eq!(buffer.undo(|o| *o == Origin::User), Ok(Some(id)));
    assert_eq!(buffer.text().to_string(), typed);
    assert_eq!(buffer.undo(|o| o.is_ai()), Ok(None));
    assert_eq!(
        proposal.hunks(buffer.current())[0].status,
        HunkStatus::Accepted
    );
    assert_eq!(proposal.origin(), &ai());
}

#[test]
fn all_in_one_user_transaction() {
    let text = "a\nb\nc\nd\ne\nf\ng\n";
    let mut buffer = Buffer::new(text);
    let snap = buffer.snapshot();
    let mut p = ProposedChange::from_text(&snap, 0..snap.len(), "A\nb\nc\nD\ne\nf\nG\n", ai());
    let ids: Vec<_> = p.hunks(&snap).iter().map(|h| h.id).collect();
    assert_eq!(ids.len(), 3);
    buffer.edit([(12..13, "x")], Origin::User); // "g" -> "x": third stale
    let typed = buffer.text().to_string();

    let (id, stale) = p.accept_all_as(&mut buffer, Origin::User);
    assert_eq!(stale, vec![ids[2]]);
    assert_eq!(buffer.text().to_string(), "A\nb\nc\nD\ne\nf\nx\n");
    assert_eq!(buffer.undo(|o| *o == Origin::User), Ok(id));
    assert_eq!(buffer.text().to_string(), typed);
}

#[test]
fn inside_an_open_user_transaction() {
    let text = "a\nb\nc\nd\ne\n";
    let mut buffer = Buffer::new(text);
    let snap = buffer.snapshot();
    let mut p = ProposedChange::from_text(&snap, 0..snap.len(), "A\nb\nc\nd\nE\n", ai());
    let ids: Vec<_> = p.hunks(&snap).iter().map(|h| h.id).collect();
    assert_eq!(ids.len(), 2);

    let open = buffer.start_transaction(Origin::User);
    buffer.edit([(2..3, "B")], Origin::User);
    let first = p.accept_as(&mut buffer, ids[0], Origin::User).unwrap();
    let (second, stale) = p.accept_all_as(&mut buffer, Origin::User);
    let second = second.unwrap();
    assert!(stale.is_empty());
    assert_ne!(first, open);
    assert_ne!(second, open);
    buffer.edit([(4..5, "C")], Origin::User);
    buffer.end_transaction();
    assert_eq!(buffer.text().to_string(), "A\nB\nC\nd\nE\n");

    // Each accept is a transaction of its own, newest first; the open
    // transaction's edits are undone last, together.
    assert_eq!(buffer.undo(|o| *o == Origin::User), Ok(Some(second)));
    assert_eq!(buffer.text().to_string(), "A\nB\nC\nd\ne\n");
    assert_eq!(buffer.undo(|o| *o == Origin::User), Ok(Some(first)));
    assert_eq!(buffer.text().to_string(), "a\nB\nC\nd\ne\n");
    assert_eq!(buffer.undo(|o| *o == Origin::User), Ok(Some(open)));
    assert_eq!(buffer.text().to_string(), text);
}
