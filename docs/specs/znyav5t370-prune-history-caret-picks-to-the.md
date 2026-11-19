---
task_uid: "01m7td2s8cxw4gfkznyav5t370"
task: "T1977"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "caret_picks (and sibling maps kept as it is) hold entries only for transactions undo or redo can still reach; pruned when history_revision changes, not on every insert."
    required: true
    manual: false
  - id: "C2"
    text: "New tests in crates/fe-editor/src/caret_picks_prune_tests.rs: bounded past a small history limit, discarded redo's entry dropped, compaction keeps live entries, undo/redo caret placement unchanged."
    required: true
    manual: false
  - id: "C3"
    text: "Existing fe-editor tests pass, including the 70-completion test."
    required: true
    manual: false
---

# Prune History::caret_picks to the transactions undo and redo can still reach (review follow-up of T147 F1)

Review follow-up of T147 F1 (high, overridden at approval): `History::caret_picks` (crates/fe-editor/src/lib.rs ~2080) gets an entry per accepted completion (insert at ~1633) and is never pruned, so it grows without bound in a long session. Entries survive history-limit trimming, discarded redos and log compaction; the comment near 2077 says they are "kept while the buffer lives" because fe-buffer was not known to say when it drops a transaction.

Fix: no fe-buffer change is needed. `Buffer::history_times(|_| true)` lists every transaction undo/redo can still reach, and `Buffer::history_revision()` changes whenever transactions are dropped, trimmed or discarded. Prune `caret_picks` (and any sibling maps documented "kept as caret_picks is", ~2082 and ~2092) to the retained ids, at most once per history_revision change (cheap check at insert time and/or when an undo/redo reads a pick). Keep every undoable and redoable transaction's entry. Update the stale comment.

Tests (new file crates/fe-editor/src/caret_picks_prune_tests.rs, declared with #[cfg(test)] #[path] beside primary_identity_tests): accept many completions past a small history limit and assert the map length stays bounded by the retained transactions; a discarded redo's entry is dropped; log compaction does not drop live entries; undo/redo caret placement for retained transactions is unchanged; the existing 70-completion test still passes.

Type is chore, not bugfix: the leak is private state (`caret_pick` and `caret_picks` are not public), so no base-API regression test can fail at base.

## Implementation notes

- `History::prune_picks(&Buffer)` prunes `caret_picks`, `redo_selections`, `typing_starts` and `change_start_carets` to `history_times(|_| true)` plus `open_transaction()`, once per `history_revision` change. It runs from `pick_caret`, `pick_redo_selections` and the typing insert of `typing_starts`.
