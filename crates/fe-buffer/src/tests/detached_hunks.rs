use super::*;

fn hunk(at: Option<Range<usize>>, old: &str, new: &str, stale: bool) -> DetachedHunk {
    DetachedHunk {
        at,
        old_text: old.into(),
        new_text: new.into(),
        stale,
    }
}

#[test]
fn sticky_stale_hunk_rejects_never_accepts_and_survives_refresh() {
    let mut buffer = Buffer::new("one\ntwo\nthree\n");
    let mut change = ProposedChange::from_detached(
        buffer.current(),
        vec![
            hunk(Some(4..7), "two", "2", true),
            hunk(None, "three", "3", false),
            hunk(Some(0..3), "one", "1", false),
        ],
        ai(),
    )
    .unwrap();
    let ids: Vec<_> = change
        .hunks(buffer.current())
        .iter()
        .map(|h| h.id)
        .collect();
    let statuses = |change: &ProposedChange, buffer: &Buffer| {
        change
            .hunks(buffer.current())
            .iter()
            .map(|h| h.status)
            .collect::<Vec<_>>()
    };
    let stale = HunkStatus::Stale;
    assert_eq!(
        statuses(&change, &buffer),
        [stale, stale, HunkStatus::Pending]
    );
    assert_eq!(
        change.accept(&mut buffer, ids[0]),
        Err(ProposalError::Stale)
    );
    assert_eq!(
        change.accept(&mut buffer, ids[1]),
        Err(ProposalError::Stale)
    );
    change.refresh(buffer.current());
    assert_eq!(
        statuses(&change, &buffer),
        [stale, stale, HunkStatus::Pending]
    );
    let (_, skipped) = change.accept_all(&mut buffer);
    assert_eq!(skipped, [ids[0], ids[1]]);
    assert_eq!(buffer.text().to_string(), "1\ntwo\nthree\n");
    change.refresh(buffer.current());
    assert_eq!(change.reject(ids[0]), Ok(()));
    assert_eq!(change.reject(ids[1]), Ok(()));
    assert_eq!(change.status_count(HunkStatus::Rejected), 2);
    assert_eq!(
        change.accept(&mut buffer, ids[0]),
        Err(ProposalError::NotPending)
    );
}

#[test]
fn mismatched_placement_is_refused() {
    let buffer = Buffer::new("one\ntwo\n");
    let refused = ProposedChange::from_detached(
        buffer.current(),
        vec![
            hunk(Some(0..3), "one", "1", false),
            hunk(Some(4..7), "TWO", "2", false),
        ],
        ai(),
    );
    assert_eq!(refused.unwrap_err(), DetachedMismatch { index: 1 });
    let outside = ProposedChange::from_detached(
        buffer.current(),
        vec![hunk(Some(6..20), "o\n", "x", false)],
        ai(),
    );
    assert_eq!(outside.unwrap_err(), DetachedMismatch { index: 0 });
}

#[test]
fn reset_never_makes_a_sticky_stale_hunk_acceptable() {
    let mut buffer = Buffer::new("one\ntwo\n");
    let mut change = ProposedChange::from_detached(
        buffer.current(),
        vec![
            hunk(Some(0..3), "one", "1", true),
            hunk(None, "", "ins\n", false),
        ],
        ai(),
    )
    .unwrap();
    let ids: Vec<_> = change
        .hunks(buffer.current())
        .iter()
        .map(|h| h.id)
        .collect();
    for id in ids {
        change.reset_to_pending(id).unwrap();
        assert_eq!(change.accept(&mut buffer, id), Err(ProposalError::Stale));
        change.reject(id).unwrap();
        change.reset_to_pending(id).unwrap();
        assert_eq!(change.accept(&mut buffer, id), Err(ProposalError::Stale));
    }
    assert_eq!(change.status_count(HunkStatus::Stale), 2);
    assert_eq!(buffer.text().to_string(), "one\ntwo\n");
}
