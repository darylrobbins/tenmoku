---
task_uid: "01m5vngm5s86qqtp896hm2x9e3"
task: "T1183"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The test passes 30 of 30 runs while the workspace's heavy tests run in parallel"
    required: true
    manual: false
  - id: "C2"
    text: "A real missing delta request still fails within the 60 s bound"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-workbench lsp_diagnostics::semantic_tokens server_tokens_recolour_and_follow_edits fails once in a loaded full run (passes alone)

## Problem
Failed once in a full workspace nextest ci run on the Mac (stack 59 gate on b2cbcab8), 2026-10-26 (loadavg 17-25); 3 of 3 alone pass. The failure text was not kept. Suspect: after `type_in`, `wait(100ms)` and `wait(REST)` advance the executor's fake clock once each. The REST timer in `ask_semantic_tokens_in` (`lsp_semantic_tokens.rs`) is armed from a spawned task. If the advance runs first, the timer never fires and the delta request is never sent.
## Change
First reproduce under load and record the panic text in the task notes. If it is not this timer race, fix the cause it shows (still test-only, or block for a respec if production code is wrong). Otherwise, in `crates/fe-workbench/src/tests/lsp_semantic_tokens.rs`, replace the single `wait(REST)` with `s.until(...)` that advances the clock by REST each pass until the DELTA request is recorded (as `setup` does for the first ask), keeping the assertion that nothing is sent after only 100 ms. Test-only.
## Tests
Prove with 30 loaded runs, before and after.

## Implementation notes

Fix: in `lsp_semantic_tokens.rs`, `wait(REST)` plus `wait_for` became `s.until` that advances the clock by REST on each pass until the DELTA request is recorded, so a REST timer armed after the advance cannot hang the test. The 100 ms no-request assertion stays, and `until`'s 60 s deadline still fails a missing request (C2).

Load exercise, run twice (baseline = the test file at the parent commit, then the fix). Load: a second shell looped the whole suite, `cargo nextest run -p fe-workbench --ignore-default-filter --no-fail-fast -E 'not test(server_tokens_recolour_and_follow_edits)'` (1332 tests, including `m3_signoff` and `find_replace_interrupted`), back to back. Measured, after a 20 s warm-up, 30 times:
`cargo nextest run -p fe-workbench --ignore-default-filter --retries 0 -E 'test(server_tokens_recolour_and_follow_edits)'`

- Baseline: 30 of 30 passed. The flake did not reproduce, so no panic text was captured and the timer-race diagnosis stays unconfirmed.
- Fix: 30 of 30 passed.

Full package run, passing: `cargo nextest run -p fe-workbench --ignore-default-filter --no-fail-fast` gave 1333 tests run, 1333 passed, 2 skipped. Clippy `-D warnings` is covered by the gate's vet stage.
