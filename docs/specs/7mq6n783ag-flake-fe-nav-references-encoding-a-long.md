---
task_uid: "01m4vrx61q0nnc8z7mq6n783ag"
task: "T710"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work passes 20 of 20 runs while the workspace's heavy tests run in parallel, and still fails if decoding is made quadratic"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-nav and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-nav references_encoding a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work trips its 9x timing ratio under load

## Problem

`crates/fe-nav/tests/references_encoding.rs:152`, `a_long_legacy_line_with_many_hits_is_decoded_in_bounded_work`,
failed T652's merge gate (run 01m4vqk7z9mxa2bevnwrc6gmjp, 2026-10-13 23:08Z, load ~150): "4x the line took 9.6x as
long (504.955167ms, 4.8295365s)", bound < 9x. T336 rewrote it as a min-of-3 timing ratio; the short quarter-length
run (~0.5 s) is noisy under load, so the ratio can pass 9x though the code is linear. A/B at base vs T652 under load
80-175 showed no difference (passes 5/5 alone).

## Change

Measure the work instead of wall time: count decoded bytes (T336's spec already allowed a decoded-byte counter) and
bound that linearly, or, if a counter can't be exposed cleanly, lengthen the baseline line and take more runs so the
ratio is stable under load. Keep what the test guards: decoding a long legacy-encoded line with many hits stays linear.

## Tests

Test-only fix unless a counter needs a test-support hook: prove it with 20 runs while the workspace's heavy tests run
in parallel.

## Implementation notes

Took the longer-baseline fallback (no counter: a global one would be shared by parallel tests). The test compares an 8,000-unit (12 MB) line with a 1,000-unit one (about 0.7 s, mostly fixed cost), best of 5 for the baseline and 3 for the long line, bound 24x (linear 8x, quadratic 64x); no retries. The long search is cancelled once it outruns the bound, so a quadratic decoder fails by assertion instead of running for minutes.

- 20/20 passes of the test binary while `cargo test --workspace` ran three times in the background (load average ~75-80); also 20/20 under two busy loops per core (load ~220) on the previous sizes.
- Mutation (decoder restarted per hit, `if true || to < self.raw` in `Cursor::advance`) at full size: fails with "8x the line took over 24x as long (the eighth took 8.59s)" after 256 s.
