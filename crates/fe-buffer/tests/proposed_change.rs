//! `ProposedChange`: an AI's edits against a snapshot, taken hunk by hunk
//! while the user keeps typing.

use std::sync::Arc;

use fe_buffer::{Buffer, BufferEvent, HunkStatus, HunkView, Origin, ProposalError, ProposedChange};

const THREE_FNS: &str = "\
fn one() {
    1
}

fn two() {
    2
}

fn three() {
    3
}
";

fn ai() -> Origin {
    Origin::Ai {
        agent: "claude".into(),
        session: "s1".into(),
    }
}

/// The text each hunk's range covers now.
fn texts_at(buffer: &Buffer, hunks: &[HunkView]) -> Vec<String> {
    hunks
        .iter()
        .map(|hunk| buffer.text().slice_to_string(hunk.range.clone()))
        .collect()
}

/// `THREE_FNS` with the bodies of `one` and `three` rewritten.
fn rewritten() -> String {
    THREE_FNS
        .replace("    1\n", "    let x = 1;\n    x\n")
        .replace("    3\n", "    30\n")
}

#[test]
fn from_text_splits_line_hunks() {
    let buffer = Buffer::new(THREE_FNS);
    let base = buffer.snapshot();
    let proposal = ProposedChange::from_text(&base, 0..base.len(), &rewritten(), ai());

    let hunks = proposal.hunks(&base);
    assert_eq!(hunks.len(), 2, "{hunks:?}");
    let one = THREE_FNS.find("    1\n").unwrap();
    let three = THREE_FNS.find("    3\n").unwrap();
    assert_eq!(hunks[0].range, one..one + 6);
    assert_eq!(&*hunks[0].old_text, "    1\n");
    assert_eq!(&*hunks[0].new_text, "    let x = 1;\n    x\n");
    assert_eq!(hunks[1].range, three..three + 6);
    assert_eq!(&*hunks[1].old_text, "    3\n");
    assert_eq!(&*hunks[1].new_text, "    30\n");
    assert!(hunks.iter().all(|hunk| hunk.status == HunkStatus::Pending));
    assert_ne!(hunks[0].id, hunks[1].id);

    // A sub-range: offsets stay in buffer coordinates.
    let two = THREE_FNS.find("fn two").unwrap();
    let sub = ProposedChange::from_text(
        &base,
        two..base.len(),
        &THREE_FNS[two..].replace("    3\n", "    30\n"),
        ai(),
    );
    let hunks = sub.hunks(&base);
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].range, three..three + 6);

    let same = ProposedChange::from_text(&base, 0..base.len(), THREE_FNS, ai());
    assert!(same.hunks(&base).is_empty());
    assert!(same.is_resolved());
}

#[test]
fn accepting_a_hunk_while_the_user_types() {
    let mut buffer = Buffer::new(THREE_FNS);
    let base = buffer.snapshot();
    let mut proposal = ProposedChange::from_text(&base, 0..base.len(), &rewritten(), ai());
    let [first, second] = [0, 1].map(|ix| proposal.hunks(&base)[ix].id);

    // The user types elsewhere, then at both edges of the first hunk.
    buffer.edit([(0..0, "// top\n")], Origin::User);
    let start = proposal.hunks(buffer.current())[0].range.start;
    buffer.edit([(start..start, "    // before\n")], Origin::User);
    let end = proposal.hunks(buffer.current())[0].range.end;
    buffer.edit([(end..end, "    // after\n")], Origin::User);
    let typed = buffer.text().to_string();

    let events = buffer.subscribe();
    let transaction = proposal.accept(&mut buffer, first).unwrap();
    let expected = typed.replace(
        "    // before\n    1\n    // after\n",
        "    // before\n    let x = 1;\n    x\n    // after\n",
    );
    assert_eq!(buffer.text().to_string(), expected);
    // One batch, recorded as one transaction with the proposal's origin.
    let events = events.drain();
    assert_eq!(events.len(), 1);
    let BufferEvent::Edited {
        transaction: id,
        origin,
        ..
    } = &events[0]
    else {
        panic!("{events:?}")
    };
    assert_eq!((*id, origin), (transaction, &ai()));

    let hunks = proposal.hunks(buffer.current());
    assert_eq!(hunks[0].status, HunkStatus::Accepted);
    assert_eq!(texts_at(&buffer, &hunks)[0], "    let x = 1;\n    x\n");
    // The other hunk followed the user's typing and is still pending.
    assert_eq!(hunks[1].status, HunkStatus::Pending);
    let three = expected.find("    3\n").unwrap();
    assert_eq!(hunks[1].range, three..three + 6);
    assert_eq!(
        proposal.accept(&mut buffer, first),
        Err(ProposalError::NotPending)
    );

    // Undoing the AI's edits reverts only the accepted hunk.
    assert_eq!(buffer.undo(|origin| origin.is_ai()), Ok(Some(transaction)));
    assert_eq!(buffer.text().to_string(), typed);
    assert_eq!(buffer.undo(|origin| origin.is_ai()), Ok(None));
    let hunks = proposal.hunks(buffer.current());
    assert_eq!(hunks[1].id, second);
    assert_eq!(hunks[1].status, HunkStatus::Pending);
    let three = typed.find("    3\n").unwrap();
    assert_eq!(hunks[1].range, three..three + 6);
}

#[test]
fn edited_hunk_goes_stale() {
    let mut buffer = Buffer::new(THREE_FNS);
    let base = buffer.snapshot();
    let proposed = THREE_FNS
        .replace("    1\n", "    10\n")
        .replace("    2\n", "    20\n")
        .replace("    3\n", "    30\n");
    let mut proposal = ProposedChange::from_text(&base, 0..base.len(), &proposed, ai());
    let ids: Vec<_> = proposal.hunks(&base).iter().map(|hunk| hunk.id).collect();
    assert_eq!(ids.len(), 3);

    // The user edits inside the second hunk.
    let two = THREE_FNS.find("    2\n").unwrap();
    buffer.edit([(two + 4..two + 5, "two")], Origin::User);
    let typed = buffer.text().to_string();

    let statuses = |proposal: &ProposedChange, buffer: &Buffer| -> Vec<HunkStatus> {
        let hunks = proposal.hunks(buffer.current());
        hunks.iter().map(|hunk| hunk.status).collect()
    };
    assert_eq!(
        statuses(&proposal, &buffer),
        [HunkStatus::Pending, HunkStatus::Stale, HunkStatus::Pending]
    );
    assert_eq!(
        proposal.accept(&mut buffer, ids[1]),
        Err(ProposalError::Stale)
    );
    assert_eq!(buffer.text().to_string(), typed);

    let (transaction, stale) = proposal.accept_all(&mut buffer);
    assert_eq!(stale, [ids[1]]);
    let expected = typed
        .replace("    1\n", "    10\n")
        .replace("    3\n", "    30\n");
    assert_eq!(buffer.text().to_string(), expected);
    assert_eq!(
        statuses(&proposal, &buffer),
        [
            HunkStatus::Accepted,
            HunkStatus::Stale,
            HunkStatus::Accepted
        ]
    );
    // Each accepted hunk's range is its new text, where it landed.
    let hunks = proposal.hunks(buffer.current());
    let texts = texts_at(&buffer, &hunks);
    assert_eq!([&*texts[0], &*texts[2]], ["    10\n", "    30\n"]);
    // Both hunks went in one transaction.
    assert_eq!(buffer.undo(|origin| origin.is_ai()), Ok(transaction));
    assert_eq!(buffer.text().to_string(), typed);
    assert_eq!(buffer.undo(|origin| origin.is_ai()), Ok(None));

    // A stale hunk isn't pending; it can still be dismissed.
    assert!(proposal.is_resolved());
    assert_eq!(proposal.accept_all(&mut buffer), (None, vec![ids[1]]));
    proposal.reject(ids[1]).unwrap();
    assert_eq!(proposal.reject(ids[1]), Err(ProposalError::NotPending));
    assert!(proposal.is_resolved());
}

#[test]
fn survives_log_compaction() {
    let mut buffer = Buffer::new(THREE_FNS);
    let base = buffer.snapshot();
    let mut proposal = ProposedChange::from_text(&base, 0..base.len(), &rewritten(), ai());
    let second = proposal.hunks(&base)[1].id;

    for round in 0..3 {
        buffer.edit([(0..0, format!("// {round}\n"))], Origin::User);
        proposal.refresh(&buffer.snapshot());
        buffer.compact_log(proposal.version());
        assert_eq!(buffer.current().oldest_version(), proposal.version());
        assert_eq!(proposal.version(), buffer.version());
    }
    // More typing after the last refresh, at the hunk's start.
    let three = buffer.text().to_string().find("    3\n").unwrap();
    buffer.edit([(three..three, "    // note\n")], Origin::User);
    let typed = buffer.text().to_string();

    proposal.accept(&mut buffer, second).unwrap();
    assert_eq!(
        buffer.text().to_string(),
        typed.replace("    3\n", "    30\n")
    );
    let landed = typed.find("    3\n").unwrap();
    assert_eq!(
        proposal.hunks(buffer.current())[1].range,
        landed..landed + 7
    );
}

#[test]
fn from_edits_makes_one_hunk_per_edit() {
    let mut buffer = Buffer::new("abcdef");
    let base = buffer.snapshot();
    // Unsorted, with a no-op and an insertion touching a replacement.
    let edits: Vec<(std::ops::Range<usize>, Arc<str>)> = vec![
        (4..6, "EF".into()),
        (0..1, "a".into()),
        (2..2, "+".into()),
        (2..4, "CD".into()),
    ];
    let mut proposal = ProposedChange::from_edits(&base, edits, ai());
    let hunks = proposal.hunks(&base);
    let ranges: Vec<_> = hunks.iter().map(|hunk| hunk.range.clone()).collect();
    assert_eq!(ranges, [2..2, 2..4, 4..6]);

    // In any order, each lands beside the others.
    for hunk in hunks.iter().rev() {
        proposal.accept(&mut buffer, hunk.id).unwrap();
    }
    assert_eq!(buffer.text().to_string(), "ab+CDEF");
    assert!(proposal.is_resolved());
}

#[test]
#[should_panic(expected = "overlap")]
fn from_edits_rejects_overlaps() {
    let base = Buffer::new("abcdef").snapshot();
    ProposedChange::from_edits(&base, [(0..3, "x"), (2..4, "y")], ai());
}

#[test]
fn an_insertion_lands_after_text_typed_at_its_point() {
    // As `Buffer::edit_from` maps an insertion.
    let mut buffer = Buffer::new("a\nb\n");
    let base = buffer.snapshot();
    let mut proposal = ProposedChange::from_edits(&base, [(2..2, "new\n")], ai());
    let id = proposal.hunks(&base)[0].id;
    buffer.edit([(2..2, "Z")], Origin::User);
    proposal.accept(&mut buffer, id).unwrap();
    assert_eq!(buffer.text().to_string(), "a\nZnew\nb\n");
    // Snapshots from before the accept still show where it was.
    assert_eq!(proposal.hunks(&base)[0].range, 2..2);
    assert_eq!(proposal.hunks(buffer.current())[0].range, 3..7);

    // A hunk of another proposal isn't this one's.
    let other = ProposedChange::from_edits(&buffer.snapshot(), [(0..1, "A")], ai());
    let foreign = other.hunks(buffer.current())[0].id;
    assert_eq!(
        proposal.accept(&mut buffer, foreign),
        Err(ProposalError::NotPending)
    );
}

/// `THREE_FNS` with a caller transaction of `open` origin already open and one
/// edit in it, and a proposal rewriting `one` and `three`.
fn with_open_transaction(open: Origin) -> (Buffer, ProposedChange, fe_buffer::TransactionId) {
    let mut buffer = Buffer::new(THREE_FNS);
    let base = buffer.snapshot();
    let proposal = ProposedChange::from_text(&base, 0..base.len(), &rewritten(), ai());
    let outer = buffer.start_transaction(open.clone());
    buffer.edit([(0..0, "// top\n")], open);
    (buffer, proposal, outer)
}

fn open_origins() -> [Origin; 2] {
    [Origin::User, ai()]
}

#[test]
fn accept_is_independent_of_an_open_transaction() {
    for open in open_origins() {
        let (mut buffer, mut proposal, outer) = with_open_transaction(open.clone());
        let id = proposal.hunks(buffer.current())[0].id;

        let accepted = proposal.accept(&mut buffer, id).unwrap();
        assert_ne!(accepted, outer, "{open:?}");
        assert!(buffer.text().to_string().contains("let x = 1;"));

        // The caller's transaction is still open: more edits join it.
        buffer.edit([(0..0, "// more\n")], open.clone());
        assert_eq!(buffer.end_transaction(), Some(outer), "{open:?}");

        // Undo reverts the accepted hunk alone, then the caller's edits.
        assert_eq!(buffer.undo(|o| *o == ai()).unwrap(), Some(accepted));
        let text = buffer.text().to_string();
        assert!(!text.contains("let x = 1;"), "{open:?}");
        assert!(text.starts_with("// more\n// top\n"), "{open:?}");
    }
}

#[test]
fn accept_all_is_independent_of_an_open_transaction() {
    for open in open_origins() {
        let (mut buffer, mut proposal, outer) = with_open_transaction(open.clone());

        let (accepted, stale) = proposal.accept_all(&mut buffer);
        let accepted = accepted.unwrap();
        assert!(stale.is_empty());
        assert_ne!(accepted, outer, "{open:?}");

        buffer.edit([(0..0, "// more\n")], open.clone());
        assert_eq!(buffer.end_transaction(), Some(outer), "{open:?}");

        // One undo reverts the whole batch, and none of the caller's edits.
        assert_eq!(buffer.undo(|o| *o == ai()).unwrap(), Some(accepted));
        let text = buffer.text().to_string();
        assert!(
            !text.contains("let x = 1;") && !text.contains("    30\n"),
            "{open:?}"
        );
        assert!(text.starts_with("// more\n// top\n"), "{open:?}");
    }
}

#[test]
fn accept_keeps_the_callers_transaction_past_the_history_limit() {
    for open in open_origins() {
        for all in [false, true] {
            let (mut buffer, mut proposal, outer) = with_open_transaction(open.clone());
            // A nested caller transaction: depth must survive the acceptance.
            assert_eq!(buffer.start_transaction(open.clone()), outer);
            buffer.set_history_limit(0);
            let id = proposal.hunks(buffer.current())[0].id;
            let accepted = if all {
                proposal.accept_all_as(&mut buffer, open.clone()).0.unwrap()
            } else {
                proposal.accept_as(&mut buffer, id, open.clone()).unwrap()
            };
            assert_ne!(accepted, outer, "{open:?} all={all}");

            // Subsequent edits still join the caller's transaction.
            assert_eq!(
                buffer.edit([(0..0, "// more\n")], open.clone()),
                Some(outer),
                "{open:?} all={all}"
            );
            // The inner close leaves it open; no history trim has run yet.
            assert_eq!(buffer.end_transaction(), None, "{open:?} all={all}");
            buffer.set_history_limit(Buffer::DEFAULT_HISTORY_LIMIT);
            assert_eq!(buffer.end_transaction(), Some(outer), "{open:?} all={all}");

            // Undo reverts the acceptance, then the caller's transaction
            // with both of its edits.
            assert_eq!(buffer.undo(|o| *o == open).unwrap(), Some(accepted));
            let text = buffer.text().to_string();
            assert!(
                !text.contains("let x = 1;") && !text.contains("    30\n"),
                "{open:?} all={all}"
            );
            assert!(text.starts_with("// more\n// top\n"), "{open:?} all={all}");
            assert_eq!(buffer.undo(|o| *o == open).unwrap(), Some(outer));
            assert_eq!(buffer.text().to_string(), THREE_FNS, "{open:?} all={all}");
        }
    }
}

#[test]
fn stale_observation_is_not_overwritten_by_an_unchanged_snapshot() {
    let mut buffer = Buffer::new(THREE_FNS);
    let base = buffer.snapshot();
    let proposal = ProposedChange::from_text(&base, 0..base.len(), &rewritten(), ai());
    let one = THREE_FNS.find("    1\n").unwrap();
    buffer.edit([(one + 4..one + 5, "9")], Origin::User);
    let edited = buffer.snapshot();

    let stale = |hunks: &[HunkView]| hunks[0].status == HunkStatus::Stale;
    assert!(stale(&proposal.hunks(&edited)));
    // A check of the unchanged snapshot, after the stale one, must not
    // resurrect the hunk: staleness is permanent.
    assert!(stale(&proposal.hunks(&base)));
    assert!(stale(&proposal.hunks(&edited)));

    // Concurrently, in either interleaving.
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                for _ in 0..200 {
                    proposal.hunks(&base);
                    proposal.hunks(&edited);
                }
            });
        }
    });
    assert!(stale(&proposal.hunks(&edited)));
}
