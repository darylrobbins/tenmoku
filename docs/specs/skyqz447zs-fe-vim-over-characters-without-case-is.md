---
task_uid: "01m54f2nsr96zz7cskyqz447zs"
task: "T131"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-vim test tilde_over_characters_without_case_is_an_undo_step (tests/toggle_case_without_case.rs, top level) passes: ~ over graphemes with no case leaves the text alone but makes one undo step, so u (after moving away) puts the caret back on its first grapheme and . repeats it, as nvim --clean does (on \"ab.c 12\\nxy 日本\" at (0,0): ll~u (0,2), ll~jju (0,2), jll~ku (1,2), 5|~ku (0,4), jllll~ku (1,6), ll~h.jju (0,2)); it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "~ that changes a case, counted ~, ~ on an empty row (fails, no change), r with the same character, visual ~ and g~ are unchanged (ll2~jju (0,2), l~jju (0,1), rajju (0,0), llv~jju (0,2), llg~ljju (0,2))"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-vim passes with no existing test edited (the oracle corpora included); cargo clippy -p fe-vim --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-vim: ~ over characters without case is still an undo step, so u puts the caret back where it started, as nvim does (Mac T757)

Filed on argon from Mac T757 (card ~/coordination/argon-cards/zy/T757.json rev 1; note zy). Mac task uid 01m54etqrwqrdkcbf14534y7kq. Base: argon trunk d52c23b (= mac/main 5837b41 synced); the file it changes is byte-identical on Mac main 36627a7c. Argon dep: T129 (Mac T744, landed 495ae6a), same file edit.rs, different function. Title the host commit "... (Mac T757)".

## Problem

`~` over graphemes none of which has a case (punctuation, digits, CJK, blanks) changes no text, but in nvim it
is still a change: an undo step whose `u` puts the caret back where the `~` started, and something `.`
repeats as one. fe-vim makes no edit at all, so the next `u` undoes the change before it instead (or nothing),
and the caret stays wherever it moved since.

`crates/fe-vim/src/edit.rs` `toggle_case` (line 378 at Mac 80e6c76d): when the toggled text equals the
original (`text.slice_to_string(range.clone()) == toggled`), the caret's `Acts` has no edits, only a landing,
so `apply` records no transaction. `r` with the same character (`ra` on `a`) and visual `~`/`g~` over
caseless text already make an undo step, and match nvim.

Reproduced at Mac 80e6c76d with `HeadlessVim` against `nvim --clean --headless` 0.12.6 (the oracle's
settings), on the buffer `ab.c 12\nxy 日本` (caret (row, byte column)); the text never changes:

| cursor | keys | nvim caret | fe-vim caret |
|---|---|---|---|
| (0,0) | `ll~u` | (0,2) | (0,3) |
| (0,0) | `ll~jju` | (0,2) | (1,3) |
| (0,0) | `jll~ku` | (1,2) | (0,3) |
| (0,0) | `5\|~ku` | (0,4) | (0,5) |
| (0,0) | `jllll~ku` (on `本`) | (1,6) | (0,5) |
| (0,0) | `ll~h.jju` | (0,2) | (1,3) |

Already matching and to stay so: `ll2~jju` (0,2) and `l~jju` (0,1) (a count or a grapheme with a case),
`rajju` (0,0), `llv~jju` (0,2), `llg~ljju` (0,2).

## Change

In `toggle_case`, a caret whose graphemes have no case still makes a change: drop the early return for an
unchanged range, so its `Acts` carries the same-text edit with `Land::Edit(0, land)`, exactly as `r` with the
same character does, and `apply` records one undo step whose `u` puts the caret back on the `~`'s first
grapheme. `.` repeats it as one change. A `~` on an empty row still fails and changes nothing, as now. A
prototype of exactly this (6 lines removed) made every table row match, took a 2,500-case random fuzz of `~`,
`2~`, motions, `u`, `.` and `<C-r>` over mixed-case text from 909 mismatches to 841 (all 841 fail at the
base too; many are fuzz artifacts, `u` undoing the case's setup) with no new mismatch, and `cargo test -p fe-vim` passed unchanged.

This is in `edit.rs`, which T744 (`J`/`:j` carrying `'<` `'>`) also changes, hence the dependency.

Regression test: a new integration test `crates/fe-vim/tests/toggle_case_without_case.rs`, at the top level of
the file, `tilde_over_characters_without_case_is_an_undo_step`, driving `fe_vim::HeadlessVim` through the
table's rows and the unchanged ones and asserting the text and the caret. Probe any added row with
`nvim --clean --headless` first.
