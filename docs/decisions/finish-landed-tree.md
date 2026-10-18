# Installing a landed whole-file parse blocks the UI thread

Status: **open; measured, not fixed.** Source card T146 (measures M2's "UI never
blocks on a parse"). Follow-up card to be filed: T146a, below.

## Numbers

`cargo bench -p fe-syntax --bench driver`, rust/7 MiB (just under the parse
limit): max 1254 ms (394 ms to 1.25 s across runs, by load) in the frame a background parse
lands, against the 8 ms budget (one 120 Hz frame). p99 is 0.017 ms. Table and
machine in PERF.md, *Typing during a background parse*.

## Which call

`SyntaxDriver::finish`, in `Syntax::finish` (`crates/fe-syntax/src/syntax.rs`).
Timing inside it for the 7 MiB case: `tree.edit` of the landed tree (one input
edit, the keystrokes typed during the parse) about 400 ms; then `self.tree =
Some(tree)` drops the previous tree for 150–215 ms. `input_edits`, `sync` and
the catch-up `sync` are microseconds. rust/20 MiB (window parses) stays under
8 ms; rust/10k lines pays 14–28 ms per landed parse, also in `tree.edit`.

## Follow-up

T146a: take the cost off the UI thread. Candidates: apply the pending edits to
the tree on the parse thread before it is sent (the job would be given the
latest snapshot's edits, or reparse once more incrementally there); drop the
replaced tree on a background thread. Not done here.

## rust/20 MiB measures the windowed path

A window parse is 32 KiB (`WINDOW_BYTES`) and takes about 1 ms, so a worker is
running at the start of only about 33 of 600 frames. The case has no minimum
for that count (T146 revision 2); its p99 and max (1.3 ms, 2.4 ms) are within
budget but don't show the UI thread under a long concurrent parse.
