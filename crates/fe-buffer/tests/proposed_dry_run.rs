//! `ProposedChange::dry_run`: what accepting a proposal would do, reported
//! without doing it.

use std::sync::Arc;

use fe_buffer::{Buffer, Conflict, DryRun, HunkId, HunkStatus, Origin, Outcome, ProposedChange};

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

/// `THREE_FNS` with the bodies of `one` and `three` rewritten.
fn rewritten() -> String {
    THREE_FNS
        .replace("    1\n", "    let x = 1;\n    x\n")
        .replace("    3\n", "    30\n")
}

fn proposal(buffer: &Buffer) -> ProposedChange {
    let base = buffer.snapshot();
    ProposedChange::from_text(&base, 0..base.len(), &rewritten(), ai())
}

fn outcomes(run: &DryRun) -> Vec<&Outcome> {
    run.hunks().iter().map(|hunk| &hunk.outcome).collect()
}

/// Asserts the dry run's text and ids match `accept_all` on the same state.
fn assert_matches_accept_all(buffer: &mut Buffer, proposal: &mut ProposedChange) {
    let run = proposal.dry_run(buffer.current());
    let (transaction, stale) = proposal.accept_all(buffer);
    assert_eq!(run.text().to_string(), buffer.text().to_string());
    let clean: Vec<HunkId> = run
        .hunks()
        .iter()
        .filter(|hunk| hunk.outcome == Outcome::Clean)
        .map(|hunk| hunk.id)
        .collect();
    let conflicts: Vec<HunkId> = run
        .hunks()
        .iter()
        .filter(|hunk| matches!(hunk.outcome, Outcome::Conflict(_)))
        .map(|hunk| hunk.id)
        .collect();
    assert_eq!(conflicts, stale);
    assert_eq!(transaction.is_some(), !clean.is_empty());
    let after = proposal.hunks(buffer.current());
    for id in clean {
        let hunk = after.iter().find(|hunk| hunk.id == id).unwrap();
        assert_eq!(hunk.status, HunkStatus::Accepted);
    }
}

#[test]
fn reports_clean_and_changed_hunks() {
    let mut buffer = Buffer::new(THREE_FNS);
    let proposal = proposal(&buffer);
    let expected = proposal.hunks(buffer.current());
    assert_eq!(expected.len(), 2);

    let three = THREE_FNS.find("    3\n").unwrap();
    buffer.edit([(three + 4..three + 5, "three")], Origin::User);

    let run = proposal.dry_run(buffer.current());
    assert_eq!(run.version(), buffer.current().version());
    assert_eq!(run.hunks().len(), 2);
    assert_eq!(run.hunks()[0].id, expected[0].id);
    assert_eq!(run.hunks()[0].outcome, Outcome::Clean);
    assert_eq!(run.hunks()[1].id, expected[1].id);
    assert_eq!(
        run.hunks()[1].outcome,
        Outcome::Conflict(Conflict::Changed {
            expected: Arc::from("    3\n"),
            found: Arc::from("    three\n"),
        })
    );
    let now = proposal.hunks(buffer.current());
    for (hunk, view) in run.hunks().iter().zip(&now) {
        assert_eq!(hunk.range, view.range);
    }
    assert!(!run.applies_cleanly());
    assert_eq!(
        Conflict::WasStale.to_string(),
        "this change went stale earlier"
    );
}

#[test]
fn changes_nothing() {
    let mut buffer = Buffer::new(THREE_FNS);
    let base = buffer.snapshot();
    let proposal = ProposedChange::from_edits(&base, [(15..16, "9")], ai());
    buffer.edit([(15..16, "7")], Origin::User);
    let (text, version) = (buffer.text().to_string(), buffer.current().version());

    let run = proposal.dry_run(buffer.current());
    assert!(matches!(
        outcomes(&run)[..],
        [Outcome::Conflict(Conflict::Changed { .. })]
    ));
    assert_eq!(buffer.text().to_string(), text);
    assert_eq!(buffer.current().version(), version);
    assert!(!proposal.is_resolved(), "no status recorded");
    assert_eq!(
        proposal.hunks(buffer.current())[0].status,
        HunkStatus::Stale
    );

    // Accepted and rejected hunks are resolved, not clean.
    let mut buffer = Buffer::new(THREE_FNS);
    let mut proposal = proposal_two(&buffer);
    let ids: Vec<HunkId> = proposal
        .hunks(buffer.current())
        .iter()
        .map(|hunk| hunk.id)
        .collect();
    proposal.accept(&mut buffer, ids[0]).unwrap();
    proposal.reject(ids[1]).unwrap();
    let run = proposal.dry_run(buffer.current());
    assert_eq!(
        outcomes(&run),
        [
            &Outcome::Resolved(HunkStatus::Accepted),
            &Outcome::Resolved(HunkStatus::Rejected)
        ]
    );
    assert!(run.applies_cleanly());
    assert_eq!(run.text().to_string(), buffer.text().to_string());
}

fn proposal_two(buffer: &Buffer) -> ProposedChange {
    proposal(buffer)
}

#[test]
fn text_matches_accept_all() {
    // (a) clean hunks only.
    let mut buffer = Buffer::new(THREE_FNS);
    let mut p = proposal(&buffer);
    assert!(p.dry_run(buffer.current()).applies_cleanly());
    assert_matches_accept_all(&mut buffer, &mut p);

    // (b) one edited hunk.
    let mut buffer = Buffer::new(THREE_FNS);
    let mut p = proposal(&buffer);
    let one = THREE_FNS.find("    1\n").unwrap();
    buffer.edit([(one + 4..one + 5, "11")], Origin::User);
    assert_matches_accept_all(&mut buffer, &mut p);

    // (c) two insertions whose anchors collapse to one offset.
    let mut buffer = Buffer::new("abcdefgh");
    let base = buffer.snapshot();
    let mut p = ProposedChange::from_edits(&base, [(2..2, "X"), (5..5, "Y")], ai());
    buffer.edit([(2..5, "")], Origin::User);
    let run = p.dry_run(buffer.current());
    assert_eq!(run.hunks()[0].range, run.hunks()[1].range);
    assert_eq!(run.text().to_string(), "abXYfgh");
    assert_matches_accept_all(&mut buffer, &mut p);
    assert_eq!(buffer.text().to_string(), "abXYfgh");

    // (d) recorded stale, then the old text is restored by undo.
    let mut buffer = Buffer::new(THREE_FNS);
    let mut p = proposal(&buffer);
    buffer.edit([(one + 4..one + 5, "11")], Origin::User);
    assert_eq!(p.hunks(buffer.current())[0].status, HunkStatus::Stale);
    buffer.undo(|origin| *origin == Origin::User).unwrap();
    assert_eq!(buffer.text().to_string(), THREE_FNS);
    let run = p.dry_run(buffer.current());
    assert_eq!(
        outcomes(&run),
        [&Outcome::Conflict(Conflict::WasStale), &Outcome::Clean]
    );
    assert_matches_accept_all(&mut buffer, &mut p);
}

#[test]
fn owned_and_thread_safe() {
    fn assert_send_sync<T: Send + Sync + 'static>() {}
    assert_send_sync::<DryRun>();

    let buffer = Buffer::new(THREE_FNS);
    let p = proposal(&buffer);
    assert_eq!(p.buffer(), buffer.current().id());

    let run = p.dry_run(buffer.current());
    drop(p);
    let expected: Vec<_> = run.hunks().to_vec();
    let text = run.text().to_string();
    let (hunks, moved_text) =
        std::thread::spawn(move || (run.hunks().to_vec(), run.text().to_string()))
            .join()
            .unwrap();
    assert_eq!(hunks, expected);
    assert_eq!(moved_text, text);
}
