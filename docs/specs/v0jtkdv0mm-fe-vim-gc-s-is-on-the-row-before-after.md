---
task_uid: "01m5hksr56eqff7tv0jtkdv0mm"
task: "T221"
rev: 1
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "The four corpus cases gc_paragraph_end_mark_linewise, gc_paragraph_end_mark_inclusive, gc_paragraph_end_mark_in_indent and gc_paragraph_end_mark_one_row (recorded from nvim) pass inside commentary_matches_neovim, which fails on trunk"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-vim passes and cargo clippy -p fe-vim --all-targets -- -D warnings is clean"
    required: true
    manual: false
---

# fe-vim: gc's '] is on the row before after an exclusive motion that ended at column 0 (T151 F4, ported from abandoned T152)

Follow-up from the abandoned argon T152 (a duplicate of trunk's Mac T890 = e4f0905d), lane commit c8b9660 (T151 review F4). Lead decision z40.

## Problem
gc's `']` after an exclusive motion that ended at a row's column 0 (`:help exclusive`) is wrong on trunk: `gc}` on `aaa\nbbb\n\nccc` at (0,0) leaves `']` at (2,0); Neovim puts it on the row before, (1,0) once the motion became linewise, else on its last character.

## Change
Port the `crates/fe-vim/src/operator.rs` change of c8b9660 (branch `keep/t152`; the diff is small) onto trunk. Do not include the spec notes or the exchange work.

## Tests
Add the commit's four nvim-recorded corpus cases to `crates/fe-vim/tests/corpus/commentary.toml` and `commentary.expected.json`: gc_paragraph_end_mark_linewise, gc_paragraph_end_mark_inclusive, gc_paragraph_end_mark_in_indent, gc_paragraph_end_mark_one_row. They run inside `commentary_matches_neovim` (tests/commentary_oracle.rs), which fails on trunk and passes with the fix.
