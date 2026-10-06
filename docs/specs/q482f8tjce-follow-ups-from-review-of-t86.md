---
task_uid: "01m4751pw4t63nmmq482f8tjce"
task: "T87"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T86

Findings of the review of T86 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/lib.rs:1115: Batched catch-up still discards folds that synchronization after each edit preserves. With "top\ndef a():\n    one\n    two\nend" folded over rows 1..=3, replace 4..13 with "renamed():\n", then append "!" below the fold. The replacement preserves the header's row count and touches no hidden row, but touches_rest rejects it because it includes the header's newline. Likewise, replacing 0..4 with "new\nprefix " leaves the original folded text intact, but the final row-start equality rejects its prefixed header. Both paths consequently lose the fold.
  fix: Determine whether edits actually change hidden rows or split/join the header, allowing header-line replacements and prefixes that preserve its row structure. Add both reproductions to the batched-versus-per-edit tests and exercise ordinary synchronization and BufferMut drop.
  instances: crates/fe-editor/src/lib.rs:1115, crates/fe-editor/src/lib.rs:1130

- F2 medium crates/fe-editor/src/lib.rs:1130: Header integrity is checked only in the final text, so catch-up can resurrect a fold invalidated by an earlier batch. Starting with the same text and fold over rows 1..=3, insert a newline at offset 6, delete that newline, then append "!" below the fold before catching up. Synchronizing after each edit removes the fold when its header splits; untouched_rows instead accepts the restored final header and fold_many recreates the fold. The outcome depends on when the view synchronizes.
  fix: Track header split/join invalidation across individual batches and retain that invalidation even when later edits restore the original layout. Ensure the resulting fold state also removes invalid survivors from aggregate display synchronization. Test split/rejoin and join/resplit sequences followed by an edit below the fold through both catch-up paths.
  instances: crates/fe-editor/src/lib.rs:1130, crates/fe-editor/src/lib.rs:1080

- F3 medium crates/fe-editor/src/lib.rs:374: Selection-span classification still happens before catch-up, and selections receiving a supplied candidate are excluded from subsequent span calculation. For b selecting "ąx" in "ąx\n    y\nend", let a prefix "Z" and have b's one-shot width callback insert a newline at byte offset 3 while catching up. Calling b.fold with candidate 0..=1 classifies the selection as single-row, applies the candidate, then loses it when the callback splits its header. Although the current anchored selection now spans rows 0..=1, fold returns with no fold because that selection never entered after_catch_up.
  fix: Recompute selected row spans for every anchored selection after synchronization completes, including selections that received candidates, and give current multiline spans priority. Preserve displayed-snapshot coordinates for supplied candidates. Add a regression combining candidate folding with a sibling-editing width callback.
  instances: crates/fe-editor/src/lib.rs:374, crates/fe-editor/src/lib.rs:382, crates/fe-editor/src/lib.rs:387
