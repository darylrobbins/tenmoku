---
task_uid: "01m425nddsm1e0fm2djgc011ye"
task: "T47"
rev: 4
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-syntax: replaying fuzz/regressions/syntax_edits/tsx-incremental-parse-hang (tsx, text x)3,`x, insert x\"\"[[)} at 0), ParseJob::run returns within 10 s on a helper thread and, after finish, Syntax::failed() is true and tree() is None (integration test crates/fe-syntax/tests/runaway_parse.rs::tsx_incremental_parse_hang, using only the public API, so the regression check can run it at base)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-syntax: replaying fuzz/regressions/syntax_edits/tsx-incremental-parse-hang-from-empty (tsx, empty text, insert (\\0[/(\\0[/]]]]]]\\u{FFFD}# at 0), ParseJob::run returns within 10 s and, after finish, Syntax::failed() is true and tree() is None (integration test crates/fe-syntax/tests/runaway_parse.rs::tsx_incremental_parse_hang_from_empty); the two hang tests no longer also live in runaway_parse_tests.rs"
    required: true
    manual: false
  - id: "C3"
    text: "fe-syntax: with out_of_time forced through the pub(crate) seam, an incremental overrun retries once from scratch and yields a tree; when the retry overruns too, the Parsed has no tree and Syntax::finish leaves failed() true and tree() None (test syntax::runaway_parse_tests::overrun_retries_from_scratch_then_gives_up)"
    required: true
    manual: false
  - id: "C4"
    text: "The cap constants in syntax.rs carry a doc comment giving the measured full-parse time of a file at the parse limit and the margin chosen, and the incremental and from-scratch caps together stay under 10 s; ParseJob::run's signature and its callers in driver.rs and fe-gpui are unchanged (git diff touches neither); cargo test -p fe-syntax and cargo test -p fe-gpui pass, including tests::incremental_reparse_matches_a_full_parse and the driver and window tests"
    required: true
    manual: false
  - id: "C5"
    text: "fuzz/README.md's Known findings says the tsx-incremental-parse-hang* inputs no longer time out because fe-syntax bounds the parse and falls back to plain text, and that the upstream hang is still open"
    required: true
    manual: false
  - id: "C6"
    text: "docs/decisions/khgdxezget-fuzzing.md no longer claims a fresh parse of the TSX hang inputs is fast: finding 3 and the draft upstream issue say a fresh parse hangs too, with the measurement"
    required: true
    manual: false
---

# Bound background parses so a runaway incremental reparse can't hang or exhaust memory (TSX fuzz hang)

## Problem

T12's fuzzing found a TSX incremental reparse that never returns and keeps
allocating (finding 3 in `docs/decisions/khgdxezget-fuzzing.md`). It
reproduces with tree-sitter 0.27.0 and tree-sitter-typescript 0.23.2 alone, so
the root cause is upstream. Contrary to the decision record, a fresh parse
(no old tree) of either final text doesn't finish either: both run past a
30 s abort in release, with the exact decoded inputs (T47 worker, confirmed
independently 2026-10-04). The initial parses take 112–481 µs.

fastedit makes it worse than it has to be. `ParseJob::run`
(`crates/fe-syntax/src/syntax.rs`) calls `parse(|| false)`: no time limit and no
way to stop it. The driver hands it to the GPUI background executor
(`fe-gpui/src/view.rs`, `ParseSpawner`), so a TSX buffer edited into this
state pins a core and grows without bound (1.15 GB after 62 s, 4.6 GB before it
was killed). Meanwhile the driver keeps a task in flight and never starts
another, so highlighting stays stale. Dropping the GPUI task doesn't stop the C
parse. The in-frame `parse_until` path is already bounded by its budget.

Both saved inputs replay with the T12 harness as a libFuzzer timeout:
`fuzz/regressions/syntax_edits/tsx-incremental-parse-hang` and
`tsx-incremental-parse-hang-from-empty`.

## Change

Bound `ParseJob::run` without changing its signature or callers:

1. Run the parse as now, from `old_tree` if there is one, but stop it through
   the existing `out_of_time` progress callback once it overruns a cap.
2. If an incremental parse overruns, retry once from scratch (`old_tree =
   None`) under a cap of its own.
3. If that overruns too, return a `Parsed` with no tree. `Syntax::finish`
   already treats that as giving up on the buffer (plain text, `failed()`),
   as it does for a panic. Log it at `error` like the panic path.

Keep `Parsed::incremental` true and `elapsed` covering both attempts when an
incremental parse overran, so T21's rule sends the next sync straight to the
background rather than spending the frame budget on it again.

Choose the caps so that no legitimate parse is cut off. A full parse of a file
at the parse limit must fit with a wide margin: measure it, and put the
measurement and the margin in a doc comment on the constants. Keep the caps as
`const`s in `syntax.rs`, with a `pub(crate)` seam (for example a
`run_until(out_of_time)` used by `run`) so tests can force an overrun without
a slow parse. Don't touch `driver.rs` or `fe-gpui`: they need no change,
because a bounded `run` always returns and `finish` already handles a missing
tree.

Tests go in a new file, `crates/fe-syntax/src/syntax/runaway_parse_tests.rs`,
declared from `syntax.rs` like the other `*_tests` modules. Each hang test
replays one saved fuzz input, decoded by hand (the fuzz crate's `arbitrary`
format isn't a dependency of fe-syntax). The test's doc comment names the
regression file it comes from:

- `tsx_incremental_parse_hang`: `fuzz/regressions/syntax_edits/tsx-incremental-parse-hang`.
  Language `tsx`, initial text `` x)3,`x ``, then one batch inserting
  `x""[[)}` at 0..0.
- `tsx_incremental_parse_hang_from_empty`:
  `fuzz/regressions/syntax_edits/tsx-incremental-parse-hang-from-empty`.
  Language `tsx`, empty initial text, then one batch inserting
  `(\0[/(\0[/]]]]]]\u{FFFD}#` at 0..0.

Drive them as the fuzz harness does: `Buffer::edit(.., Origin::User)`,
`Syntax::edit(snapshot)`, then `parse_job().run()` and `finish`. Run the parse
on a spawned thread and wait with `recv_timeout(10 s)`, so at the base commit
the test fails rather than hanging the run. At the base the leaked thread
allocates about 200 MB in those 10 s; that is accepted.

For these two inputs both attempts overrun, so each hang test passes when
`run` returns within 10 s and, after `finish`, `Syntax::failed()` is true and
`tree()` is `None`: the buffer falls back to plain text. The two caps together
must stay under 10 s. Keep the retry from scratch (the user, 2026-10-04): it
still covers runaways that only an incremental parse hits; C3 tests it through
the seam.

Correct `docs/decisions/khgdxezget-fuzzing.md`: finding 3's "A fresh parse of
the result takes 11 ms" (about L254) and the draft upstream issue's "A fresh
parse of the same text takes microseconds" (about L273) are wrong. Say a
fresh parse hangs too, give the measurement, and adjust the issue's title and
text so it doesn't call the bug incremental-only. Keep the rest of the record.

Update `fuzz/README.md`'s "Known findings": the TSX inputs no longer time out,
because fe-syntax now bounds the parse and gives up on the buffer. The upstream
hang is still open and still needs the issue drafted in the decision record.

## Out of scope

- Filing the upstream tree-sitter issue (a person posts it).
- Cancelling a background parse when its GPUI task is dropped (`view.rs`).
- Injected-layer parses (`injection.rs`), which the highlight deadline
  already bounds.
- The other T12 findings (Bash, Markdown inline, Java/Python/C++, JavaScript):
  they are mismatches, not hangs, and they need a decision first.
