---
task_uid: "01m4s6vs27r0n7arr5hmtw3pca"
task: "T95"
rev: 1
type: "chore"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-syntax test wide_parent_is_linear compares best-of-N interleaved timings (or a size-scaling ratio) instead of one wall-clock sample, and still fails if pairing were quadratic in the parent's width (show it by a temporary quadratic stand-in, recorded in the notes)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-syntax --test brackets passes 20 times in a row while the machine is under heavy build load (record the command and load average)"
    required: true
    manual: false
---

# Flake: fe-syntax brackets wide_parent_is_linear fails under load (single 20x wall-clock ratio): bound pairing by best-of-interleaved runs (Mac T570)

fe-syntax test `brackets::wide_parent_is_linear` (crates/fe-syntax/tests/brackets.rs:80) asserts pairing x 20 < parsing on a single wall-clock sample. Under gate load it failed in T475's gate (run 01m4s478pvdan1n7ymfyjed3j9: "pairing took 121.55ms, parsing 273.50ms"), though it passes alone (5/5 at load ~34). T475 doesn't touch fe-syntax.

Make the bound robust under load without weakening what it proves (pairing is linear, not superlinear): compare best-of-N interleaved runs of pairing and parsing (as T494 and T442 did for their ratio tests), or compare pairing on input of size n and 4n by best-of-N and bound the ratio. Test-only change.

## Implementation notes

- `wide_parent_is_linear` now takes the best of 5 interleaved parse + pair runs (min of each) and asserts `pair_min * 20 < parse_min`.
- Quadratic stand-in (temporary, not committed): inside the timed pairing, `for i in 0..array.child_count() { array.child(i).unwrap().parent() }` (a pass over the wide parent per child). The test failed: "pairing took 998ms, parsing 341ms".
- Load check: 16 `yes >/dev/null` busy loops, load average ~34 at start (~24 at end); `cargo test -p fe-syntax --test brackets` x20 in a row: passed 20/20.
- Heavy build load (second run): three concurrent `CARGO_TARGET_DIR=<scratch>/tN cargo build --workspace --tests` (30 rustc processes live at the end), load average 20.7 at start of the tests and 50.0 at the end; `cargo test -p fe-syntax --test brackets` x20 in a row: passed 20/20.
