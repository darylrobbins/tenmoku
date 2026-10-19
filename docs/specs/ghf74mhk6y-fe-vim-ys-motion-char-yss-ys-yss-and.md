---
task_uid: "01m55mj4gfbn52xeghf74mhk6y"
task: "T150"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test surround_adds_pairs (top level, in crates/fe-vim/tests/surround_add.rs) passes: ysiw) ysiw( ysiw\" ys$\" yss) yS and ySS (own indented rows), ysiw\u003cem\u003e, ysiw\u003ca href=\"x\"\u003e closing with \u003c/a\u003e, cs\"\u003cb\u003e, Esc while typing a tag cancelling, . repeating with the same pair, and one u undoing."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test visual_s_surrounds (top level, same file) passes: viwS) charwise, VS{ putting the pair on rows of their own, a block surrounding each row's part, and visual R still changing the rows."
    required: true
    manual: false
---

# fe-vim: ys{motion}{char}, yss, yS, ySS and visual S add a surrounding pair or tag as vim-surround does (Mac T886)

Filed on argon from Mac T886 (card ~/coordination/argon-cards/zzp/T886.json rev 1; note zzp, the Vim parity track part 2). Mac task uid 01m55me6v9sab6mjtqf1zmnpbk. Base: argon trunk (mac/main synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T149 (Mac T885). Title the host commit "... (Mac T886)".

## Problem

The `ds`/`cs` card adds vim-surround's delete and change; adding a pair is still missing: `ysiw)` cancels
(`s` isn't a motion after `y`) and visual `S` changes the selected lines. The user approved surround in the
"Vim parity" track on 2026-10-17.

## Change

In `crates/fe-vim/src/feature/surround.rs`, through the feature prefix the `ds`/`cs` card added:

- `ys{motion or text object}{c}` surrounds the target's text with the pair `{c}` names (the same names as
  `ds`; an opening bracket adds a space inside); `yss{c}` surrounds the row's text without its indent;
  `yS{target}{c}` and `ySS{c}` put the text on its own indented rows between the pair (vim-surround's
  documented forms).
- Tags: `{c}` of `<` or `t` reads a tag name with attributes up to `>` (`ysiw<em>`, `ysiw<a href="x">`),
  closing with `</em>`; `<Esc>` while typing it cancels. `cs{old}<em>` and `cs{old}t` take a tag the same way.
- Visual `S{c}` surrounds the selection: charwise as `ys`, linewise putting the pair on rows of their own
  around the rows, blockwise each row's part. This takes visual `S` from nvim's "change the rows"
  (visual `R` and `cc` still do that), as vim-surround and VSCodeVim take it: change its arm in
  `crates/fe-vim/src/reducer.rs`'s visual operators to the feature.
- One undo step; `.` repeats with the same pair; the cursor lands where vim-surround's documentation
  says (the start of the inserted opening pair).

Behaviour reference: vim-surround's `doc/surround.txt` examples (no source). Tests in a new integration file
`crates/fe-vim/tests/surround_add.rs`. Check `src/tests` for a test that asserts visual `S` changes rows
(`rg '"S"|run\("V.*S' crates/fe-vim/src/tests`) and say if one must change.

## Implementation notes

- `ys` and `yS` start a new `Operator::Surround { rows }` (args.rs); the reducer's commit hands its
  target to `feature::surround::target`, which waits on the feature prefix holding it
  (`FeaturePrefix::with_target`). The prefix now owns a `Vec<char>` (up to 1024 keys, for a tag's
  attributes), so `FeaturePrefix` and the reducer's `Prefix` are no longer `Copy`, and the prefix hook
  takes `&FeaturePrefix`. `vim.operate` refuses the surround operator; `ys/` and `ys?` cancel.
- A new command, `vim.surround_add` (`SurroundAddArgs { target, pair, rows, count }`), runs `ys`,
  `yS` and visual `S`; `.` repeats it through `Change::SurroundAdd` (repeat.rs). `vim.surround`'s
  `new` is now a string: a char naming a pair as before, or a whole opening tag (`<em>`).
- `yss` and `ys_` both take the row's text without its indent (`Motion::Line`); a charwise target
  from `ys` drops the blanks it ends with (vim-surround's `ysaw`). Linewise targets put the pair on
  rows of their own at the first row's indent; `yS` indents the text one 'shiftwidth' (vim-surround
  reindents with `=` instead). Visual `S` isn't repeated by `.` (vim-repeat doesn't either).
- Changed existing tests: `crates/fe-vim/tests/surround.rs` `feature_prefix_waits_and_cancels`
  (`cs"<` and `cs"t` now wait for a tag, and `ys` leaves the operator pending), and
  two unit tests that asserted visual `S` changes the rows: `tests::visual::reducer_and_commands`
  (visual `S` now commits `vim.surround_add`) and `tests::jump::opt_in_s` (with jump-on-`s` off,
  `vSX<Esc>` became `vS)`; the spec's suggested `rg` pattern misses this one).
- The nvim corpus case `block_S` (`tests/corpus/visual_block.toml`) recorded nvim's blockwise `S`;
  it became `block_R_tab_row`, the same block and buffer with `R` (nvim's `v_S` and `v_R` are one
  command), re-recorded with nvim 0.12.4.
- `v_S` and `V_S` (`tests/corpus/visual.toml`), nvim's charwise and linewise `S`, went: `v_R` and
  `V_R` beside them are the same keys with `R` on the same buffer and cursor. Both expected files
  were re-recorded with nvim 0.12.4; nothing else in them changed.
