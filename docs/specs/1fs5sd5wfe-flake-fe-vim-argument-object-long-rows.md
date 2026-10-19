---
task_uid: "01m5ardfefvhcb361fs5sd5wfe"
task: "T1010"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "argument_object_long_rows_are_bounded and replace_copies_count_the_indent_their_keys_write no longer compare elapsed wall-clock time to a fixed limit; each asserts a deterministic work bound (or a same-run small-n ratio) that a quadratic implementation would exceed"
    required: true
    manual: false
  - id: "C2"
    text: "Both tests pass 5 of 5 runs with a CPU-bound busy loop per core running, and cargo nextest run -p fe-vim, clippy and fmt are clean"
    required: true
    manual: false
---

# Flake: fe-vim argument_object_long_rows_are_bounded and keys_tests replace_copies_count_the_indent_their_keys_write fail their wall-clock asserts under load

Both failed in main's fast CI rerun on 6cf2cc6a (run 01m5aq39dzkxfx7jb45enz85pp) with the Mac at load average ~130 (stack gate + two lanes + rerun): argument_object.rs:188 took 4.2s against DEFAULT_TIME_LIMIT, and keys_tests.rs replace_copies_count_the_indent_their_keys_write hit its 60s assert. Both pass alone. They guard against quadratic work (rebuilding the row per argument; rewriting the whole indent per R copy), not against slowness, so a wall-clock bound makes them load flakes, like T228 and T1006.

Replace each wall-clock assert with a deterministic bound on the work (a counted budget or a test-only counter of row rebuilds / bytes written, as T228 and T1006 did), or with a ratio against a small-n baseline measured in the same test. The test must still fail on the quadratic behaviour it guards. Linux-OK. Prove it with repeated runs under load.

## Implementation notes

- `replace_copies_count_the_indent_their_keys_write`: dropped the wall-clock assert; the copies bound (`4 * copies * (copies + 1) / 2 <= MAX_PUT`) is the deterministic work bound.
- `argument_object_long_rows_are_bounded`: compares fastest thread-CPU-time samples of a 10x input against a small one (fail at 50x; linear ~10, quadratic ~100), as `a_block_over_a_long_row_is_quick` does. The search time limit is raised for the measured runs (restored on drop) so a quadratic run isn't truncated by the 2s deadline.
- Load proof: `ncpu` background `while :; do :; done` loops, then `cargo nextest run -p fe-vim argument_object_long replace_copies_count` five times in a row. All 5 runs passed both tests (argument_object ~17-21s, replace_copies ~62-72s each).
