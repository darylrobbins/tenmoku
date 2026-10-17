---
task_uid: "01m53v57jpvxs14kcr19f5cab9"
task: "T133"
rev: 2
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "reparsed returns Some whenever its seed parse and catch-up finish before its deadline, however long the seed took; it still returns None once the deadline passes"
    required: true
    manual: false
  - id: "C2"
    text: "the regression test in auto_indent_host_tests.rs fails at base on every run and passes with the fix; a_stale_tree_is_parsed_within_the_time and a_slow_seed_stops_at_the_deadline still pass"
    required: true
    manual: false
---

# Flake: fe-gpui auto_indent_host_tests a_stale_tree_is_parsed_within_the_time: reparsed gives up once its seed parse passes parse_until's 20 ms last_reparse budget, whatever its deadline (T119 follow-up)

## Problem

`reparsed` (crates/fe-gpui/src/commands.rs:393-403, from T119) is meant to stop only at its `deadline`. But after the
seed parse it calls `caught_up.parse_until(INDENT_PARSE_BUDGET, out_of_time)`. `parse_until` returns false at once when
the copy's `last_reparse` exceeds the budget (crates/fe-syntax/src/syntax.rs:253), and `finish` has just set
`last_reparse` to the seed's own time (syntax.rs:350-351). So whenever the seed takes over 20 ms, `reparsed` returns
`None`, whatever deadline it was given.

Seen as a flake: `commands::auto_indent_host_tests::a_stale_tree_is_parsed_within_the_time` panicked at :121
("in time") after 1.56 s against a 60 s deadline, in the gate of helium sync 5cf25e3 (run 01m53t90yj93gkg8htxc2bxdjk).
Reproduced on a2a27be, debug build, machine load about 45: it failed 1 time in 20 run alone, and 14 times in 24 with 24
copies running at once.

## Fix

In `reparsed`, finish the copy's catch-up within `deadline` only. Drop `parse_until` and its `last_reparse` budget check:
`!caught_up.needs_parse() || caught_up.parse_job().parse(out_of_time)` and then `finish`, or an equivalent. Triage
checked that change: the test passed 48 times in 48 under the same 24-copy load, and all of fe-gpui's auto_indent and
`commands::` tests passed. In production the deadline is the same 20 ms (commands.rs:290), so behaviour there barely
changes. Update `reparsed`'s doc comment if it changes what it promises.

## Regression test

Add it to the existing crates/fe-gpui/src/auto_indent_host_tests.rs, not a new module. Make the seed parse reliably take
more than 20 ms in a debug build, for example with a larger file or text the seed must reparse around errors, and give
`reparsed` a long deadline (60 s). Assert `reparsed(..).is_some()` with assert!/assert_eq!, not expect()/unwrap(),
so the base failure reads as an assertion. It must fail at base on every run: show 10 of 10 runs failing at base in the
finish notes.
