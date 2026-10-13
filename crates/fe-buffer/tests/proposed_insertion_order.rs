//! Two proposed insertions whose points a user edit merged land in text order,
//! whichever hunk is accepted first.

use fe_buffer::{Buffer, HunkStatus, Origin, ProposedChange};

fn ai() -> Origin {
    Origin::Ai {
        agent: "claude".into(),
        session: "s1".into(),
    }
}

/// Hunk 0 inserts "X\n" and hunk 1 "Y\n", both at 2 once "b\n" is deleted.
fn setup() -> (Buffer, ProposedChange) {
    let mut buffer = Buffer::new("a\nb\nc\n");
    let base = buffer.snapshot();
    let proposal = ProposedChange::from_text(&base, 0..base.len(), "a\nX\nb\nY\nc\n", ai());
    assert_eq!(proposal.hunks(&base).len(), 2);
    buffer.edit([(2..4, "")], Origin::User);
    (buffer, proposal)
}

#[test]
fn accepting_a_later_insertion_first_keeps_text_order() {
    let (mut buffer, mut proposal) = setup();
    let ids: Vec<_> = proposal
        .hunks(buffer.current())
        .iter()
        .map(|h| h.id)
        .collect();
    proposal.accept(&mut buffer, ids[1]).unwrap();
    assert_eq!(buffer.text().to_string(), "a\nY\nc\n");
    let views = proposal.hunks(buffer.current());
    assert_eq!(views[0].range, 2..2);
    assert_eq!(views[1].range, 2..4);

    proposal.accept(&mut buffer, ids[0]).unwrap();
    assert_eq!(buffer.text().to_string(), "a\nX\nY\nc\n");
    let views = proposal.hunks(buffer.current());
    assert_eq!(views[0].range, 2..4);
    assert_eq!(views[1].range, 4..6);
}

#[test]
fn accept_all_after_a_later_insertion_keeps_text_order() {
    let (mut buffer, mut proposal) = setup();
    let ids: Vec<_> = proposal
        .hunks(buffer.current())
        .iter()
        .map(|h| h.id)
        .collect();
    proposal.accept(&mut buffer, ids[1]).unwrap();
    let dry = proposal.dry_run(buffer.current());
    assert_eq!(dry.text().to_string(), "a\nX\nY\nc\n");
    let (_, stale) = proposal.accept_all(&mut buffer);
    assert!(stale.is_empty());
    assert_eq!(buffer.text().to_string(), "a\nX\nY\nc\n");
    assert!(
        proposal
            .hunks(buffer.current())
            .iter()
            .all(|h| h.status == HunkStatus::Accepted)
    );
}

#[test]
fn accepting_in_text_order_keeps_text_order() {
    let (mut buffer, mut proposal) = setup();
    let ids: Vec<_> = proposal
        .hunks(buffer.current())
        .iter()
        .map(|h| h.id)
        .collect();
    proposal.accept(&mut buffer, ids[0]).unwrap();
    proposal.accept(&mut buffer, ids[1]).unwrap();
    assert_eq!(buffer.text().to_string(), "a\nX\nY\nc\n");
}
