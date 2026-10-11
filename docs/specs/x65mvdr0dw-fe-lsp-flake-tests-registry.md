---
task_uid: "01m4ffqpfkznbxzfx65mvdr0dw"
task: "T256"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp test tests::registry::configuration_answers_and_changes passes 10 of 10 runs under ~2x core-count CPU load (command and counts in the criteria file), and passes in the gate"
    required: true
    manual: false
  - id: "C2"
    text: "The cause is named in the notes: the test's timing assumption, or a production race in fe-lsp (if production, fixed there with a regression test and the file added to scope)"
    required: true
    manual: false
---

# fe-lsp flake: tests::registry::configuration_answers_and_changes sees a superseded launch with no initialize under load

# fe-lsp flake: tests::registry::configuration_answers_and_changes misses a launch's initialize under load

Seen once, in T188's gate (run 01m4ffdrr50ts53e57s5e7qv9k, test stage, 2026-10-09 ~04:43Z, load high):
`crates/fe-lsp/src/tests/registry.rs:685` panicked with "launch 7: exit Some(57), initialize None"
inside `assert_launches_follow_exits`. In the back-to-back restart rounds, one fake server was launched
but never recorded `initialize` before it was superseded. T188 touches only crates/fe-vim, and fe-lsp
last changed in T204; 20 isolated runs pass.

## Change

Find the race between the test's restart rounds and the fake's `initialize` record, and make the test
wait on the condition it asserts (or assert only what a superseded launch guarantees) rather than on
timing. If the race is in production code (crates/fe-lsp/src/registry.rs / restart.rs), say so in the
notes, and fix it there with a regression test; otherwise change only the test.

## Proof

A fix to the test itself can't fail at base on demand, so prove it with repeated runs under load:
cap the load proof at ~10 runs under ~2x core-count load (no 30-run batches, no 84-busy-loop
batches). Record the command and the counts in the criteria file.

## Implementation notes

Cause: the test's assumption, not production. In the back-to-back restart rounds the first launch of a pair (`--aN`) is superseded by `--bN`, possibly before it handshakes, so it legitimately records no `initialize` (and may be killed without recording `exit`). `assert_launches_follow_exits` demanded `initialize` for every launch. It checks the exit-before-initialize order only for a launch that initialized, against a predecessor that completed its handshake (sent `initialized`). Receiving `initialize` is not completing it: a launch superseded before the client consumed the reply is killed with no `exit`. Covered by `a_launch_superseded_mid_handshake_needs_no_exit`.

Proof: `cargo test -p fe-lsp configuration_answers_and_changes` x10 under 28 busy loops (2x 14 cores): 10/10 passed.
