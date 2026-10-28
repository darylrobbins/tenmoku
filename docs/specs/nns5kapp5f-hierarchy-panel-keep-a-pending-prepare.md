---
task_uid: "01m5s0kbfb9f2982nns5kapp5f"
task: "T242"
rev: 3
type: "chore"
complexity: ""
criteria: []
---

# Hierarchy panel: keep a pending prepare when the direction is toggled, and resolve an unopened target's position against the normally decoded buffer instead of read_to_string, with delayed-prepare and non-UTF-8 tests (T218 F8, F9)

## Implementation notes

The production fixes were already in place (`toggle_direction` restarts a pending prepare; an unopened target is read with `loader::read`). Added tests: a delayed call-hierarchy prepare survives a toggle and lands once in the new direction, and a Latin-1 (non-UTF-8) unopened target opens at the call site in its decoded text. The T240 F1 second-stage focus-test rework was not attempted.
