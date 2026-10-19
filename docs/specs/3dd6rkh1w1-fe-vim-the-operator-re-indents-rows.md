---
task_uid: "01m58zfwd0catayt3dd6rkh1w1"
task: "T211"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test reindent_operator_follows_the_language (top level, in crates/fe-vim/tests/reindent.rs) passes: with a brace-language host, == on a mis-indented row, =ip, =G, visual =, 3==, . and u each give the host's indentation row by row, blank rows staying empty and the cursor on the first row's first non-blank."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test reindent_without_suggestions_keeps_rows (top level, same file) passes: with no suggestions, =ip changes nothing and leaves no undo step."
    required: true
    manual: false
---

# fe-vim: the = operator re-indents rows through the editor's language indentation (==, ={motion}, visual =) (Mac T902)

## Problem

At Mac main 6f032b12 fe-vim has no `=` operator (`crates/fe-vim/src/insert.rs` docs: "There's no `=`
operator yet to re-indent rows already there"): `==`, `=ip` and visual `=` cancel. VSCodeVim and IdeaVim map
it to their editor's reindent. fastedit already suggests indentation per language
(`EditorHost::suggest_indent`, used by `o`, `O`, `cc` and `Enter`). The user approved `=` in the "Vim parity"
track on 2026-10-17, through the editor's indent engine.

## Change

New feature module `crates/fe-vim/src/feature/reindent.rs`, declared in `crates/fe-vim/src/feature.rs` and
listed in `FEATURES`, its `key` hook taking `=` (starting a new linewise `Operator::Reindent`,
`crates/fe-vim/src/args.rs`, with arms in `crates/fe-vim/src/operator.rs` and the visual operators in
`crates/fe-vim/src/visual.rs`), `==`/`{count}==` over `Motion::Line`:

- Each target row, top to bottom, takes the indentation `host.suggest_indent` gives for it in the text as the
  rows above it have just been re-indented, in the editor's indent style; a row with no suggestion keeps its
  indentation (fastedit has no C-indenting fallback, unlike nvim with no 'indentexpr': a documented
  divergence); blank rows stay empty.
- One undo step; the cursor goes to the first non-blank of the first row, as nvim's does; `.` repeats over
  as many rows; marks stay on their rows.

Tests in a new integration file `crates/fe-vim/tests/reindent.rs`, with a brace-language host as
`tests/auto_indent.rs` builds one (`Braces`): `==` on a mis-indented row, `=ip`, `=G`, visual `=`, a count,
`.`, undo, a host with no suggestion, blank rows.

## Implementation notes

- The host is asked about every target row in the text as `=` found it, in one plan with no catch-up
  between (`operator::apply_plan`), and each suggestion's basis row is read as it has just been re-indented,
  as `o`/`O` resolve theirs in `insert.rs`. That is what asking after each row's edit would give, since a
  suggestion is relative to its basis row's indentation, and it keeps the app's host to one reparse: its
  `suggest_indent` reparses once per buffer version within a 20 ms budget per dispatch
  (`fe-gpui/src/commands.rs`), so asking after each row's edit would run out of budget on a long `=G`
  and leave the later rows as they were.
- A row of only blanks is emptied, as nvim's `=` empties one (its amount is 0 for a blank line).
- The cursor: nvim 0.12 with its default 'nostartofline' keeps the goal column on the first row
  (probed: `=j` from column 2 lands on column 2); Vim's 'startofline' gives the first non-blank, which the
  criteria ask for and fe-vim does. Documented as a known difference in `feature/reindent.rs`.
- Visual `=` over a block takes the block's rows, as `J` does.
- `insert.rs`'s module docs still say there's no `=` operator; that file is outside this task's scope.
- Scope widened to `visual/operate.rs` and `visual/block.rs` (the visual operator matches) and
  `tests/put_newline_register.rs`, the regression test for carried finding T169 F4: a newline-led charwise
  put goes to the next row's first non-blank only from the `.` register (nvim 0.12 probed: `"ap` and `"aP`
  of `\n x` leave the cursor at (0, 0)).
