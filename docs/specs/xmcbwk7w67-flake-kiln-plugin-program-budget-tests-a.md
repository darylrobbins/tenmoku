---
task_uid: "01m6ryqety3kr9dexmcbwk7w67"
task: "T1420"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "program_budget_tests::a_share_that_ends_mid_check_refuses_without_suspending_the_grant passes 30 consecutive runs with CPU burners running (2x ncpu yes \u003e /dev/null)"
    required: true
    manual: false
  - id: "C2"
    text: "only kiln/crates/kiln-plugin/src/program_budget_tests.rs changes; grants.rs and HASH_CHUNK are untouched"
    required: true
    manual: false
---

# Flake: kiln-plugin program_budget_tests::a_share_that_ends_mid_check_refuses_without_suspending_the_grant fails under load (290.5 ms against a 290 ms wall-clock bound)

Flake: kiln-plugin `program_budget_tests::a_share_that_ends_mid_check_refuses_without_suspending_the_grant` fails under load.

Evidence: T1416's gate run 01m6rwje15egxcr3ns3vx2s2f4 (candidate 7c9f4051 on base f8ca28e3; T1416 only edits regatta.toml and TESTING.md) failed only this test, at kiln/crates/kiln-plugin/src/program_budget_tests.rs:214 (the `took <= share + delay + 150 ms` assert) with `290.497709ms` against a 290 ms bound, at load ~45-50. The refusal and the chunk-count assertions passed: the hash was interrupted (read < CHUNKS) and the result was TimedOut, so production behaved as designed; only the wall-clock margin was missed by 0.5 ms.

The bound is too close to the time a completed hash takes: 8 chunks x 40 ms = 320 ms, so the 150 ms slack is all that separates "interrupted" from "finished", and a loaded machine eats it.

Fix (test only, no production change, no regression test): in kiln/crates/kiln-plugin/src/program_budget_tests.rs, in that test, make a full hash take far longer than the bound so the slack can grow: raise CHUNKS from 8 to 32 (a full slow hash is then >= 1.28 s) and the slack from 150 ms to 600 ms (bound share + delay + 600 ms = 740 ms). Keep the TimedOut, `read > 0 && read < CHUNKS`, suspended-is-empty and check_program assertions. Do not change grants.rs, HASH_CHUNK or any non-test file.

Scope: kiln/crates/kiln-plugin/src/program_budget_tests.rs only.

Proof (chore, no regression test): run `cargo nextest run -p kiln-plugin program_budget_tests::a_share_that_ends_mid_check_refuses_without_suspending_the_grant` 30 times, several in parallel with a CPU burner (`yes > /dev/null` x ncpu*2, stopped afterwards), with zero failures, plus the normal kiln-plugin gate.
