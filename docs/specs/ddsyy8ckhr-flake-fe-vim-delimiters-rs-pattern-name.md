---
task_uid: "01m4vcqqzhv237mqddsyy8ckhr"
task: "T708"
rev: 2
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "Every pattern-name test in crates/fe-vim/src/tests/delimiters.rs, including the three named, passes 20 of 20 runs while the workspace's heavy tests run in parallel"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-vim and clippy -D warnings pass"
    required: true
    manual: false
---

# Flake: fe-vim delimiters.rs pattern-name tests (names_are_patterns, pattern_names_pair_as_nvim_reads_them, pattern_names_read_collections_and_magic_as_nvim_does) fail under load when yit hits the 2 s search clock

## Problem

fe-vim's `crates/fe-vim/src/tests/delimiters.rs` pattern-name tests fail merge gates under load: their `yit` on a
pattern-name tag yanks "" instead of "x" once delimiters.rs's 2 s search `Clock` runs out. Seen so far:

- `tests::delimiters::names_are_patterns` (delimiters.rs:236, 2.127 s): T648 gate, run 01m4vbrfwe0hrjy5zt5g00mx0s,
  load ~132.
- `pattern_names_pair_as_nvim_reads_them` (delimiters.rs:262, 2.08 s) and
  `pattern_names_read_collections_and_magic_as_nvim_does` (delimiters.rs:285, 2.16 s): T652 gate, run
  01m4vmn48st0p2j04p2234ng88, load ~170.

Alone, under load 110-170, they pass every time in 0.3-0.5 s. Same mechanism as T582 (delimiters_oracle pattern
names hitting the 2 s clock).

## Change

Make every test in delimiters.rs that searches pattern names independent of machine load (for example give the
tests a clock that doesn't run out, or a much larger budget for tests), without weakening what they assert. Cover
all such tests in the file, not only the three seen. Consider fixing T582's test the same way in the same change
if it is still open.

## Tests

No regression test (test-only fix): prove it with 20 runs while the workspace's heavy tests run in parallel.

## Implementation notes

- Cause: `delimiters::Clock::new()` took `Deadline::new()`, the process-wide 2 s search limit, so under load a
  pattern-name `yit` ran out and yanked "". `set_time_limit` is process-wide and other unit tests change it, so
  raising it from the tests would race; the fix is in `Clock::new()` itself. Under `cfg(test)` it uses
  `Deadline::never()`. `STOPPED` still forces expiry for the timeout test. This is a source change outside the
  test file (`crates/fe-vim/src/delimiters.rs`, 7 lines), needed because the clock is built there.
- This reaches every `tests::delimiters::` test (21 of them, all pattern-name tests included: `names_are_patterns`,
  `pattern_names_pair_as_nvim_reads_them`, `pattern_names_read_collections_and_magic_as_nvim_does`,
  `pattern_names_keep_joined_chars_whole`, `a_count_passes_over_openings_unpaired`, ...). Not T582's
  `delimiters_oracle`: an integration test, built without `cfg(test)`, so left alone.
- Load validation (C1), 2026-10-14: the fe-vim lib test binary run as
  `fe_vim-<hash> tests::delimiters::` 20 times in a row while `cargo test --workspace` ran concurrently
  (`history_overhead` and other heavy tests alive throughout; load average 21 at start, 40 at the end).
  Result: 20 of 20 runs `21 passed; 0 failed`, no retries, each run 2.7-4.4 s (0.9 s unloaded).
