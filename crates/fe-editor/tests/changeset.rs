//! `Changeset`: one turn's proposals for several files, reviewed together.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
};

use fe_buffer::{Buffer, BufferId, Origin, ProposedChange};
use fe_editor::{BufferSet, Changeset, ChangesetError, RevertOutcome};

const TWO_FNS: &str = "fn one() {\n    1\n}\n\nfn two() {\n    2\n}\n";

#[derive(Default)]
struct Set {
    buffers: HashMap<BufferId, Buffer>,
}

impl BufferSet for Set {
    fn buffer_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.buffers.get_mut(&id)
    }
    fn create(&mut self, _path: &Path, text: &str) -> Option<BufferId> {
        let buffer = Buffer::new(text);
        let id = buffer.id();
        self.buffers.insert(id, buffer);
        Some(id)
    }
    fn remove(&mut self, id: BufferId) -> bool {
        self.buffers.remove(&id).is_some()
    }
}

impl Set {
    fn text(&self, id: BufferId) -> String {
        self.buffers[&id].text().to_string()
    }
}

fn ai() -> Origin {
    Origin::Ai {
        agent: "claude".into(),
        session: "s".into(),
    }
}

/// A buffer of `TWO_FNS` with both bodies proposed to change.
fn file(set: &mut Set, cs: &mut Changeset, path: &str) -> (BufferId, Rc<RefCell<ProposedChange>>) {
    let buffer = Buffer::new(TWO_FNS);
    let id = buffer.id();
    let base = buffer.snapshot();
    let new = TWO_FNS
        .replace("    1\n", "    10\n")
        .replace("    2\n", "    20\n");
    let proposal = Rc::new(RefCell::new(ProposedChange::from_text(
        &base,
        0..base.len(),
        &new,
        ai(),
    )));
    set.buffers.insert(id, buffer);
    cs.add_file(path, id, proposal.clone());
    (id, proposal)
}

fn hunk_ids(set: &Set, id: BufferId, p: &Rc<RefCell<ProposedChange>>) -> Vec<fe_buffer::HunkId> {
    p.borrow()
        .hunks(set.buffers[&id].current())
        .iter()
        .map(|h| h.id)
        .collect()
}

type Proposed = (BufferId, Rc<RefCell<ProposedChange>>);

fn fixture() -> (Set, Changeset, [Proposed; 2]) {
    let mut set = Set::default();
    let mut cs = Changeset::new("turn-1", ai());
    let a = file(&mut set, &mut cs, "a.rs");
    let b = file(&mut set, &mut cs, "b.rs");
    (set, cs, [a, b])
}

#[test]
fn files_listed_in_order_with_counts() {
    let (mut set, mut cs, [(ida, pa), _]) = fixture();
    let ids = hunk_ids(&set, ida, &pa);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[0]).unwrap();
    cs.reject_hunk(&mut set, Path::new("a.rs"), ids[1]).unwrap();
    let files = cs.files(&mut set);
    let paths: Vec<_> = files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(paths, [PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
    let c = files[0].counts;
    assert_eq!((c.hunks, c.pending, c.accepted, c.rejected), (2, 0, 1, 1));
    let c = files[1].counts;
    assert_eq!((c.hunks, c.pending, c.accepted, c.rejected), (2, 2, 0, 0));
}

#[test]
fn summary_totals() {
    let (mut set, mut cs, [(ida, pa), _]) = fixture();
    let ids = hunk_ids(&set, ida, &pa);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[0]).unwrap();
    let s = cs.summary(&mut set);
    assert_eq!(s.files, 2);
    let c = s.counts;
    assert_eq!((c.hunks, c.pending, c.accepted, c.rejected), (4, 3, 1, 0));
}

#[test]
fn accept_one_hunk() {
    let (mut set, mut cs, [(ida, pa), _]) = fixture();
    let ids = hunk_ids(&set, ida, &pa);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[1]).unwrap();
    assert_eq!(set.text(ida), TWO_FNS.replace("    2\n", "    20\n"));
    assert_eq!(
        cs.accept_hunk(&mut set, Path::new("a.rs"), ids[1]),
        Err(ChangesetError::NotPending {
            path: "a.rs".into()
        })
    );
}

#[test]
fn accept_file() {
    let (mut set, mut cs, [(ida, _), (idb, _)]) = fixture();
    let ids = cs.accept_file(&mut set, Path::new("a.rs")).unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(
        set.text(ida),
        TWO_FNS
            .replace("    1\n", "    10\n")
            .replace("    2\n", "    20\n")
    );
    assert_eq!(set.text(idb), TWO_FNS);
}

#[test]
fn accept_all_skips_conflicting_file_and_reports_it() {
    let (mut set, mut cs, [(ida, _), (idb, _)]) = fixture();
    set.buffers
        .get_mut(&ida)
        .unwrap()
        .edit([(15..16, "9")], Origin::User);
    let results = cs.accept_all(&mut set);
    assert_eq!(
        results[0].1,
        Err(ChangesetError::Conflict {
            path: "a.rs".into()
        })
    );
    assert!(results[1].1.is_ok());
    assert_eq!(
        set.text(idb),
        TWO_FNS
            .replace("    1\n", "    10\n")
            .replace("    2\n", "    20\n")
    );
    assert!(set.text(ida).contains("    9\n"));
    // A missing buffer conflicts too.
    set.buffers.remove(&ida);
    assert!(matches!(
        cs.accept_file(&mut set, Path::new("a.rs")),
        Err(ChangesetError::Conflict { .. })
    ));
}

#[test]
fn reject_all_leaves_text_untouched() {
    let (mut set, mut cs, [(ida, pa), (idb, pb)]) = fixture();
    let results = cs.reject_all(&mut set);
    assert!(results.iter().all(|(_, r)| r.is_ok()));
    assert_eq!(set.text(ida), TWO_FNS);
    assert_eq!(set.text(idb), TWO_FNS);
    assert!(pa.borrow().is_resolved() && pb.borrow().is_resolved());
    assert_eq!(cs.summary(&mut set).counts.rejected, 4);
}

#[test]
fn resolved_when_every_hunk_decided() {
    let (mut set, mut cs, [(ida, pa), _]) = fixture();
    assert!(!cs.is_resolved());
    cs.accept_file(&mut set, Path::new("a.rs")).unwrap();
    assert!(!cs.is_resolved());
    cs.reject_file(&mut set, Path::new("b.rs")).unwrap();
    assert!(cs.is_resolved());
    let _ = (ida, pa);
}

#[test]
fn revert_restores_pre_accept_text() {
    let (mut set, mut cs, [(ida, pa), (idb, _)]) = fixture();
    let ids = hunk_ids(&set, ida, &pa);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[0]).unwrap();
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[1]).unwrap();
    cs.accept_file(&mut set, Path::new("b.rs")).unwrap();
    let out = cs.revert(&mut set);
    assert_eq!(
        out,
        [
            RevertOutcome::Reverted {
                path: "a.rs".into(),
                reapplied: true
            },
            RevertOutcome::Reverted {
                path: "b.rs".into(),
                reapplied: true
            },
        ]
    );
    assert_eq!(set.text(ida), TWO_FNS);
    assert_eq!(set.text(idb), TWO_FNS);
}

#[test]
fn revert_after_user_edit_needs_confirm() {
    let (mut set, mut cs, [(ida, _), _]) = fixture();
    cs.accept_file(&mut set, Path::new("a.rs")).unwrap();
    set.buffers
        .get_mut(&ida)
        .unwrap()
        .edit([(0..0, "// hi\n")], Origin::User);
    let before = set.text(ida);
    assert_eq!(
        cs.revert(&mut set),
        [RevertOutcome::NeedsConfirm {
            path: "a.rs".into()
        }]
    );
    assert_eq!(set.text(ida), before);
}

#[test]
fn revert_forced_restores_base() {
    let (mut set, mut cs, [(ida, _), _]) = fixture();
    cs.accept_file(&mut set, Path::new("a.rs")).unwrap();
    set.buffers
        .get_mut(&ida)
        .unwrap()
        .edit([(0..0, "// hi\n")], Origin::User);
    assert_eq!(
        cs.revert_forced(&mut set),
        [RevertOutcome::Reverted {
            path: "a.rs".into(),
            reapplied: true
        }]
    );
    assert_eq!(set.text(ida), TWO_FNS);
}

#[test]
fn revert_makes_hunks_pending_again() {
    let (mut set, mut cs, [(ida, pa), _]) = fixture();
    cs.accept_file(&mut set, Path::new("a.rs")).unwrap();
    cs.revert(&mut set);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.hunks, c.pending, c.accepted), (2, 2, 0));
    assert!(!pa.borrow().is_resolved());
    // And they accept again.
    cs.accept_file(&mut set, Path::new("a.rs")).unwrap();
    assert!(set.text(ida).contains("    10\n"));
}

#[test]
fn revert_with_nothing_accepted_is_noop() {
    let (mut set, mut cs, [(ida, _), _]) = fixture();
    cs.reject_all(&mut set);
    assert!(cs.revert(&mut set).is_empty());
    assert_eq!(set.text(ida), TWO_FNS);
}

#[test]
fn new_file_entry_accept_and_revert() {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    cs.add_new_file("new.rs", "fn n() {}\n");
    assert_eq!(cs.files(&mut set)[0].counts.pending, 1);
    assert!(set.buffers.is_empty());
    cs.accept_file(&mut set, Path::new("new.rs")).unwrap();
    assert_eq!(set.buffers.len(), 1);
    assert_eq!(
        set.buffers.values().next().unwrap().text().to_string(),
        "fn n() {}\n"
    );
    assert_eq!(cs.files(&mut set)[0].counts.accepted, 1);
    assert_eq!(
        cs.revert(&mut set),
        [RevertOutcome::Reverted {
            path: "new.rs".into(),
            reapplied: false
        }]
    );
    assert!(set.buffers.is_empty());
    assert_eq!(cs.files(&mut set)[0].counts.pending, 1);
}

#[test]
fn failed_or_empty_accepts_leave_no_checkpoint() {
    let (mut set, mut cs, [(ida, pa), _]) = fixture();
    let ids = hunk_ids(&set, ida, &pa);
    cs.reject_hunk(&mut set, Path::new("a.rs"), ids[0]).unwrap();
    assert!(cs.accept_hunk(&mut set, Path::new("a.rs"), ids[0]).is_err());
    cs.reject_file(&mut set, Path::new("a.rs")).unwrap();
    assert!(
        cs.accept_file(&mut set, Path::new("a.rs"))
            .unwrap()
            .is_empty()
    );
    set.buffers
        .get_mut(&ida)
        .unwrap()
        .edit([(0..0, "// hi\n")], Origin::User);
    let before = set.text(ida);
    assert!(cs.revert(&mut set).is_empty());
    assert!(cs.revert_forced(&mut set).is_empty());
    assert_eq!(set.text(ida), before);
}

#[test]
fn collapsed_insertions_accept_and_revert() {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    let mut buffer = Buffer::new("a\nb\nc\n");
    let base = buffer.snapshot();
    let p = ProposedChange::from_text(&base, 0..base.len(), "a\nX\nb\nY\nc\n", ai());
    buffer.edit([(2..4, "")], Origin::User);
    let id = buffer.id();
    set.buffers.insert(id, buffer);
    cs.add_file("c.txt", id, Rc::new(RefCell::new(p)));
    cs.accept_file(&mut set, Path::new("c.txt")).unwrap();
    assert_eq!(set.text(id), "a\nX\nY\nc\n");
    cs.revert(&mut set);
    assert_eq!(set.text(id), "a\nc\n");
    assert!(cs.files(&mut set)[0].counts.pending >= 1);
}

#[test]
fn missing_buffer_reports_last_known_counts() {
    let (mut set, mut cs, [(ida, pa), (idb, _)]) = fixture();
    let ids = hunk_ids(&set, ida, &pa);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[0]).unwrap();
    set.buffers.remove(&ida);
    set.buffers.remove(&idb);
    let files = cs.files(&mut set);
    assert!(files[0].missing && files[1].missing);
    assert!(files[0].counts_known && files[1].counts_known);
    assert_eq!(files[1].counts.hunks, 2);
    assert_eq!((files[0].counts.hunks, files[0].counts.accepted), (2, 1));
}

#[test]
fn counts_survive_a_vanished_buffer_after_reject_and_conflict() {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    let buffer = Buffer::new(TWO_FNS);
    let id = buffer.id();
    let base = buffer.snapshot();
    let new = TWO_FNS
        .replace("    1\n", "    10\n")
        .replace("    2\n", "    20\n");
    let p = Rc::new(RefCell::new(ProposedChange::from_text(
        &base,
        0..base.len(),
        &new,
        ai(),
    )));
    set.buffers.insert(id, buffer);
    cs.add_file_seen(&mut set, "a.rs", id, p.clone());
    let ids = hunk_ids(&set, id, &p);
    cs.reject_hunk(&mut set, Path::new("a.rs"), ids[0]).unwrap();
    set.buffers
        .get_mut(&id)
        .unwrap()
        .edit([(35..36, "9")], Origin::User);
    assert!(cs.accept_file(&mut set, Path::new("a.rs")).is_err());
    set.buffers.remove(&id);
    let f = cs.files(&mut set);
    assert!(f[0].missing);
    let c = f[0].counts;
    assert_eq!((c.hunks, c.rejected, c.stale), (2, 1, 1));
}

struct NoRemove(Set);

impl BufferSet for NoRemove {
    fn buffer_mut(&mut self, id: BufferId) -> Option<&mut Buffer> {
        self.0.buffer_mut(id)
    }
    fn create(&mut self, path: &Path, text: &str) -> Option<BufferId> {
        self.0.create(path, text)
    }
}

#[test]
fn new_file_revert_conflicts_when_it_cannot_remove() {
    let mut set = NoRemove(Set::default());
    let mut cs = Changeset::new("t", ai());
    cs.add_new_file("new.rs", "x\n");
    cs.accept_file(&mut set, Path::new("new.rs")).unwrap();
    assert_eq!(
        cs.revert(&mut set),
        [RevertOutcome::Conflict {
            path: "new.rs".into()
        }]
    );
    assert_eq!(cs.files(&mut set)[0].counts.accepted, 1);
    assert_eq!(set.0.buffers.len(), 1);
}

#[test]
fn first_listing_of_an_available_buffer_is_not_missing() {
    let (mut set, mut cs, _) = fixture();
    let files = cs.files(&mut set);
    assert!(files.iter().all(|f| !f.missing && f.counts_known));
    assert_eq!(files[0].counts.hunks, 2);
}

#[test]
fn reject_hunk_on_a_vanished_buffer_conflicts_without_mutating() {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    let (id, p) = file(&mut set, &mut cs, "a.rs");
    let ids = hunk_ids(&set, id, &p);
    let buffer = set.buffers.remove(&id).unwrap();
    assert_eq!(
        cs.reject_hunk(&mut set, Path::new("a.rs"), ids[0]),
        Err(ChangesetError::Conflict {
            path: "a.rs".into()
        })
    );
    set.buffers.insert(id, buffer);
    // Still pending: the failed reject changed nothing.
    cs.reject_hunk(&mut set, Path::new("a.rs"), ids[0]).unwrap();
    assert_eq!(cs.files(&mut set)[0].counts.rejected, 1);
}

#[test]
fn revert_keeps_a_stale_sibling_pending() {
    let (mut set, mut cs, [(ida, pa), _]) = fixture();
    let ids = hunk_ids(&set, ida, &pa);
    // The user edits under the second hunk, making it stale.
    set.buffers
        .get_mut(&ida)
        .unwrap()
        .edit([(35..36, "9")], Origin::User);
    let edited = set.text(ida);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[0]).unwrap();
    assert_eq!(cs.files(&mut set)[0].counts.stale, 1);
    // Nothing else edited since the accept.
    cs.revert(&mut set);
    assert_eq!(set.text(ida), edited);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.hunks, c.pending, c.stale, c.accepted), (2, 2, 0, 0));
}

#[test]
fn accepted_new_file_whose_buffer_vanished_conflicts() {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    cs.add_new_file("new.rs", "x\n");
    cs.accept_file(&mut set, Path::new("new.rs")).unwrap();
    set.buffers.clear();
    assert!(cs.files(&mut set)[0].missing);
    assert!(matches!(
        cs.accept_file(&mut set, Path::new("new.rs")),
        Err(ChangesetError::Conflict { .. })
    ));
    assert!(matches!(
        cs.reject_file(&mut set, Path::new("new.rs")),
        Err(ChangesetError::Conflict { .. })
    ));
}

#[test]
fn plain_add_file_lists_real_counts() {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    let (id, _) = file(&mut set, &mut cs, "a.rs");
    let buffer = set.buffers.remove(&id).unwrap();
    let files = cs.files(&mut set);
    assert!(files[0].counts_known && files[0].missing);
    assert_eq!((files[0].counts.hunks, files[0].counts.pending), (2, 2));
    set.buffers.insert(id, buffer);
    let files = cs.files(&mut set);
    assert!(files[0].counts_known && !files[0].missing);
    assert_eq!(files[0].counts.hunks, 2);
}

#[test]
fn revert_restores_a_stale_noop_sibling_to_pending() {
    let (mut set, mut cs, [(ida, pa), _]) = fixture();
    let ids = hunk_ids(&set, ida, &pa);
    // The user already typed the second hunk's proposed text.
    set.buffers
        .get_mut(&ida)
        .unwrap()
        .edit([(35..36, "20")], Origin::User);
    let edited = set.text(ida);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[0]).unwrap();
    assert_eq!(cs.files(&mut set)[0].counts.stale, 1);
    cs.revert(&mut set);
    assert_eq!(set.text(ida), edited);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.hunks, c.pending, c.stale, c.accepted), (2, 2, 0, 0));
}

#[test]
fn add_file_seen_with_an_unavailable_buffer_keeps_the_initial_counts() {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    let (id, p) = file(&mut set, &mut cs, "a.rs");
    set.buffers.remove(&id);
    cs.add_file_seen(&mut set, "b.rs", id, p);
    let files = cs.files(&mut set);
    assert!(files[1].counts_known && files[1].missing);
    assert_eq!((files[1].counts.hunks, files[1].counts.pending), (2, 2));
}

#[test]
fn a_restored_deletion_already_done_accepts_without_a_transaction() {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    // Hunk 1 deletes "    1\n"; hunk 2 rewrites "2".
    let buffer = Buffer::new(TWO_FNS);
    let id = buffer.id();
    let base = buffer.snapshot();
    let one = TWO_FNS.find("    1\n").unwrap();
    let two = TWO_FNS.find("    2\n").unwrap();
    let proposal = Rc::new(RefCell::new(ProposedChange::from_edits(
        &base,
        vec![(one..one + 6, ""), (two..two + 6, "    20\n")],
        ai(),
    )));
    set.buffers.insert(id, buffer);
    cs.add_file("a.rs", id, proposal.clone());
    // The user already deleted the first line, then the changeset accepts
    // the second hunk, then everything is reverted.
    set.buffers
        .get_mut(&id)
        .unwrap()
        .edit([(one..one + 6, "")], Origin::User);
    let ids = hunk_ids(&set, id, &proposal);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[1]).unwrap();
    cs.revert(&mut set);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.hunks, c.pending), (2, 2));

    // The deletion that is already done accepts one by one without a panic.
    let ids = hunk_ids(&set, id, &proposal);
    assert_eq!(
        cs.accept_hunk(&mut set, Path::new("a.rs"), ids[0]),
        Ok(None)
    );
    // And through accept_file, with the other hunk decided first.
    cs.reject_hunk(&mut set, Path::new("a.rs"), ids[1]).unwrap();
    assert!(cs.accept_file(&mut set, Path::new("a.rs")).is_ok());
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.accepted, c.rejected, c.pending), (1, 1, 0));
}

#[test]
fn accept_file_of_only_restored_noop_deletions_does_not_panic() {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    let buffer = Buffer::new(TWO_FNS);
    let id = buffer.id();
    let base = buffer.snapshot();
    let one = TWO_FNS.find("    1\n").unwrap();
    let two = TWO_FNS.find("    2\n").unwrap();
    let proposal = Rc::new(RefCell::new(ProposedChange::from_edits(
        &base,
        vec![
            (one..one + 6, ""),
            (two..two + 6, "    20\n"),
            (TWO_FNS.len() - 2..TWO_FNS.len() - 2, "// end\n"),
        ],
        ai(),
    )));
    set.buffers.insert(id, buffer);
    cs.add_file("a.rs", id, proposal.clone());
    // The user already deleted the first line; the insertion at the end is
    // accepted, then everything is reverted.
    set.buffers
        .get_mut(&id)
        .unwrap()
        .edit([(one..one + 6, "")], Origin::User);
    let ids = hunk_ids(&set, id, &proposal);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[2]).unwrap();
    cs.revert(&mut set);
    let ids = hunk_ids(&set, id, &proposal);
    cs.reject_hunk(&mut set, Path::new("a.rs"), ids[1]).unwrap();
    cs.reject_hunk(&mut set, Path::new("a.rs"), ids[2]).unwrap();
    // Only the no-op deletion is pending.
    assert!(cs.accept_file(&mut set, Path::new("a.rs")).is_ok());
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.accepted, c.rejected, c.pending), (1, 2, 0));
}

/// Three hunks of `TWO_FNS`: a deletion the user already did (a no-op once
/// restored), a rewrite, and an insertion at the end.
fn noop_fixture() -> (Set, Changeset, BufferId, Rc<RefCell<ProposedChange>>) {
    let mut set = Set::default();
    let mut cs = Changeset::new("t", ai());
    let buffer = Buffer::new(TWO_FNS);
    let id = buffer.id();
    let base = buffer.snapshot();
    let one = TWO_FNS.find("    1\n").unwrap();
    let two = TWO_FNS.find("    2\n").unwrap();
    let end = TWO_FNS.len();
    let proposal = Rc::new(RefCell::new(ProposedChange::from_edits(
        &base,
        vec![
            (one..one + 6, ""),
            (two..two + 6, "    20\n"),
            (end..end, "// end\n"),
        ],
        ai(),
    )));
    set.buffers.insert(id, buffer);
    cs.add_file("a.rs", id, proposal.clone());
    set.buffers
        .get_mut(&id)
        .unwrap()
        .edit([(one..one + 6, "")], Origin::User);
    (set, cs, id, proposal)
}

#[test]
fn noop_accept_then_sibling_then_revert_twice_keeps_all_pending() {
    let (mut set, mut cs, id, p) = noop_fixture();
    let ids = hunk_ids(&set, id, &p);
    assert_eq!(ids.len(), 3);
    // Revert once so the no-op hunk exists (stale deletion rebuilt).
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[2]).unwrap();
    cs.revert(&mut set);
    let ids = hunk_ids(&set, id, &p);
    assert_eq!(cs.files(&mut set)[0].counts.pending, 3);
    // Individually: no-op first, then a real sibling, then revert.
    assert_eq!(
        cs.accept_hunk(&mut set, Path::new("a.rs"), ids[0]),
        Ok(None)
    );
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[1]).unwrap();
    cs.revert(&mut set);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.hunks, c.pending, c.accepted), (3, 3, 0));
    // A second revert of the rebuilt proposal after another accept.
    let ids = hunk_ids(&set, id, &p);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[1]).unwrap();
    cs.revert(&mut set);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.hunks, c.pending), (3, 3));
}

#[test]
fn accepting_only_noop_hunks_still_reverts_to_pending() {
    let (mut set, mut cs, id, p) = noop_fixture();
    let ids = hunk_ids(&set, id, &p);
    cs.accept_hunk(&mut set, Path::new("a.rs"), ids[2]).unwrap();
    cs.revert(&mut set);
    let ids = hunk_ids(&set, id, &p);
    cs.reject_hunk(&mut set, Path::new("a.rs"), ids[1]).unwrap();
    cs.reject_hunk(&mut set, Path::new("a.rs"), ids[2]).unwrap();
    // The batch accepts only the no-op deletion.
    cs.accept_file(&mut set, Path::new("a.rs")).unwrap();
    assert_eq!(cs.files(&mut set)[0].counts.accepted, 1);
    cs.revert(&mut set);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.hunks, c.pending, c.accepted, c.rejected), (3, 3, 0, 0));
}

#[test]
fn accept_all_does_not_record_earlier_turn_edits_in_the_checkpoint() {
    let (mut set, mut cs, [(id, proposal), _]) = fixture();
    let path = Path::new("a.rs");
    cs.arm(&mut set, path);
    // An earlier turn's proposal, still shown by the editor.
    let earlier =
        ProposedChange::from_edits(set.buffers[&id].current(), [(0..0, "// earlier\n")], ai());
    let mut earlier = earlier;
    let hunks = hunk_ids(&set, id, &proposal);
    // The editor's Accept All: each proposal in turn, then the events.
    let buf = set.buffers.get_mut(&id).unwrap();
    earlier.accept_all_as(buf, Origin::User);
    let (transaction, _) = proposal.borrow_mut().accept_all_as(buf, Origin::User);
    for hunk in hunks {
        cs.adopt_accept(&mut set, path, hunk, transaction);
    }
    cs.observe(&mut set);
    let expected = format!("// earlier\n{}", proposed());
    assert_eq!(set.text(id), expected);

    let outcomes = cs.revert(&mut set);
    assert!(matches!(
        outcomes.as_slice(),
        [RevertOutcome::Reverted { .. }]
    ));
    assert_eq!(set.text(id), format!("// earlier\n{TWO_FNS}"));
}

fn proposed() -> String {
    TWO_FNS
        .replace("    1\n", "    10\n")
        .replace("    2\n", "    20\n")
}

/// The file saved, its tab closed and opened again: a new buffer with the
/// text the old one had.
fn reopen(set: &mut Set, cs: &mut Changeset, id: BufferId, path: &str) -> BufferId {
    cs.observe(set);
    let saved = set.text(id);
    set.buffers.remove(&id);
    let buffer = Buffer::new(&saved);
    let new = buffer.id();
    set.buffers.insert(new, buffer);
    cs.rebind(set, Path::new(path), new).unwrap();
    new
}

#[test]
fn checkpoint_and_decisions_survive_save_close_reopen() {
    let counts = |cs: &mut Changeset, set: &mut Set| {
        let c = cs.files(set)[0].counts;
        (c.pending, c.accepted, c.rejected)
    };
    // Accepted and rejected hunks.
    let (mut set, mut cs, [(id, proposal), _]) = fixture();
    let path = Path::new("a.rs");
    let hunks = hunk_ids(&set, id, &proposal);
    cs.accept_hunk(&mut set, path, hunks[0]).unwrap();
    cs.reject_hunk(&mut set, path, hunks[1]).unwrap();
    let saved = set.text(id);
    let id = reopen(&mut set, &mut cs, id, "a.rs");
    assert_eq!(set.text(id), saved);
    assert_eq!(counts(&mut cs, &mut set), (0, 1, 1));
    let outcomes = cs.revert(&mut set);
    assert!(matches!(
        outcomes.as_slice(),
        [RevertOutcome::Reverted { .. }]
    ));
    assert_eq!(set.text(id), TWO_FNS);

    // Everything accepted.
    let (mut set, mut cs, [(id, _), _]) = fixture();
    cs.accept_file(&mut set, path).unwrap();
    let id = reopen(&mut set, &mut cs, id, "a.rs");
    assert_eq!(counts(&mut cs, &mut set), (0, 2, 0));
    cs.revert(&mut set);
    assert_eq!(set.text(id), TWO_FNS);

    // Only rejected: nothing to revert, the decision stays.
    let (mut set, mut cs, [(id, proposal), _]) = fixture();
    cs.arm(&mut set, path);
    let hunks = hunk_ids(&set, id, &proposal);
    cs.reject_hunk(&mut set, path, hunks[1]).unwrap();
    reopen(&mut set, &mut cs, id, "a.rs");
    assert_eq!(counts(&mut cs, &mut set), (1, 0, 1));
}

#[test]
fn a_manually_satisfied_rejected_hunk_survives_reopening_beside_an_accepted_one() {
    let (mut set, mut cs, [(id, proposal), _]) = fixture();
    let path = Path::new("a.rs");
    let hunks = hunk_ids(&set, id, &proposal);
    cs.reject_hunk(&mut set, path, hunks[1]).unwrap();
    // The rejected hunk's replacement typed by hand, then the other accepted.
    let buf = set.buffers.get_mut(&id).unwrap();
    let at = buf.text().to_string().find("    2\n").unwrap();
    buf.edit([(at..at + "    2\n".len(), "    20\n")], Origin::User);
    cs.accept_hunk(&mut set, path, hunks[0]).unwrap();
    let saved = set.text(id);
    let id = reopen(&mut set, &mut cs, id, "a.rs");
    assert_eq!(set.text(id), saved);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.accepted, c.rejected), (1, 1));
}

#[test]
fn mixed_turn_accept_all_survives_save_close_reopen_and_revert() {
    let (mut set, mut cs, [(id, proposal), _]) = fixture();
    let path = Path::new("a.rs");
    cs.arm(&mut set, path);
    let mut earlier =
        ProposedChange::from_edits(set.buffers[&id].current(), [(0..0, "// earlier\n")], ai());
    let hunks = hunk_ids(&set, id, &proposal);
    let buf = set.buffers.get_mut(&id).unwrap();
    earlier.accept_all_as(buf, Origin::User);
    let (transaction, _) = proposal.borrow_mut().accept_all_as(buf, Origin::User);
    for hunk in hunks {
        cs.adopt_accept(&mut set, path, hunk, transaction);
    }
    let saved = set.text(id);
    let id = reopen(&mut set, &mut cs, id, "a.rs");
    assert_eq!(set.text(id), saved);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.pending, c.accepted), (0, 2));
    cs.revert(&mut set);
    assert_eq!(set.text(id), format!("// earlier\n{TWO_FNS}"));
}

#[test]
fn a_foreign_revert_asks_when_an_accepted_hunk_was_edited_and_keeps_its_checkpoint() {
    let (mut set, mut cs, [(id, proposal), _]) = fixture();
    let path = Path::new("a.rs");
    cs.arm(&mut set, path);
    let mut earlier =
        ProposedChange::from_edits(set.buffers[&id].current(), [(0..0, "// earlier\n")], ai());
    let hunks = hunk_ids(&set, id, &proposal);
    let buf = set.buffers.get_mut(&id).unwrap();
    earlier.accept_all_as(buf, Origin::User);
    let (transaction, _) = proposal.borrow_mut().accept_all_as(buf, Origin::User);
    for hunk in hunks {
        cs.adopt_accept(&mut set, path, hunk, transaction);
    }
    // An accepted hunk edited afterwards.
    let buf = set.buffers.get_mut(&id).unwrap();
    let at = buf.text().to_string().find("    10\n").unwrap();
    buf.edit([(at..at + 7, "    11\n")], Origin::User);
    let outcomes = cs.revert(&mut set);
    assert!(matches!(
        outcomes.as_slice(),
        [RevertOutcome::NeedsConfirm { .. }]
    ));
    // Confirmed: what can be undone is, and the checkpoint stays for the rest.
    let outcomes = cs.revert_forced(&mut set);
    assert!(matches!(
        outcomes.as_slice(),
        [RevertOutcome::Conflict { .. }]
    ));
    assert!(set.text(id).contains("    11\n"));
    assert!(!set.text(id).contains("    20\n"));
    assert!(matches!(
        cs.revert(&mut set).as_slice(),
        [RevertOutcome::NeedsConfirm { .. }]
    ));
}

#[test]
fn text_saved_outside_the_hunks_is_asked_about_after_reopening() {
    let (mut set, mut cs, [(id, proposal), _]) = fixture();
    let path = Path::new("a.rs");
    let hunks = hunk_ids(&set, id, &proposal);
    cs.accept_hunk(&mut set, path, hunks[0]).unwrap();
    let buf = set.buffers.get_mut(&id).unwrap();
    buf.edit([(0..0, "// mine\n")], Origin::User);
    let id = reopen(&mut set, &mut cs, id, "a.rs");
    let outcomes = cs.revert(&mut set);
    assert!(matches!(
        outcomes.as_slice(),
        [RevertOutcome::NeedsConfirm { .. }]
    ));
    assert!(set.text(id).starts_with("// mine\n"));
}

#[test]
fn reopening_a_file_with_collapsed_or_stale_hunks_is_refused_untouched() {
    let (mut set, mut cs, [(id, proposal), _]) = fixture();
    let path = Path::new("a.rs");
    let hunks = hunk_ids(&set, id, &proposal);
    cs.accept_hunk(&mut set, path, hunks[0]).unwrap();
    // The whole text deleted: both remaining ranges collapse to one offset.
    let buf = set.buffers.get_mut(&id).unwrap();
    let len = buf.len();
    buf.edit([(0..len, "")], Origin::User);
    cs.observe(&mut set);
    set.buffers.remove(&id);
    let buffer = Buffer::new("");
    let new = buffer.id();
    set.buffers.insert(new, buffer);
    assert!(cs.rebind(&mut set, path, new).is_err());
    assert_eq!(set.text(new), "");
}

#[test]
fn an_arbitrary_manual_replacement_of_a_rejected_hunk_survives_reopening() {
    let (mut set, mut cs, [(id, proposal), _]) = fixture();
    let path = Path::new("a.rs");
    let hunks = hunk_ids(&set, id, &proposal);
    cs.reject_hunk(&mut set, path, hunks[1]).unwrap();
    let buf = set.buffers.get_mut(&id).unwrap();
    let at = buf.text().to_string().find("    2\n").unwrap();
    buf.edit(
        [(at..at + "    2\n".len(), "    something else\n")],
        Origin::User,
    );
    cs.accept_hunk(&mut set, path, hunks[0]).unwrap();
    let saved = set.text(id);
    let id = reopen(&mut set, &mut cs, id, "a.rs");
    assert_eq!(set.text(id), saved);
    let c = cs.files(&mut set)[0].counts;
    assert_eq!((c.accepted, c.rejected), (1, 1));
    cs.revert(&mut set);
    assert!(set.text(id).contains("    something else\n"));
    assert!(set.text(id).contains("    1\n"));
}
