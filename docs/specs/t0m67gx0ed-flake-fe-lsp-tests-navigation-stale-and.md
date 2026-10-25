---
task_uid: "01m5r0442mpy84mrt0m67gx0ed"
task: "T1169"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The cancellation-latency assertion in stale_and_cancelled_requests is bounded against the request's own deadline (well under it, at most 1 s), not a fixed 50 ms; the assertion that a cancelled request answers Err(NavError::Cancelled) and every other assertion in the test are kept, and no skip-under-load guard is added."
    required: true
    manual: false
  - id: "C2"
    text: "Proof: 20 consecutive runs of fe-lsp tests::navigation::stale_and_cancelled_requests (cargo nextest, --retries 0) pass while 3x-ncpu 'yes' burners keep load above 30."
    required: true
    manual: false
---

# Flake: fe-lsp tests::navigation::stale_and_cancelled_requests fails under load (cancelled request answered after 54.6 ms vs a 50 ms wall-clock bound)

`tests::navigation::stale_and_cancelled_requests` (crates/fe-lsp/src/tests/navigation.rs:503) asserts that a cancelled `definitions` request answers `Cancelled` within 50 ms of the cancel flag being set. On the Mac under load (battery, two lanes building), the daemon's fast check of main f92d54fb failed it with "answered after 54.610833ms" (run 01m5qz7h1aw3h6bphfsf8xqzw2, 2026-10-24 22:20Z); the same commit passed 5367/5367 in the stack 57 gate. The 50 ms is a wall-clock bound on thread scheduling, not on the cancel path.

Keep the test's point (cancellation answers promptly, well before the request would time out on its own) but bound it against the request's own deadline rather than a scheduler-sensitive 50 ms: e.g. assert it answered in under a tenth of the navigation deadline the request was made with, or under 1 s if that deadline is longer. Keep every other assertion in the test.
