---
task_uid: "01m76s8va1s2dqpdb527h2jenw"
task: "T353"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Tests (cargo nextest run -p fe-workbench quickfix_fill) pass: a cancellation raised partway through each of the preview-column, fallback and item-traversal phases stops the fill before the remaining items are processed, driven by a counter or cancel flag, with no wall-clock thresholds"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist)"
    required: true
    manual: false
---

# Quickfix fill: periodic cancellation through preview, fallback and item traversal (Mac T1494 follow-up)

# Quickfix fill: periodic cancellation through preview, fallback and item traversal (follow-up of Mac T1494, review F3)

Mac T1494 (argon T321) reads each quickfix line once, safely (`open_file_under`, non-regular files refused, one read and decode per line). Its C3 is narrowed to that, and this card is the known limit it names: the periodic cancellation checks codex asked for in review round 3 (finding F3).

## Change (`crates/fe-workbench/src/quickfix_fill.rs`, plus a new test file or the module's own unit tests)

While the quickfix fill collects preview columns, takes the fallback path, and walks the items, it checks for cancellation periodically (per item and inside long per-line loops), so a cancelled fill stops promptly instead of finishing its traversal. Add deterministic tests (no wall-clock thresholds): a cancellation raised partway through each of the three phases stops the fill before it processes the remaining items, driven by a counter or a cancel flag rather than timing. If a test needs non-public helpers, put it in `quickfix_fill.rs`'s own test module (a new test file that needs a new public export cannot compile on finish's base regression run).

Platform: **Linux-OK**.
