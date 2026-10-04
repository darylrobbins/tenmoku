# Multi-cursor keystrokes: one tree-sitter edit per cursor

Status: **measured** (same-run ratio, loaded machine). A batch of edits
between the tree and the snapshot now gives one `InputEdit` per cursor instead
of a hull over the whole span. Measured 2026-10-03. Source card T23
(Paddock T-11).

## Method

One run of
`cargo bench -p fe-syntax --bench syntax -- 'reparse (keystroke|two-cursor keystroke.*)/10k'`.
The two-cursor cases type the same inside-a-word letter at a random row in the
first 1% and one in the last 1% of the file, deleted again untimed:

- `reparse two-cursor keystroke`: both edits in one `buffer.edit` batch, so one
  `InputEdit` per cursor.
- `reparse two-cursor keystroke, two batches`: the same two edits as two
  `buffer.edit` calls synced together, so the merged-span path (the old
  behaviour for every multi-cursor sync).

Load average at the start and end of the run: 9.06 and 9.97 (three lanes share
the machine). Absolute times are inflated; only the ratio within the run is
the criterion.

## Result (10k lines, mean with criterion's interval)

| | One cursor | Two cursors, one batch | Two cursors, two batches | One batch / two batches |
|---|---|---|---|---|
| Rust | 1.43 ms [1.27, 1.52] | 3.34 ms [2.42, 4.61] | 35.05 ms [34.3, 36.5] | **0.095** |
| JSON | 0.37 ms | 0.39 ms | 17.27 ms | 0.022 |
| Markdown | 14.2 ms | 11.6 ms | 48.7 ms | 0.24 |

Rust meets the at-most-0.5 criterion by a wide margin. The one-cursor figure
in PERF.md from an idle machine is 0.87 ms; under this load the same bench
reads 1.43 ms, and the two-cursor one-batch time is about 2x that here, so the
batch case is still well above one cursor's cost in this run, though far below
the whole-file hull.

## Full run: `cargo bench -p fe-syntax --bench syntax -- two-cursor`

Run to completion on the final candidate, all three languages at both sizes
(10k lines and the 8 MB `MAX_PARSE_BYTES` corpus), load average 8.21 at the
start and 8.48 at the end. This filter does not include the one-cursor case,
so the one-cursor figure is the 1.43 ms Rust / 0.37 ms JSON / 14.2 ms Markdown
from the run above (load about 9 to 10).

| Mean | 10k, one batch | 10k, two batches | Ratio | 8 MB, one batch | 8 MB, two batches | Ratio |
|---|---|---|---|---|---|---|
| Rust | 3.10 ms | 32.94 ms | **0.094** | 43.9 ms | 1.056 s | 0.042 |
| JSON | 0.36 ms | 17.32 ms | 0.021 | 20.3 ms | 717 ms | 0.028 |
| Markdown | 10.99 ms | 48.08 ms | 0.23 | 598 ms | 2.194 s | 0.27 |

Every case finished, and the one-batch path is at most 0.27x the merged-span
path in all six comparisons. The required ratio (Rust, 10k lines) is 0.094
here and 0.095 in the first run.
