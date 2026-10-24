---
task_uid: "01m5nskst0v581m6p9xa2r3jd4"
task: "T240"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "ProposedChange reports its hunk count without a BufferSnapshot, and a decided hunk can be returned to pending, with tests in fe-buffer."
    required: true
    manual: false
  - id: "C2"
    text: "Changeset::add_file records the proposal's real counts: files() lists them with counts_known = true."
    required: true
    manual: false
  - id: "C3"
    text: "After revert every hunk of a file, including a stale one whose new text already equals the proposed text, is pending again (test)."
    required: true
    manual: false
  - id: "C4"
    text: "The two documented-limit paragraphs in changeset.rs's module docs are removed."
    required: true
    manual: false
---

# Changeset: real hunk counts from plain add_file and revert restoring stale no-op hunks to pending, via a snapshot-free count and a reset-to-pending in ProposedChange (T233 documented limits)

## Problem
T233 (changeset model) ships two documented limits because `ProposedChange` cannot do two things from fe-editor alone:
1. `ProposedChange` exposes no hunk count without a `BufferSnapshot`, so plain `Changeset::add_file` records `counts_known = false` instead of the real counts (only `add_file_seen`, which reads the buffer, has them).
2. `ProposedChange::from_edits` drops no-op edits and a hunk cannot be reset to pending, so after `revert`, a stale hunk whose new text already equals its proposed text (or a stale deletion already done) is not rebuilt as pending.

## Change
In `crates/fe-buffer` (the `proposed_change` module), add a snapshot-free hunk-count accessor and a way to reset a decided hunk to pending (or to rebuild a proposal that keeps no-op edits). Use them in `crates/fe-editor/src/changeset.rs`: `add_file` records the real initial counts, and `revert` restores every hunk, stale or not, to pending. Remove the two documented-limit paragraphs from the module docs.

## Tests
Extend `crates/fe-buffer/tests/proposed_change.rs` for the new API and `crates/fe-editor/tests/changeset.rs` with: a file added by plain `add_file` lists real counts; a stale no-op sibling is pending again after revert.

## Implementation notes

The module is `crates/fe-buffer/src/proposed.rs` (the scope names `proposed_change.rs`, which does not exist). Added `ProposedChange::hunk_count`, `status_count`, `reset_to_pending` and `from_edits_keeping_noops`; revert rebuilds with the last so stale no-op hunks come back pending.
