---
task_uid: "01m5ja4nzkcsxvtgy6xhz67pt0"
task: "T227"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "quitting_waits_for_slow_servers passes 200 times in a row at load 30+ on argon (about 700 runs passed at load 48), with the 1-s exit-grace step still asserted as at least one 1-s sleep (a stray STDERR_GRACE timer can hide an exactly-one count); the driver prints rust_gone, the armed sleeps and both servers' records when stuck, so a recurrence on the Mac gate names its cause. Documented limit: the failure did not reproduce on argon, so the cause is the likeliest (SHUT_DOWN_WITHIN's 6 s expiring), not proven (Mac lead z46)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench -p fe-lsp passes and cargo clippy -p fe-workbench -p fe-lsp --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# Flake: fe-workbench tests::lsp::quitting_waits_for_slow_servers still fails under load 30+: QuitDriver missed step '1 s: both shut_down, one 1-s sleep' with 6.6 s elapsed (Mac T1082 follow-up)

Flake follow-up of Mac T1082 / argon T217 (8c2a1700), requested by the Mac lead (z45). The T217 fix still fails on the macOS stack-49 gate (top e0db017a, argon batches 35+36 in it) under load 30+.

## Failure
```
FAIL [6.640s] fe-workbench tests::lsp::quitting_waits_for_slow_servers
panicked at crates/fe-workbench/src/tests/lsp.rs:949:13:
QuitDriver missed step: ["1 s: both shut_down, one 1-s sleep"]
```
It passes in isolation (0.13-0.18 s).

## What the failure says
The test ran 6.64 s, less than `QuitDriver::PATIENCE` (10 s), so step 2 was not missed by timeout: `wait()` returned false because `done` was set, i.e. `crate::lsp::shut_down_all` returned before the driver ever saw step 2's state (`have(&shut_down) && rust_gone && sleeps(1 s) == 1`). Either:
1. Quitting can finish without the test clock's 1 s advance (a real-time path or a fallback in production: a REAL BUG; then fix production and give it a regression test; respec this card to `bugfix` with the regression test), or
2. Step 2's state is transient under load (rust_gone, both shut_down and exactly one 1-s sleep never hold together), a test fix.

## Work
Reproduce under load on argon first (stress-ng, or a parallel `cargo nextest run -p fe-workbench -E 'test(quitting_waits_for_slow_servers)' --stress-count`/loop with the machine loaded to load 30+). Find which of the two it is, from the order of the actor's events (shutdown answered/timed out, exit sent, process exit seen, 1-s sleep armed and dropped) versus the driver's polling. Fix the cause. Do not loosen the assertion into a no-op: step 2 (the 1-s exit grace, pyright killed after it) must still be asserted.

Files: `crates/fe-workbench/src/tests/lsp.rs` (the test and QuitDriver), `crates/fe-workbench/src/lsp.rs` / `crates/fe-lsp/src/` only if production needs a fix.
