---
task_uid: "01m4n6nbv6jg4ey7tx2xrbr12j"
task: "T350"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T348

Findings of the review of T348 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-vim/src/marks.rs:659: Mark::get replays every available batch, including batches newer than the requested snapshot. This makes results for an immutable snapshot change after unrelated edits. Capture a handle on bb in "aa\nbb", append "\ncc", delete 3..6, and save snapshot S: handle.get(&S) returns None. Append "x" at 5..5, then query S again: replay advances past S, Place::position falls back to the original Last anchors, and the deleted mark incorrectly returns Some((1, 1)). Eager replay in VimState::mark also discards the lifetime state needed for intermediate snapshots.
  fix: Preserve a replay baseline and replay only through the requested snapshot's version; avoid eagerly replacing that baseline with the latest lifetime in VimState::mark. Add a regression querying the same saved snapshot before and after later edits.
  instances: crates/fe-vim/src/marks.rs:659, crates/fe-vim/src/marks.rs:705

- F2 medium crates/fe-vim/src/marks.rs:108: Every named or jump mark read registers a new consumer, but dropped consumers are removed only by Feed::clear. Marks::advance skips clear when there are no batches, so repeatedly reading or jumping to marks in an unchanged buffer grows consumers indefinitely. Each retained Weak also keeps its AtomicUsize allocation alive after the returned handle is dropped.
  fix: Prune expired consumers during registration or another operation reached without edits, or unregister them on drop. Add a regression repeatedly creating and dropping mark handles without buffer edits and verify that consumer storage remains bounded.
  instances: crates/fe-vim/src/marks.rs:108, crates/fe-vim/src/marks.rs:172
