---
task_uid: "01m4pp8regwv1rtkxnfzf12kt9"
task: "T19"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "cargo bench -p fe-gpui --bench keystroke -- --test runs every case including type/during-parse and move/long-row, and type/during-parse asserts EditorView::syntax().needs_parse() before each timed keystroke"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test keystroke_latency passes unchanged or with only case-lookup edits"
    required: true
    manual: false
  - id: "C3"
    text: "PERF.md's Typing latency section has a run of every case on current main (date, commit, load average) beside the 2026-10-05 baseline, and its closing paragraph no longer says typing during a background parse is left out"
    required: true
    manual: false
---

# Add typing-during-a-background-parse and long-row cases to the keystroke bench and re-run it on current main (Mac T406)

Filed on helium from Mac T406 (~/coordination/helium-cards/T406.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-f.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium note (Mac lead, note f): any numbers you record in PERF.md are measured on helium (Linux): label them so, next to the Mac rows, and never overwrite or remove Mac numbers.

## Problem

- **M2 "UI never blocks on a parse"** is marked "✅ by design" only (PERF.md:73). The keystroke bench
  (`crates/fe-gpui/benches/keystroke/`) settles every background parse before each sample (`Harness::reset`
  → `settle`). So PERF.md:766-767 says its numbers leave out "typing while a background parse runs".
- **M6 p99 < 8 ms** was last measured on 2026-10-05 at `9c08650`, before the keymap engine. PERF.md:764 says
  "re-run the bench after it lands and compare". Nobody has. Vim, LSP sync, diagnostics, find highlights and
  kiln have all landed on the keystroke path since.
- The long-row caret move (`EditorModel::row_at` copies the whole row on every arrow key,
  `crates/fe-editor/src/lib.rs:770`) has no bench case. Its fix, a separate card, needs one to show the
  saving.

## Change

In `crates/fe-gpui/benches/keystroke/harness.rs`, `cases()` gains two cases:

- **`type/during-parse`** types `x` into about 5 MB of generated Rust. That is under Rust's 7.5 MiB parse
  limit (`TREE_BUDGET_BYTES / 40`) and too large to reparse inside the frame's 1 ms budget. For this case,
  `reset` undoes the keystroke and draws one frame, so the reparse moves to the background, but doesn't
  `settle`. The next timed keystroke therefore lands while that parse is pending. Before each sample, the
  harness asserts that `EditorView::syntax().needs_parse()` is true, so the case measures what its name
  says. The harness settles fully every 50 samples, so pending work stays bounded.
- **`move/long-row`** presses `right` in the middle of one 10 MB row of plain ASCII text (no language), as
  `move/10k-rust` does in the Rust file.

Then run the whole bench on current main and replace PERF.md's Typing latency tables with the new run.
Build into a private `CARGO_TARGET_DIR`. Give the date, commit and load average, and put each case's p50 and
p99 next to the 2026-10-05 baseline. Replace the closing paragraph ("This is the baseline before the keymap
engine… and typing while a background parse runs") with what the new run shows. Update
`crates/fe-gpui/tests/keystroke_latency.rs` only if it pins the case count.
