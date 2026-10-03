---
task_uid: "01m417z3s6jmxydpgyqazr5rkv"
task: "T7"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "Gate stage build compiles the fe-buffer example history_memory"
    required: true
    manual: false
  - id: "C2"
    text: "Release-run numbers for the long session, the replace-all passes and after undo are recorded in the example's module doc (manual)"
    required: true
    manual: false
---

# Measure edit-log and undo-history memory for a long session and a big replace-all

## Problem

`EditLog` (`crates/fe-buffer/src/log.rs`) keeps every edit batch ever applied,
so any old version's anchors can resolve. `BufferSnapshot::edits_since`
(`snapshot.rs:172`) depends on it, as do fe-syntax (`syntax.rs:460`) and
fe-display (`display_map.rs:245`). Undo history (`Buffer::history`,
`buffer.rs`) keeps `old_texts` and `new_texts` (`Vec<Arc<str>>`) for every
transaction. In a long session, or after a replace-all on a big file, memory
only grows.

Nobody has measured how much. The source card says "Measure first", before
choosing among compaction, caps and rope slices (the follow-up cards T-19b and
T-19c).

## Change

Add `crates/fe-buffer/examples/history_memory.rs`. Measure resident bytes the
way `examples/load_memory.rs` does (`ps -o rss=`, sampled while the inputs are
still alive) for:

1. **Long session:** a 1 MB file, then 200k single-character `Origin::User`
   transactions typed at moving positions, with 10% of them deleting a
   character instead. Print the growth in bytes per transaction.
2. **Replace-all:** a 33 MB file, then one batch replacing every occurrence of
   a 6-byte word (about 500k edits) with an 8-byte one, repeated 5 times. Print
   the growth after each pass, in bytes per replaced byte.
3. **Both after undo:** undo everything in each scenario. Report whether memory
   is reclaimed, which it isn't expected to be, since the log keeps the undo
   batches too.

Print one line per scenario. Record the numbers from a release run in the
example's module doc, next to how to run it, as `load_memory.rs` does.

Don't change library code.
