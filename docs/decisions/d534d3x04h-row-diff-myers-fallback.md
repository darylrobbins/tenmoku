# T136: row diffs fall back from Histogram to Myers past a work bound

`row_changes::row_diff` strips the common prefix and suffix and runs Histogram
when `min(b, a) * (b + a) <= HISTOGRAM_WORK` (1 << 21), else imara's Myers
(non-minimal). `diff_rows` and `TextDiff::new` both call it. Blame and
`proposed.rs` keep Histogram.

## C5: release timings

Linux x86 dev box, `cargo test --release`, rows `"{i}"` vs `"x{i}"` on the
changed rows, minimum of 15 runs, measured with other lanes' builds running
(the box was loaded: the base numbers are 2-5x the quieter run below).
Diff step = `Diff::compute` plus `postprocess_lines` on an interned input:
base is Histogram, candidate is `row_diff` (prefix/suffix strip, then Histogram
or Myers). Interning = splitting the text into rows and `InternedInput::new`,
unchanged from the base and not bounded.

| Case | Base diff step | Candidate diff step (`row_diff`) | Interning (both) |
|---|---|---|---|
| 20,000 rows, every other changed | 1,291 ms | 0.54 ms | 1.9 ms |
| 40,000 rows, every other changed | 4,134 ms | 0.93 ms | 3.9 ms |
| 100,000 rows, 200 scattered edits | 50.8 ms | 2.06 ms | 31.8 ms |

Every candidate diff step is under 5 ms. All three cases are over the work
bound (the changed region is nearly the whole text), so they run Myers.

An earlier, quieter run of the whole `TextDiff::new` (split, intern, `row_diff`,
postprocess) gave 1.17 ms, 2.44 ms and 5.97 ms for the three cases, against
271 ms, 1,084 ms and 17.6 ms for the base's diff step; interning was 0.93 ms,
1.99 ms and 5.03 ms of those.
