---
task_uid: "01m4r04zjasdknjncn9xbjzbfn"
task: "T38"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "In crates/fe-workbench/src/tests/lsp.rs, quitting_waits_for_a_held_launch takes its start time before the release thread starts sleeping, its other assertions unchanged, and passes under cargo nextest run --profile ci"
    required: true
    manual: false
  - id: "C2"
    text: "No file outside crates/fe-workbench/src/tests/lsp.rs changes (the lane's docs/specs file excepted)"
    required: true
    manual: false
---

# Flake: quitting_waits_for_a_held_launch starts its timer after the release thread's sleep

Flake seen in the gate of argon's trunk-sync merge 4165373 (rgt ci run 01m4qzp3hmcpa8kfhax73evk99, load ~24): `fe-workbench tests::lsp::quitting_waits_for_a_held_launch` panicked at crates/fe-workbench/src/tests/lsp.rs:948 with `199.160388ms` (asserts `took >= 200ms`). Logged in ~/coordination/flakes.md.

## Cause

The test spawns the release thread (sleep 200 ms, then `hold.set(false)`) and only then takes `let started = Instant::now();`. The sleep starts before the timer does, so `shut_down_all` can return less than 200 ms after `started`, even though it correctly waited for the held launch.

## Fix

Take `started` before spawning the release thread (or measure from inside it), so the lower bound measures what it claims. Keep the 6 s upper bound and every other assertion. Test-only; no source change.
