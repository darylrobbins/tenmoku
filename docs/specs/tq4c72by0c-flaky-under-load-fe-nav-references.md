---
task_uid: "01m4m9847mbwv63mtq4c72by0c"
task: "T30"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The test still fails if find() re-decodes the line from its start for each hit (argued in the finish notes from the bound's margin), and its other assertions are unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "The timing assertion no longer uses an absolute 5-s wall clock that gate load can exceed"
    required: true
    manual: false
  - id: "C3"
    text: "3 consecutive runs of the test (nextest -E 'test(=a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work)') pass, with each run's 1-min load average logged in the finish notes"
    required: true
    manual: false
  - id: "C4"
    text: "Of the worker's own changes, only crates/fe-nav/tests/references_encoding.rs changes; Regatta's spec file in docs/specs/ for this task is not the worker's change"
    required: true
    manual: false
---

# Flaky under load: fe-nav references_encoding::a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work: drop the absolute 5-s wall clock

## Problem

`fe-nav::references_encoding::a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work`
(`crates/fe-nav/tests/references_encoding.rs` ~103-129) asserts `elapsed.as_secs() < 5` around `find()` on a
3 MB legacy-encoded line with 2,000 hits. Alone, in the debug test build, it takes about 2.55 s, so it has
less than 2x headroom. Under gate load it goes over: T22's finish gate (candidate 01m4m7yyqc071dnfwzr1zsnenc,
integrated a331699, 2026-10-11 ~01:18Z) failed at 5.36 s while another test in the same suite ran past 60 s.
It has failed the same way in 3 gate runs under `~/.local/state/regatta/runs/`.

The test means to catch re-decoding the line from its start for each hit, which is quadratic. That would be
hundreds to thousands of times slower than one decode, not 2x. An absolute 5-s wall clock on a loaded
12-core machine doesn't separate the two.

## Fix

Change only the test. Keep its point, catching per-hit re-decoding, but make the bound relative to the
machine's current speed rather than an absolute 5 s. For example, time one decode of the same bytes
(`Buffer::from_reader_detecting` or the decoder `find` uses) in the same test, and assert that `find` takes
less than a generous multiple of it (say 50x, which a quadratic regression at 2,000 hits still breaks by far).
Or use a bound that load can't reach, with a comment explaining the margin. Don't change fe-nav's production
code.
