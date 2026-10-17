---
task_uid: "01m53sfmwsve8kmz85bnk98zy3"
task: "T730"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_format_across_a_save_as uses Lsp::with_fake_clock: no real-clock sleep or delay decides its outcome, and every wait has a finite deadline"
    required: true
    manual: false
  - id: "C2"
    text: "It passes 30 of 30 runs while a workspace cargo test runs alongside (recorded in the spec notes), and keeps asserting what it asserts today"
    required: true
    manual: false
  - id: "C3"
    text: "cargo test -p fe-workbench and clippy -D warnings pass; only lsp_format_tests.rs and this card's spec change"
    required: true
    manual: false
---

# Flake: fe-workbench lsp_format a_format_across_a_save_as relies on a 300 ms real-clock reply delay; drive it with the fake clock

## Problem

`crates/fe-workbench/src/lsp_format_tests.rs` `a_format_across_a_save_as` (from Mac T434 Save As, landed in host
stack 11) gives the fake server a 300 ms reply delay on the real clock, then relies on the save-as happening inside
that window. Under load the window can pass first, so the test can pass or fail on timing. T712's round-3 review
(finding F1, review mycee9) found it; T712 fixed the same pattern in `stale_missing_and_slow`.

## Change

Do what T712 did for `stale_missing_and_slow`: set the test up with `Lsp::with_fake_clock` (via `setup_on`), wait
until the reply delay is armed, start the save-as, wait until it has taken effect, then release the clock. Every wait
keeps a finite deadline. Test-only change.

## Implementation notes

`a_format_across_a_save_as` now uses `setup_on(.., Lsp::with_fake_clock)`: it waits for the request and for the 300 ms sleep to be armed on the fake clock, starts the save-as, waits (`until`, finite deadline) for the moved file to exist, then advances the clock. 30 of 30 runs passed while `cargo test --workspace` ran alongside.
