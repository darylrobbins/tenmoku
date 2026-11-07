---
task_uid: "01m62r8b5n7a51rx1mrg4ppyts"
task: "T256"
rev: 4
type: "refactor"
complexity: ""
criteria:
  - id: "C1"
    text: "Deadline::check samples the clock (at least every N units of work, N bounded) instead of only reading the cached ran_out flag, so time passing between matcher calls is detected."
    required: true
    manual: false
  - id: "C2"
    text: "Template validation (before the size cap), tilde measurement, expansion, compilation and application each check the deadline; a preview whose replacement work runs past the limit returns None or a truncated result within a small bound of the limit."
    required: true
    manual: false
  - id: "C3"
    text: "Deterministic tests in the new crates/fe-vim/tests/substitute_preview_deadline.rs expire the deadline at each of those stages (a test clock or injected deadline, not a lifted limit with a 5 s allowance) and see the preview give up cleanly with no side effects. Red at base."
    required: true
    manual: false
  - id: "C4"
    text: "Existing tests pass unedited; clippy passes."
    required: true
    manual: false
---

# Make :s preview replacement work observe its time limit: sample the clock in Deadline::check and check it through template validation, tilde measurement, expansion, compilation and application (T238 F1, C2's time-limit clause)

Follow-up to T238's review (F1, high, `crates/fe-vim/src/substitute.rs:438`): replacement work in the `:s` preview does not observe the preview deadline. `Deadline::check()` only reads the cached `ran_out` flag, so time passing between matcher calls goes unnoticed, and template validation, tilde measurement, expansion, compilation and application never check the clock.

The user accepted F1 as a known limit on T238 (findings_override approved 10-29) and asked for this card (the time-limit clause of T238's C2 moved here).

Fix: make `Deadline::check` sample the clock at least every N units of work (N bounded), and check the deadline through template validation (before the size cap), tilde measurement, expansion, compilation and application. A preview that runs past the limit gives `None` or a truncated result within a small bound.

Tests go in the new `crates/fe-vim/tests/substitute_preview_deadline.rs`: expire the deadline deterministically at each stage (a test clock or injected deadline, not a lifted limit with a 5 s allowance) and see the preview give up cleanly with no side effects.
