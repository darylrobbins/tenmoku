---
task_uid: "01m4egtxe2tza2ggkk5d2t50qd"
task: "T203"
rev: 3
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "No test under crates/fe-syntax/src, nor in crates/fe-syntax/tests/runaway_parse.rs or crates/fe-syntax/tests/outline.rs, asserts on absolute wall-clock time (a generous hang-guard timeout, at least 60 s, that only fires on a hang is allowed and named in the finish notes); each one converted still fails on a quadratic (or otherwise unbounded) implementation, shown in the finish notes. crates/fe-syntax/tests/more_languages_followups_2.rs lua_long_assignments_scale is left as is (converting it needs a work counter in non-test code) and listed in the finish notes"
    required: true
    manual: false
  - id: "C2"
    text: "null"
    required: true
    manual: false
  - id: "C3"
    text: "null"
    required: true
    manual: false
  - id: "C4"
    text: "null"
    required: true
    manual: false
---

# Make fe-syntax's remaining timing-sensitive tests load-robust, as T193 did

Gates failed repeatedly on 2026-10-08 at load average 100-180 on timing-sensitive fe-syntax tests.
T193 (by the user's decision) makes the two worst ones load-robust:
`imports::tests::a_great_many_jsx_attributes_named_extends_are_read_in_linear_time` and
`tests::imports_recovery::overlapping_lookaheads_are_read_in_linear_time`. Read how T193 did it,
and treat every other test in crates/fe-syntax the same way.

## Change

Sweep `crates/fe-syntax/src/**/tests*.rs` and `crates/fe-syntax/src/tests/**` for:

- asserts on absolute wall-clock time (`Instant`, `elapsed()`, `Duration::from_secs` /
  `from_millis` bounds);
- inputs big enough that a parse could hit the production parse cap under load and turn an
  `unwrap` into a panic.

Convert each the way T193 and T189 did: assert linearity or bounded work with a count (a
test-only counter, as `ARGUMENT_VALIDATIONS` does, or one the code already exposes) and parse with
no deadline, keeping the test failing on a quadratic implementation. List each test changed, or
left as it is with the reason, in the finish notes.
