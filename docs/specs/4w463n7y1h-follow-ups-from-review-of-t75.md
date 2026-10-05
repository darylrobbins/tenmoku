---
task_uid: "01m4734wsnva19ms4w463n7y1h"
task: "T86"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T75

Findings of the review of T75 that don't block the merge (below merge.block_severity):

- F8 medium crates/fe-editor/src/lib.rs:270: Releasing the buffer borrow fixes F7, but width callbacks can now edit through a sibling after the snapshot is captured. Neither refresh path checks the version again. For example, with b's caret after foo, a inserts ą at offset 0; during b.insert("X")'s catch-up, a one-shot width callback inserts Z after ą. b then edits using the snapshot ąfoo against the live text ąZfoo, producing ąZfoXo instead of ąZfooX. Events for the callback edit are also drained despite the cached snapshot remaining behind.
  fix: Recheck the shared version after measuring and finish catch-up before deriving edit positions. Anchor edit/history caret positions before callbacks and resolve them afterward. Add tests whose width callback actually edits through a sibling, covering ordinary refresh and BufferMut drop.
  instances: crates/fe-editor/src/lib.rs:270, crates/fe-editor/src/lib.rs:1035

- F9 medium crates/fe-editor/src/lib.rs:485: Cached selection reads break the existing editor.move completion check. Start with b's caret after foo, insert one character before foo through a, then dispatch left with count=2 through b. The dispatcher records stale offset 3; move_cursors catches up to offset 4 and moves left to 3, so the equality check incorrectly stops after one movement. The expected final offset is 2.
  fix: Catch up before the counted-movement loop records its initial selections, and add a shared-buffer command-dispatch regression for sibling edits that cancel the first movement's offset change.
  instances: crates/fe-editor/src/lib.rs:485, crates/fe-editor/src/commands.rs:445

- F10 medium crates/fe-editor/src/lib.rs:378: fold still derives selection-based row spans before catching up. With b selecting "top\n    one\n", inserting "    extra\n" at the start of the selected second row through a makes b.fold(&|_| None) create the old fold and immediately discard it during synchronization, leaving no fold instead of folding the current selected rows. The fallback also maps every selection start with Bias::Left, although nonempty selection starts are anchored with Bias::Right, so insertion at that boundary can make it fold the wrong block.
  fix: Keep supplied candidates bound to the displayed snapshot, but derive selected row spans and indentation fallback positions from the current anchored selections after catch-up. Test both insertion inside a selected multiline span and insertion at a nonempty selection's start.
  instances: crates/fe-editor/src/lib.rs:378, crates/fe-editor/src/lib.rs:391

- F11 medium crates/fe-editor/src/lib.rs:270: Shared catch-up can discard folds whose text was never edited. DisplayMap::sync collapses multiple edit batches into one spanning row edit. If b folds rows 1..=3, a inserts a line above the fold and then edits the final row below it before b catches up, the aggregate edit spans the fold and removes it. Synchronizing after each edit preserves it. The single-edit C3 test misses this normal shared-buffer case.
  fix: Preserve per-model folds across batched catch-up using their anchored ranges and actual edit intersections, within the allowed crate scope. Cover separated sibling edits above and below an untouched fold through both synchronization paths.
  instances: crates/fe-editor/src/lib.rs:270, crates/fe-editor/src/lib.rs:1035
