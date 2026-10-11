---
task_uid: "01m4n3fbksrkgmvf0w5ghyg6hh"
task: "T348"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T343

Findings of the review of T343 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/marks.rs:195: Charwise boundary-row protection persists across every later edit batch. `cutting` sets `life.kept`, but replay never clears it. On "abc\ndef\nghi" from (1,1), `mak0vjd` leaves "ef\nghi" and preserves a correctly; an external deletion of 0..3 then incorrectly preserves a even though its surviving row is deleted.
  fix: Scope join protection to the deletion batch it protects, then refresh the surviving row's ordinary lifetime. Test subsequent external deletion after charwise deletes and visual puts.
  instances: crates/fe-vim/src/marks.rs:195, crates/fe-vim/src/marks.rs:510

- F2 medium crates/fe-vim/src/marks.rs:199: Lifetime mapping always uses left bias for an empty row, even when `opening` deliberately changes its row anchor to right bias. On "aa\n\ncc" from (1,0), `makyyjP` produces "aa\naa\n\ncc": the anchor follows the original empty row to row 2, but Life follows the inserted row 1. An external deletion of 5..7 removes both breaks around the original empty row, yet a survives.
  fix: Make lifetime mapping use the same explicit insertion bias as the row anchor, including empty rows adjusted by opening. Add external-edit regressions after linewise P, p and O.
  instances: crates/fe-vim/src/marks.rs:143, crates/fe-vim/src/marks.rs:199

- F3 medium crates/fe-vim/src/marks.rs:223: The new mapper returns at the end of an earlier replacement instead of deferring to a touching edit, unlike fe-buffer's anchor mapper. Set a at (1,1) in "a\nbb\ncc", then externally apply [(0..2, ""), (2..2, "x\n")] in one batch. The anchor remains on bb, but Life tracks x; subsequently deleting 0..2 incorrectly unsets a while bb survives.
  fix: Match fe-buffer's mapping semantics for touching replacements and insertions, preferably through a shared mapping API. Test adjacent edits with both insertion biases and a subsequent row deletion.
  instances: crates/fe-vim/src/marks.rs:223

- F4 medium crates/fe-vim/src/marks.rs:415: Cloned Vim states share Feed but hold independent Place lifetimes. Advancing one state clears batches required by the other, which then falls back to the frozen boundaries this task fixes. For example, set a on bb in "aa\nbb", clone the state, append "\ncc", run a yank through the first state to advance and clear its feed, then externally delete 3..6. The clone misses the append batch and incorrectly retains a.
  fix: Retain batches until every independent consumer has advanced, or give cloned states independent replay subscriptions and histories. Add a regression that advances one clone between consecutive external edits.
  instances: crates/fe-vim/src/marks.rs:69, crates/fe-vim/src/marks.rs:415

- F5 medium crates/fe-vim/src/marks.rs:617: A returned Mark only replays events available when `VimState::mark` is called; it cannot follow subsequent batches despite `Mark::get` explicitly supporting newer snapshots. Capture a's Mark on bb in "aa\nbb", externally append "\ncc" and delete 3..6, then call the captured mark's get with the latest snapshot: it falls back to the original Last anchors and incorrectly reports a surviving mark.
  fix: Let returned Mark handles follow subsequent edit batches with a retained replay history or shared lifetime tracker, while preserving supported snapshot behavior. Test a handle captured before consecutive external edits.
  instances: crates/fe-vim/src/marks.rs:617, crates/fe-vim/src/marks.rs:338
