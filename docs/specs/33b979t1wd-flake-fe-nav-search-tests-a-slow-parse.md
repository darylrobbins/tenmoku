---
task_uid: "01m7mzsybj79q56t33b979t1wd"
task: "T675"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_slow_parse_is_given_up no longer depends on a 1 ms budget expiring in time: the out-of-time step is made certain (expired budget or a deadline seam), and the full-answer and cancelled steps still assert what they did"
    required: true
    manual: false
  - id: "C2"
    text: "The hand-in records at least 50 passing runs of the test under parallel CPU load"
    required: true
    manual: false
---

# Flake: fe-nav search::tests::a_slow_parse_is_given_up fails under load (the 1 ms budget step can answer before the deadline fires) (Mac T1927)

`fe-nav` `search::tests::a_slow_parse_is_given_up` (crates/fe-nav/src/search.rs) failed in the Mac stack 206 gate
(run93, 11-17, load ~28) and passed on a rerun. The "out of time" step builds a navigator with a 1 ms budget and
asserts the answer has no items; under load the step returned an answer with items (the partial/same-file result)
instead of giving up. The assertion depends on the 1 ms budget expiring before the work it bounds finishes, which
timing on a loaded machine doesn't guarantee in either direction.

Make the test deterministic without weakening what it checks: e.g. a zero or already-expired budget, or a test
seam (clock or a forced-deadline hook) so "out of time" is certain rather than raced. Keep the full-answer and
cancelled steps. Test-only unless a real race in the production deadline check turns up (then say so and stop).
Prove it with repeated runs under load (e.g. 50 runs with a parallel CPU load), recorded in the hand-in.

## Helium filing note

Load loops: start your CPU load with its own PID recorded and stop it by that PID only. Never `pkill`/`killall` by name (cargo, cargo-nextest, stress, yes): other lanes' gates share this host, and a name kill on T672 SIGTERMed three of them.
