---
task_uid: "01m54r2fzy0x8s2fp2gsvs641x"
task: "T132"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test increment_that_finds_no_number_repeats_and_keeps_a_macro_going (tests/increment_no_number.rs, top level) passes, as nvim --clean does: on \"ab\\ncd\\nef\\ngh\\nij\" at (0,0) qav\u003cC-a\u003exjq2@a gives \"b\\nd\\nf\\ngh\\nij\" with the caret at (3,0), and qaVg\u003cC-x\u003exjq2@a the same; on \"ab\\n5\\n7\\n9\" at (0,0) v\u003cC-a\u003ej. gives \"ab\\n6\\n7\\n9\", vl3\u003cC-x\u003ej. and 3\u003cC-x\u003ej. give \"ab\\n2\\n7\\n9\", \u003cC-a\u003ej. gives \"ab\\n6\\n7\\n9\" and x\u003cC-a\u003ej. gives \"b\\n6\\n7\\n9\"; v$\u003cC-a\u003e on \"abc def\" leaves state().failed() false; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "Unchanged: a normal-mode \u003cC-a\u003e or \u003cC-x\u003e that finds no number still fails (state().failed() true), changes nothing and leaves no undo step; Vj\u003cC-a\u003ejj. on \"ab\\n5\\n7\\n9\" still gives \"ab\\n6\\n8\\n10\"; the module docs say what . repeats and that only normal mode fails"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: ctrl-a and ctrl-x that find no number are still the change . repeats, and over a selection they do not fail, so a macro goes on, as nvim does (Mac T763)

Filed on argon from Mac T763 (card ~/coordination/argon-cards/zzb/T763.json rev 1; note zzb). Mac task uid 01m54qvdvtb4hdpvj430mj63k9. Base: argon trunk 79b99f1 (= mac/main 42180d6 synced); the files it changes are byte-identical on Mac main 838c0b35. Title the host commit "... (Mac T763)".

## Problem

`<C-a>` and `<C-x>` that find no number go wrong two ways in fe-vim (`crates/fe-vim/src/increment.rs`, the
command's tail, about line 440 at Mac main 838c0b35):

1. Over a visual selection (`v`, `V`, `<C-v>`, and `g<C-a>`/`g<C-x>`) with no number in it, fe-vim sets
   `state.failed` (`target.state.failed |= transaction.is_none()`), so a macro stops there. nvim's ends the mode
   as fe-vim does but doesn't fail: the macro goes on. Only normal mode fails (nvim beeps there and a macro
   stops, as fe-vim's does).
2. A change that finds no number, in normal or visual mode, isn't recorded as the last change
   (`set_last_change` runs only inside `if let Some(id) = transaction`), so `.` repeats the change before it, or
   nothing. nvim records it: `.` then runs that `<C-a>`/`<C-x>` again, with its count, from the new caret (over
   a selection of the same size for a visual one).

Reproduced at Mac main 838c0b35 with `HeadlessVim` against `nvim --clean --headless` 0.12.6 (the oracle's
settings), caret at (0,0):

| buffer | keys | nvim | fe-vim |
|---|---|---|---|
| `ab\ncd\nef\ngh\nij` | `qav<C-a>xjq2@a` | `b\nd\nf\ngh\nij`, caret (3,0) | `b\ncd\nef\ngh\nij`, caret (1,0): the replay stops |
| `ab\ncd\nef\ngh\nij` | `qaVg<C-x>xjq2@a` | `b\nd\nf\ngh\nij`, caret (3,0) | `b\ncd\nef\ngh\nij`, caret (1,0) |
| `ab\ncd\nef\ngh\nij` | `qa<C-v>j<C-x>xjjq@a` | `b\ncd\nf\ngh\nij`, caret (4,0) | unchanged after the first, caret (2,0) |
| `ab\n5\n7\n9` | `v<C-a>j.` | `ab\n6\n7\n9` | `ab\n5\n7\n9` (`.` fails) |
| `ab\n5\n7\n9` | `vl3<C-x>j.` | `ab\n2\n7\n9` | `ab\n5\n7\n9` |
| `ab\n5\n7\n9` | `<C-a>j.` | `ab\n6\n7\n9` | `ab\n5\n7\n9` |
| `ab\n5\n7\n9` | `3<C-x>j.` | `ab\n2\n7\n9` | `ab\n5\n7\n9` |
| `ab\n5\n7\n9` | `x<C-a>j.` | `b\n6\n7\n9` | `b\n\n7\n9` (`.` repeats the `x`) |
| `abc def` | `v$<C-a>` then `state().failed()` | false | true |

Unchanged: a normal-mode `<C-a>` with no number still fails (`<C-a>` on `abc def`: failed, so a macro stops),
leaves no undo step and changes nothing; `Vj<C-a>jj.` on `ab\n5\n7\n9` still gives `ab\n6\n8\n10`.

A prototype of the change below (about 10 lines) took a 1,500-case random fuzz of `<C-a>`, `<C-x>`, `g<C-a>`
with counts, visual modes and `.` from 42 mismatches with nvim (all the visual no-number failure) to 0, plus the
`.` and macro rows above, with no new mismatch; `cargo test -p fe-vim` passed unchanged.

## Change

In `crates/fe-vim/src/increment.rs`, the command's tail:

- record the last change (`Change::Increment` with the replay extent, as now) whether or not a number was
  found, keeping `state.carets.record` and the goal reset for a change that made a transaction;
- set `state.failed` only when no number was found and the command ran over no area (normal mode, not a
  selection or a visual `.` replay);
- update the module docs' "A row with no number changes nothing and leaves no undo step; `.` repeats it" to
  say what `.` repeats and that only normal mode fails.

Regression test: a new integration test `crates/fe-vim/tests/increment_no_number.rs`, at the top level of the
file, `increment_that_finds_no_number_repeats_and_keeps_a_macro_going`, driving `fe_vim::HeadlessVim` through
the table's rows (text, caret, and `state().failed()` where the table says). Probe any added row with
`nvim --clean --headless` first.
