use std::sync::Arc;
use std::time::SystemTime;

use LaneState::*;
use fe_editor::lane::{
    Backing, Hunks, Lane, LaneCounts, LaneEvent, LaneId, LaneKind, LaneState, Lanes, TreeId,
};

const NONE: Hunks = Hunks {
    pending: 0,
    any_accepted: false,
};
const NONE_ACCEPTED: Hunks = Hunks {
    pending: 0,
    any_accepted: true,
};
const SOME: Hunks = Hunks {
    pending: 2,
    any_accepted: false,
};

fn tree(s: &str) -> TreeId {
    TreeId(Arc::from(s))
}

fn open_as(lanes: &mut Lanes, session: &str, kind: LaneKind, backing: Backing) -> LaneId {
    lanes
        .open(
            "n".into(),
            "agent".into(),
            Some(session.into()),
            kind,
            backing,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap()
}

/// Each call gets its own session: "s" for the first, then "s2", "s3"...
fn open(lanes: &mut Lanes, kind: LaneKind, backing: Backing) -> LaneId {
    let number = lanes.next_number().unwrap_or(1);
    let session = if number == 1 {
        "s".to_string()
    } else {
        format!("s{number}")
    };
    open_as(lanes, &session, kind, backing)
}

fn ext() -> LaneKind {
    LaneKind::External { host: "acp".into() }
}

fn step(from: LaneState, event: LaneEvent, hunks: Hunks) -> Option<LaneState> {
    from.next(&event, hunks)
}

#[test]
fn transition_table() {
    // (new) -> Working
    let mut lanes = Lanes::new();
    let id = open(&mut lanes, ext(), Backing::Shared);
    assert_eq!(lanes.get(id).unwrap().state, Working);

    // Working -> PendingReview / Accepted / Rejected on turn end
    assert_eq!(
        step(Working, LaneEvent::TurnEnded, SOME),
        Some(PendingReview)
    );
    assert_eq!(
        step(Working, LaneEvent::TurnEnded, NONE_ACCEPTED),
        Some(Accepted)
    );
    assert_eq!(step(Working, LaneEvent::TurnEnded, NONE), Some(Rejected));

    // PendingReview -> Working
    assert_eq!(
        step(PendingReview, LaneEvent::TurnStarted, SOME),
        Some(Working)
    );
    // PendingReview -> Accepted / Rejected: the last pending hunk decided
    assert_eq!(
        step(PendingReview, LaneEvent::Decided, NONE_ACCEPTED),
        Some(Accepted)
    );
    assert_eq!(
        step(PendingReview, LaneEvent::Decided, NONE),
        Some(Rejected)
    );
    assert_eq!(step(PendingReview, LaneEvent::Decided, SOME), None);

    // Accepted / Rejected -> PendingReview: hunks added outside a turn
    for from in [Accepted, Rejected] {
        assert_eq!(step(from, LaneEvent::HunksAdded, SOME), Some(PendingReview));
        assert_eq!(step(from, LaneEvent::HunksAdded, NONE), None);
        // -> Working: another turn
        assert_eq!(step(from, LaneEvent::TurnStarted, NONE), Some(Working));
    }

    // Working / PendingReview -> Abandoned with hunks pending, else decided
    for from in [Working, PendingReview] {
        assert_eq!(step(from, LaneEvent::Ended, SOME), Some(Abandoned));
        assert_eq!(step(from, LaneEvent::Ended, NONE_ACCEPTED), Some(Accepted));
        assert_eq!(step(from, LaneEvent::Ended, NONE), Some(Rejected));
    }

    // Abandoned -> Accepted / Rejected: its last pending hunk decided
    assert_eq!(
        step(Abandoned, LaneEvent::Decided, NONE_ACCEPTED),
        Some(Accepted)
    );
    assert_eq!(step(Abandoned, LaneEvent::Decided, NONE), Some(Rejected));
    assert_eq!(step(Abandoned, LaneEvent::Decided, SOME), None);
    // Abandoned -> Working: the same session reopened
    assert_eq!(step(Abandoned, LaneEvent::Reopened, SOME), Some(Working));

    // Rows that don't exist.
    assert_eq!(step(Working, LaneEvent::Decided, NONE), None);
    assert_eq!(step(Abandoned, LaneEvent::TurnStarted, SOME), None);
    assert_eq!(step(Accepted, LaneEvent::Ended, NONE), None);
    assert_eq!(step(Working, LaneEvent::Reopened, NONE), None);
}

#[test]
fn reopen_goes_through_the_session() {
    let mut lanes = Lanes::new();
    let id = open(&mut lanes, ext(), Backing::Shared);
    assert_eq!(lanes.reopen("agent", "s"), None, "not abandoned");
    lanes.apply(id, &LaneEvent::Ended, SOME);
    assert_eq!(lanes.get(id).unwrap().state, Abandoned);
    assert_eq!(lanes.reopen("agent", "other"), None);
    assert_eq!(lanes.reopen("other", "s"), None);
    assert_eq!(lanes.reopen("agent", "s"), Some(id));
    assert_eq!(lanes.get(id).unwrap().state, Working);
}

#[test]
fn worktree_rows_by_tree_id() {
    let mut lanes = Lanes::new();
    let id = open(&mut lanes, LaneKind::Worktree, Backing::Worktree);
    let lane = lanes.get_mut(id).unwrap();

    // Nothing changes on a Working lane.
    assert_eq!(lane.observe_tree(&tree("a")), None);
    // Frozen with hunks: turn ends, PendingReview.
    assert_eq!(lane.freeze(tree("a"), SOME), Some(PendingReview));
    // Same tree: nothing.
    assert_eq!(lane.observe_tree(&tree("a")), None);
    // The agent wrote again.
    assert_eq!(lane.observe_tree(&tree("b")), Some(Working));
    assert_eq!(lane.freeze(tree("b"), NONE_ACCEPTED), Some(Accepted));
    assert_eq!(lane.observe_tree(&tree("c")), Some(Working));
    assert_eq!(lane.freeze(tree("c"), NONE), Some(Rejected));
    // Frozen again with new hunks: Rejected -> PendingReview.
    assert_eq!(lane.freeze(tree("d"), SOME), Some(PendingReview));
    // Accepted -> Working on a changed tree.
    lane.state = Accepted;
    assert_eq!(lane.observe_tree(&tree("e")), Some(Working));

    // Abandoned and Working don't react; non-worktree lanes don't either.
    lane.state = Abandoned;
    assert_eq!(lane.observe_tree(&tree("f")), None);
    let other = open(&mut lanes, ext(), Backing::Shared);
    let lane = lanes.get_mut(other).unwrap();
    lane.state = PendingReview;
    assert_eq!(lane.observe_tree(&tree("f")), None);
}

#[test]
fn restart_rules() {
    let chat = LaneKind::Chat;
    for saved in [Working, PendingReview, Accepted, Rejected, Abandoned] {
        assert_eq!(saved.reconciled(&chat, SOME), PendingReview);
        assert_eq!(saved.reconciled(&chat, NONE_ACCEPTED), Accepted);
        assert_eq!(saved.reconciled(&chat, NONE), Rejected);
    }

    // An external lane's agent is gone.
    let e = ext();
    assert_eq!(Working.reconciled(&e, SOME), Abandoned);
    assert_eq!(PendingReview.reconciled(&e, SOME), Abandoned);
    // Even with nothing pending: Abandoned, so its session can reopen.
    assert_eq!(Working.reconciled(&e, NONE), Abandoned);
    assert_eq!(PendingReview.reconciled(&e, NONE_ACCEPTED), Abandoned);
    for saved in [Accepted, Rejected, Abandoned] {
        assert_eq!(saved.reconciled(&e, SOME), saved);
    }

    // A worktree lane is frozen once.
    let w = LaneKind::Worktree;
    for saved in [Working, PendingReview, Accepted, Rejected] {
        assert_eq!(saved.reconciled(&w, SOME), PendingReview);
    }
    assert_eq!(Working.reconciled(&w, NONE_ACCEPTED), Accepted);
    assert_eq!(Accepted.reconciled(&w, NONE_ACCEPTED), Accepted);
    assert_eq!(Rejected.reconciled(&w, NONE), Rejected);
    assert_eq!(Abandoned.reconciled(&w, SOME), Abandoned);

    // Restoring a record reconciles each lane and keeps numbers spent.
    let mut lanes = Lanes::new();
    let c = open(&mut lanes, LaneKind::Chat, Backing::Shared);
    let x = open(&mut lanes, ext(), Backing::Shared);
    let w = open(&mut lanes, LaneKind::Worktree, Backing::Worktree);
    let saved: Vec<Lane> = lanes.iter().cloned().collect();
    let facts = |lane: &Lane| {
        let tree = (lane.kind == LaneKind::Worktree).then(|| tree("b"));
        (SOME, tree)
    };
    let mut restored = Lanes::restore(saved.clone(), Some(7), facts);
    assert_eq!(restored.get(c).unwrap().state, PendingReview);
    assert_eq!(restored.get(x).unwrap().state, Abandoned);
    assert_eq!(restored.get(w).unwrap().state, PendingReview);
    assert_eq!(restored.next_number(), Some(7));
    // The worktree's current tree is its newest revision: no new turn.
    assert_eq!(restored.get_mut(w).unwrap().observe_tree(&tree("b")), None);
    assert_eq!(
        restored.get_mut(w).unwrap().observe_tree(&tree("c")),
        Some(Working)
    );
    // A zero-pending external lane restores Abandoned and reopens.
    let zero = Lanes::restore(saved.clone(), Some(1), |_| (NONE, None));
    assert_eq!(zero.get(x).unwrap().state, Abandoned);
    assert_eq!(zero.next_number(), Some(4));
    let mut zero = zero;
    assert_eq!(zero.reopen("agent", "s2"), Some(x));
    assert_eq!(zero.get(x).unwrap().state, Working);
}

#[test]
fn numbers_are_never_reused_at_the_boundary() {
    let lane = |n: u32| {
        let mut one = Lanes::new();
        let id = open(&mut one, ext(), Backing::Shared);
        let mut lane = one.get(id).unwrap().clone();
        lane.id = LaneId::from_number(n).unwrap();
        lane
    };
    // Restoring the maximum-numbered lane exhausts numbering.
    let mut full = Lanes::restore(vec![lane(u32::MAX)], Some(1), |_| (NONE, None));
    assert_eq!(full.next_number(), None);
    assert!(
        full.open(
            "n".into(),
            "a".into(),
            None,
            ext(),
            Backing::Shared,
            SystemTime::UNIX_EPOCH
        )
        .is_none()
    );
    assert_eq!(full.len(), 1);
    // The last number is given once; exhaustion survives a restore.
    let mut last = Lanes::restore(vec![], Some(u32::MAX), |_| (NONE, None));
    let id = last.open(
        "n".into(),
        "a".into(),
        None,
        ext(),
        Backing::Shared,
        SystemTime::UNIX_EPOCH,
    );
    assert_eq!(id.map(LaneId::get), Some(u32::MAX));
    assert_eq!(last.next_number(), None);
    assert!(
        last.open(
            "n".into(),
            "a".into(),
            None,
            ext(),
            Backing::Shared,
            SystemTime::UNIX_EPOCH
        )
        .is_none()
    );
    assert_eq!(last.len(), 1);
    let again = Lanes::restore(last.iter().cloned().collect(), last.next_number(), |_| {
        (NONE, None)
    });
    assert_eq!(again.next_number(), None);
}

#[test]
fn open_rejects_a_claimed_session() {
    let mut lanes = Lanes::new();
    let first = open_as(&mut lanes, "s", ext(), Backing::Shared);
    let dup = lanes.open(
        "n".into(),
        "agent".into(),
        Some("s".into()),
        ext(),
        Backing::Shared,
        SystemTime::UNIX_EPOCH,
    );
    assert!(dup.is_none());
    assert_eq!(lanes.len(), 1);
    assert_eq!(lanes.next_number(), Some(2), "nothing spent");
    // Another agent may use the same session id.
    assert!(
        lanes
            .open(
                "n".into(),
                "other".into(),
                Some("s".into()),
                ext(),
                Backing::Shared,
                SystemTime::UNIX_EPOCH
            )
            .is_some()
    );
    assert_eq!(lanes.lane_for("agent", "s").unwrap().id, first);
}

#[test]
fn numbering_claims_and_clearing() {
    let mut lanes = Lanes::new();
    let chat = lanes
        .open(
            "Chat".into(),
            "fastedit.chat".into(),
            Some("c1".into()),
            LaneKind::Chat,
            Backing::Shared,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
    let ext_id = open(&mut lanes, ext(), Backing::Shared);
    assert_eq!((chat.get(), ext_id.get()), (1, 2));

    // Claims.
    assert!(lanes.claim(chat, "c2".into()));
    assert!(lanes.claim(chat, "c2".into()));
    assert_eq!(lanes.get(chat).unwrap().sessions.len(), 2);
    assert_eq!(lanes.lane_for("fastedit.chat", "c2").unwrap().id, chat);
    assert_eq!(lanes.lane_for("agent", "s2").unwrap().id, ext_id);
    assert!(lanes.lane_for("fastedit.chat", "s").is_none());
    assert!(lanes.lane_for("fastedit.chat", "s2").is_none());
    assert!(lanes.claim(ext_id, "s2".into()));
    assert!(!lanes.claim(LaneId::from_number(9).unwrap(), "z".into()));
    let third = lanes
        .open(
            "n".into(),
            "agent".into(),
            None,
            ext(),
            Backing::Shared,
            SystemTime::UNIX_EPOCH,
        )
        .unwrap();
    assert!(!lanes.claim(third, "s2".into()), "held by lane 2");

    // Clearing: not the chat, not a live lane; numbers are not reused.
    lanes.apply(chat, &LaneEvent::TurnEnded, NONE);
    assert!(lanes.clear(chat).is_none());
    assert!(lanes.clear(ext_id).is_none(), "working");
    lanes.apply(ext_id, &LaneEvent::Ended, SOME);
    assert_eq!(lanes.clear(ext_id).unwrap().id, ext_id);
    assert!(lanes.get(ext_id).is_none());
    let fourth = open(&mut lanes, ext(), Backing::Shared);
    assert_eq!(fourth.get(), 4);
}

#[test]
fn counts() {
    let mut lanes = Lanes::new();
    assert_eq!(
        lanes.counts(),
        LaneCounts {
            working: 0,
            pending: 0
        }
    );
    let ids: Vec<LaneId> = (0..5)
        .map(|_| open(&mut lanes, ext(), Backing::Shared))
        .collect();
    // all Working
    assert_eq!(
        lanes.counts(),
        LaneCounts {
            working: 5,
            pending: 5
        }
    );
    lanes.apply(ids[0], &LaneEvent::TurnEnded, SOME); // PendingReview
    lanes.apply(ids[1], &LaneEvent::TurnEnded, NONE_ACCEPTED); // Accepted
    lanes.apply(ids[2], &LaneEvent::TurnEnded, NONE); // Rejected
    lanes.apply(ids[3], &LaneEvent::Ended, SOME); // Abandoned
    assert_eq!(
        lanes.counts(),
        LaneCounts {
            working: 1,
            pending: 3
        }
    );
    lanes.apply(ids[3], &LaneEvent::Decided, NONE); // Abandoned -> Rejected
    assert_eq!(
        lanes.counts(),
        LaneCounts {
            working: 1,
            pending: 2
        }
    );
}

#[test]
fn restart_to_counts_excludes_zero_pending_abandoned() {
    for saved in [Working, PendingReview] {
        let mut lanes = Lanes::new();
        let id = open(&mut lanes, ext(), Backing::Shared);
        lanes.get_mut(id).unwrap().state = saved;
        let saved_lanes: Vec<Lane> = lanes.iter().cloned().collect();

        let none = Lanes::restore(saved_lanes.clone(), Some(2), |_| (NONE, None));
        assert_eq!(none.get(id).unwrap().state, Abandoned);
        assert_eq!(
            none.counts(),
            LaneCounts {
                working: 0,
                pending: 0
            }
        );

        let mut some = Lanes::restore(saved_lanes, Some(2), |_| (SOME, None));
        assert_eq!(
            some.counts(),
            LaneCounts {
                working: 0,
                pending: 1
            }
        );
        // Reopening keeps it reopenable and counted as working.
        assert_eq!(some.reopen("agent", "s"), Some(id));
        assert_eq!(
            some.counts(),
            LaneCounts {
                working: 1,
                pending: 1
            }
        );
    }
    // Deciding the last hunk of an Abandoned lane takes it out of the rail.
    let mut lanes = Lanes::new();
    let id = open(&mut lanes, ext(), Backing::Shared);
    lanes.apply(id, &LaneEvent::Ended, SOME);
    assert_eq!(lanes.counts().pending, 1);
    lanes.apply(
        id,
        &LaneEvent::Decided,
        Hunks {
            pending: 1,
            any_accepted: true,
        },
    );
    assert_eq!(lanes.counts().pending, 1, "still undecided");
    lanes.apply(id, &LaneEvent::Decided, NONE_ACCEPTED);
    assert_eq!(lanes.counts().pending, 0);
}

#[test]
fn restore_installs_the_frozen_tree_on_worktree_backed_external_lanes() {
    let mut lanes = Lanes::new();
    let id = open(&mut lanes, ext(), Backing::Worktree);
    lanes.get_mut(id).unwrap().revision = Some(tree("a"));
    lanes.get_mut(id).unwrap().state = Accepted;
    let saved: Vec<Lane> = lanes.iter().cloned().collect();
    let mut restored = Lanes::restore(saved, Some(2), |_| (NONE_ACCEPTED, Some(tree("b"))));
    let lane = restored.get_mut(id).unwrap();
    assert_eq!(lane.revision, Some(tree("b")));
    assert_eq!(lane.observe_tree(&tree("b")), None);
    assert_eq!(lane.state, Accepted);
}
