---
task_uid: "01m5e06f4hh0bhsgbn0fpdy208"
task: "T238"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test commentary_matches_neovim (top level, in crates/fe-vim/tests/commentary_oracle.rs) passes on crates/fe-vim/tests/corpus/commentary.toml, at least 16 cases recorded with nvim 0.12.x with set commentstring, among them (asserted by name) gcc, gcc_count, gcc_uncomments, gc_j, gc_ip, gc_visual_rows, gc_visual_charwise, gc_mixed_rows_comment, gc_indented_rows, gc_dot_repeat, gc_undo, dgc_object, ygc_object, cgc_object."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test comment_keys_without_tokens_do_nothing (top level, same file) passes: through HeadlessVim with no comment tokens, gcc and gcip change nothing and leave no undo step; and the module docs of crates/fe-vim/src/feature/comment.rs list each divergence from nvim the corpus leaves out."
    required: true
    manual: false
---

# fe-vim: gc{motion}, gcc, visual gc and the gc text object toggle line comments as nvim 0.12's built-in commenting does, through the editor's toggle-comment code (Mac T888)

## Problem

At Mac main 6f032b12 fe-vim has no comment keys: `gcc` and `gc{motion}` cancel, where nvim 0.12.6 maps them
by default (verified with `maparg`: normal `gc`/`gcc` "Toggle comment", visual `gc`, operator-pending `gc`
"Comment textobject"), as vim-commentary, VSCodeVim and IdeaVim do. fastedit's editor already toggles line
comments from the language's delimiters (`EditorModel::toggle_comment`, `crates/fe-editor/src/comment.rs`,
with `EditorHost::comment_tokens`). The user approved commentary in the "Vim parity" track on 2026-10-17,
reusing the editor's toggle-comment code.

## Change

New feature module `crates/fe-vim/src/feature/comment.rs`, declared in `crates/fe-vim/src/feature.rs` and
listed in its `FEATURES`:

- `gc{motion or object}` is a new linewise `Operator::Comment` (`crates/fe-vim/src/args.rs`, with its arm in
  `operator::operate`, `crates/fe-vim/src/operator.rs`, and in the visual operators,
  `crates/fe-vim/src/visual.rs`): it toggles the comment on the target's rows by calling
  `EditorModel::toggle_comment` with the host's `comment_tokens()` over exactly those rows, in one undo step,
  then puts the carets where nvim's do (probe). No tokens (no language) does nothing, as the editor's
  command does. `gcc` and `{count}gcc` take `count` rows, as `gUU` does (`Pending::operate` over
  `Motion::Line`); visual `gc` its rows; `.` repeats.
- `gc` after an operator (`dgc`, `ygc`, `cgc`) is a text object: the run of commented rows around the
  cursor, linewise (`crates/fe-vim/src/text_object.rs` arm), as nvim's.
- Where fastedit's toggle differs from nvim's (blank rows, a block-only language, mixed indentation), keep
  fastedit's and list it in the module docs as a divergence; the oracle corpus holds only cases where they
  agree.
- `HeadlessVim::set_comment_tokens(Option<CommentTokens>)` (`crates/fe-vim/src/headless.rs`) feeds the
  headless host's `comment_tokens`.

Test with the oracle pattern: a new corpus `crates/fe-vim/tests/corpus/commentary.toml` whose cases set
`set = "commentstring=#\\ %s"` (the corpus `set` field the search-case card adds to
`crates/fe-vim/tests/support/mod.rs` and `tests/support/oracle.lua`: extend `support::check` to map
`commentstring=<x>\ %s` to `set_comment_tokens` with line token `<x>`), recorded into
`commentary.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a new
`crates/fe-vim/tests/commentary_oracle.rs` modelled on `tests/line_motions_oracle.rs`.

T787 (inbox) changes how the editor's toggle treats blank rows; whichever lands second keeps the corpus
green. args.rs and operator.rs are changed by the lent T834 and T835, headless.rs by T802.

## Implementation notes

- `gcc`'s second `c` is a reducer operator key, which the reducer cancelled under another pending
  operator without asking the features. Rev 3 widened scope so `reducer.rs` hands it to `reject` (the
  features' `key` hook, as for `gUU`) and `remap.rs` reads it unmapped (nvim maps `gcc` whole); a first
  try with a `FeaturePrefix` after `gc` broke `gc/foo<CR>` and omaps (review rgzp9f F2).
- Over a motion or object, `Operator::Comment` takes rows as `> < = zf` do in `operate_unless_failing`;
  the caret stays at nvim's `'[` (row and byte column), and `u` puts it back there (probed). Visual `gc`
  is routed in `operate_as_wide` to `feature::comment::selection` (scope widened in rev 2 for that and
  `visual/block.rs`'s unreachable arm), leaving the mode as `leave_for_ex` does.
- The `gc` object selects `{first}GV{last}G` (`visual::select_rows`) and applies the operator over that
  selection, so `'<`/`'>` match nvim's, which the oracle records; `.` is patched back to the object.
- Carried T198 F3 (Ex yanks skip `record_yank`) needs `crates/fe-vim/src/ex/lines.rs`, outside this
  card's scope; operator.rs needs no change for it.
