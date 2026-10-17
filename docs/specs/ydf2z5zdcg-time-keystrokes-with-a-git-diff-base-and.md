---
task_uid: "01m55bq6tkm734f1ydf2z5zdcg"
task: "T160"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "crates/fe-gpui/benches/keystroke/harness.rs: Case has diff_base and carets, cases() returns 8 with type/diff-base (167 changed rows against the 10k-rust corpus) and type/100-carets, and Harness::new asserts non-empty row_changes() and 100 selections for them"
    required: true
    manual: false
  - id: "C2"
    text: "cargo test -p fe-gpui --test keystroke_latency passes unchanged, and cargo bench -p fe-gpui --bench keystroke -- --test runs all 8 cases"
    required: true
    manual: false
  - id: "C3"
    text: "PERF.md's Typing latency section has per-stage p50/p99 tables for type/diff-base and type/100-carets from one run of every case under xvfb-run on this host, with machine, commit, date and load, and says for each whether total p99 \u003c 8 ms and the editor's own stages (total minus present) p99 \u003c= 2 ms hold; a miss is attributed by stage with the follow-up named"
    required: true
    manual: false
  - id: "C4"
    text: "Cargo.lock and crates/fe-gpui/Cargo.toml are unchanged; no file outside the bench and PERF.md changes"
    required: true
    manual: false
---

# Time keystrokes with a git diff base and with 100 carets: two keystroke bench cases, run against the 8 ms p99 target (Mac T815)

## Problem

M6's done-when is "p99 keystroke under 8 ms", and M6 is multi-cursor, undo and save polish. The keystroke
bench (`crates/fe-gpui/benches/keystroke/`, PERF.md "Typing latency") has six cases, all with one caret and
no diff base. Two per-keystroke costs on the real typing path are therefore unmeasured:

- **A diff base.** With a git base set (`EditorModel::set_diff_base`, as fe-workbench's `git.rs` does for
  every tracked file), each new buffer version is row-diffed synchronously when the change bar paints
  (`crates/fe-gpui/src/view/change_bar.rs` -> `EditorModel::row_changes` -> `row_changes::diff_rows`,
  imara-diff Histogram). Measured directly on the Mac, Histogram takes 31 ms on 100k rows with 200
  scattered changes, so a 10k-row file with ~170 changed rows plausibly costs milliseconds per keystroke.
- **Many carets.** fe-buffer's `edit/100 cursors in 100MB` is 0.1 ms, but the view's side (100 selections
  synced, laid out and painted) isn't timed.

## Change

In `crates/fe-gpui/benches/keystroke/harness.rs`:

- `Case` gains `diff_base: Option<fn(&str) -> String>` (the base made from the corpus text, set with
  `set_diff_base` before the first settle) and `carets: usize` (1 for every existing case).
- Two cases (so `cases()` returns 8): `type/diff-base`, the `type/10k-rust` corpus with a base in which one
  body row of every 10th function differs (167 changed rows), typing `x` at the same caret; and
  `type/100-carets`, the `type/10k-rust` corpus with 100 carets inside a word in 100 functions spread
  through the file, the first at the existing caret, typing `x` (one edit at every caret; `reset`'s
  `backspace` undoes it at every caret).
- `Harness::new` asserts what each case needs: `row_changes()` non-empty for `type/diff-base`, 100
  selections for `type/100-carets`.
- `crates/fe-gpui/benches/keystroke/main.rs`'s header lists the new cases. `tests/keystroke_latency.rs`
  picks its case by name and needs no change.

Run every case on this host, under `xvfb-run` as PERF.md's helium run did (`cargo bench -p fe-gpui --bench
keystroke`, release, private `CARGO_TARGET_DIR`), and add `type/diff-base` and `type/100-carets` tables to
PERF.md's "Typing latency" section in the same per-stage format, with machine, commit, date and load.

Budgets (the plan's typing target): total p99 under 8 ms, and the editor's own stages (`dispatch` through
`draw_rest`, i.e. total minus `present`, which on Linux is the software renderer's) at most 2 ms at p99.
Say for each new case whether it meets them; if `type/diff-base` misses, attribute it by stage and name Mac T812 (the
row-diff Histogram-to-Myers bound, lent to argon) as the fix. Don't change the editor here.
