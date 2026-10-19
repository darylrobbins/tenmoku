---
task_uid: "01m58shgb1hdaw9rsp0x6w8m8c"
task: "T207"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-gpui test commands::auto_indent_host_tests::a_slow_seed_still_catches_up_by_a_long_deadline passes 20 of 20 runs (cargo nextest run -p fe-gpui a_slow_seed_still_catches_up_by_a_long_deadline with --retries 0 repeated, and under cargo nextest -j high load), and its assertions no longer compare an elapsed time against a fixed lower bound"
    required: true
    manual: false
  - id: "C2"
    text: "The other tests in commands::auto_indent_host_tests keep passing unchanged, including the short-deadline test; cargo nextest run -p fe-gpui passes; cargo clippy -p fe-gpui --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Flake: fe-gpui a_slow_seed_still_catches_up_by_a_long_deadline asserts a wall-clock lower bound that a fast reparse breaks (6.9µs is not slow)

## Problem

`fe-gpui commands::auto_indent_host_tests::a_slow_seed_still_catches_up_by_a_long_deadline` (`crates/fe-gpui/src/auto_indent_host_tests.rs:~172-192`) failed once at the helium gate (2026-10-18, batch 69 first run, `test.fastedit.log` line 788) with `6.913µs is not slow`, then passed on the rerun and on the identical tree on main. The test builds a 8000-copy malformed Rust buffer, edits it, calls `reparsed(&syntax, &snapshot, now + 60 s)` (`crates/fe-gpui/src/commands.rs:394`) and asserts `took > 20 ms` as proof that the seed was slow, then that a copy came back and needs no parse.

A reparse that returns in 7 µs means `reparsed` did no seeding work that run, so the "slow" precondition did not hold: the assertion measures the machine, not the behaviour. Find why the work can be skipped (shared or cached parse state, a tree already current, a deadline that expires first) and make the test deterministic.

## Change

Make the test prove "a long deadline still gets the copy" without a wall-clock lower bound: assert on what the call did (a copy was returned, it needs no parse, and the seed ran, via a counter or the syntax's own state), or force the slow precondition deterministically. Keep the 20 ms-deadline behaviour (a short deadline gives no copy) covered by its existing test. Only test code and, if the cause needs it, the smallest change in `commands.rs`.
