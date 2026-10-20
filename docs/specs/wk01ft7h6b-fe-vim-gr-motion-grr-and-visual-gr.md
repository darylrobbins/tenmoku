---
task_uid: "01m55rk00ehbjv6jwk01ft7h6b"
task: "T163"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The integration test replace_with_register_operator (top level, in crates/fe-vim/tests/replace_with_register.rs) passes on HeadlessVim with set_replace_with_register(true): grw, gr$, 2grw, gr2w, griw, gri\" on \"\" (putting before the cursor), grip, \"agrw, a linewise register over grw (no line break added), a charwise register over grr and 3grr, grgr; after each, the unnamed, small-delete, numbered and (with clipboard unnamedplus) + registers are unchanged, one u restores the text, . repeats with the same register, and the jumplist is unchanged; every case the test marks uncorrected gives the same text, cursor and registers as its plugin-equivalent visual \"xP keys on a twin HeadlessVim."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test replace_with_register_visual (top level, same file) passes: viwgr, Vgr, Vjgr with a charwise register, a block gr with a one-row register (repeated on each row) and with a several-row linewise one (put as a block), \"bviwgr, and . after visual gr replacing a same-size selection at the cursor, the registers unchanged each time."
    required: true
    manual: false
  - id: "C3"
    text: "The integration test replace_with_register_is_opt_in (top level, same file) passes: with the setting off (the default) grw, grr and viwgr change nothing, leave no undo step and leave the context frame without vim.replace_with_register; with it on the frame has that flag; vim.operate with operator replace_with_register replaces either way; and cargo test -p fe-vim passes with any changed existing test named in the finish notes."
    required: true
    manual: false
---

# fe-vim: gr{motion}, grr and visual gr replace text with a register's contents as the ReplaceWithRegister plugin does, behind VimState::set_replace_with_register (Mac T910)

Filed on argon from Mac T910 (card ~/coordination/argon-cards/zzq/T910.json rev 1; note zzq, Vim parity after the user's decisions). Mac task uid 01m55rgzy3rwgsc16qveeads2n. Base: argon trunk 7edfe4f (= mac/main c7b6a04 synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T158 (Mac T905). Held in the inbox: waits until Mac T882, Mac T876 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T910)".

## Problem

The user decided on 2026-10-17 ("Vim parity", follow-up to T900) that `gr` stays nvim 0.12's language-server
prefix by default (T900 binds `grr grn gra gri grt` in the keymap), and that an opt-in setting
`vim.replace_with_register` gives `gr` to ReplaceWithRegister instead, as VSCodeVim's
`vim.replaceWithRegister` (default false) does. At Mac main c7b6a04a fe-vim takes no `gr` key: `g` then `r`
reaches `feature::g`, which no feature answers, and cancels (`crates/fe-vim/src/reducer.rs`,
`Prefix::G => feature::g(..).unwrap_or(Step::Cancel)`).

ReplaceWithRegister's documented behaviour (`doc/ReplaceWithRegister.txt` and `autoload/ReplaceWithRegister.vim`,
v1.42, https://github.com/vim-scripts/ReplaceWithRegister):

- `[count]["x]gr{motion}` replaces what the motion covers with register `x`; `[count]["x]grr` replaces `count`
  lines; `{Visual}["x]gr` replaces the selection. The count goes to the motion or the lines (`g@`'s and
  `V{count}_`'s), never to the put: the register goes in once.
- The old text goes to the black hole: the unnamed register, `"-`, `"0`-`"9` and the clipboard registers are
  unchanged (the plugin restores `"` and empties 'clipboard' around its `p`).
- "The replacement mode (characters or entire lines) is determined by the replacement command / selection,
  not by the register contents": a linewise register's final line break is dropped and it goes in
  characterwise (`s:CorrectForRegtype`); over rows (`grr`, a linewise motion, `V`) the text takes whole rows.
  Blockwise: a one-row register is repeated on every row of the block; a linewise one of several rows goes in
  as a block.
- An empty span (`gri"` on `""`, `'[` after `']`) puts before the cursor (`P`).
- It doesn't touch the jumplist. `.` repeats it with the same register (repeat.vim); `.` after visual `gr`
  repeats over a selection of the same size at the cursor.

nvim has no built-in ReplaceWithRegister to record an oracle against.

## Change

fe-vim only; the setting, its live wiring and the keymap's `gr` rows are the next card.

- `Operator::ReplaceWithRegister` (serialized `replace_with_register`) in `crates/fe-vim/src/args.rs`, its
  behaviour in a new feature module `crates/fe-vim/src/feature/replace_register.rs`, declared in
  `crates/fe-vim/src/feature.rs` as the other `src/feature/` modules are and listed in `FEATURES`, modelled
  on `crates/fe-vim/src/g_operators.rs`: its `g` hook takes `r` (start the operator; in a visual mode,
  operate on `Target::Selection`; with it already pending, `Motion::Line`), and its `key` hook takes the
  doubled `r` of `grr`. The dispatch arms in `crates/fe-vim/src/operator.rs`,
  `crates/fe-vim/src/visual/operate.rs` and `crates/fe-vim/src/visual/block.rs` call into the module.
- The put is `crates/fe-vim/src/visual/put.rs`'s: factor `put`'s body after the selection's ends into a
  function over a span and its kind that the operator, `grr` and visual `gr` share, with the plugin's
  correction (a linewise register over a charwise span goes in charwise without its final line break) and
  `P`'s register handling (nothing written). One undo step; `'[` and `']` as the put sets them; the
  jumplist untouched; a read-only buffer refuses it as any edit.
- `VimState::set_replace_with_register(bool)` (off by default) and `HeadlessVim::set_replace_with_register`
  (`crates/fe-vim/src/state.rs`, `crates/fe-vim/src/headless.rs`), next to `set_jump_on_s`. While it is on,
  the context frame carries the flag `vim.replace_with_register` (for the keymap's `gr` rows); while it is
  off, `VimState::feed` cancels a step that would start or commit `Operator::ReplaceWithRegister`, so `gr`
  then any key cancels as it does today. `vim.operate` with the operator, invoked directly, works either way.
- The module docs list each divergence from the plugin the tests leave out (`"=`, 'selection'=exclusive).

Tests, in a new integration file `crates/fe-vim/tests/replace_with_register.rs`, on HeadlessVim with the
setting on: a hand-written case table, each case's expected text, cursor and registers worked out from the
plugin's autoload code and its comment naming the plugin's equivalent keys (`g\`[vg\`]"xp` with `"`
restored). Where no register correction applies, the same case also runs those equivalent built-in keys
(visual `"xP`, whose registers stay unchanged) on a twin HeadlessVim, whose built-ins the oracle corpora
already check against nvim, and asserts the same text, cursor and registers.

Feature and file order: `feature.rs`, `state.rs`, `headless.rs`, `args.rs`, `operator.rs` and `visual*` are
changed by the vim-parity chain (last: T905 for feature.rs, T882 for state.rs and headless.rs) and by the
remaps chain (T876, state.rs): this card depends on T905, T882 and T876. The filter card T908 (feature.rs)
now depends on this one. T877 (insert/cmdline remaps, parked until the user asks) also touches state.rs and
headless.rs; it isn't a dependency.

## Implementation notes

- `visual/put.rs`: `put` now reads the register and calls `put_over`, the body after the selection's ends
  (over `Ends` and a value), which `visual::replace` shares. `replace` takes the visual selection, or a span
  given as kind, `'[` and `']` (`` g`[vg`] `` or `V`), applies the correction it's handed, adjusts the marks as
  `vim.visual_put` does (cut characters, deleted rows), and puts as `P` (no register written). Over the
  selection it keeps the extent for `.`. `operator.rs`'s normal put was split the same way (`put_value`), for
  the empty-span case (`gri"` on `""`), which is `"xP` at `'[`.
- `vim.operate` with `gr` keeps a `/ . :` register (it reads it). `feature/replace_register.rs` measures the
  target at the primary caret with `operator::motion_span` / `text_object::select`, so `:help exclusive`'s
  adjustments are `g@`'s; at the text's start an empty span takes the grapheme there, as nvim's `decl` can't
  put `']` before `'[`.
- The off gate is in `VimState::reduce` (`replace_register::takes`): a `Wait` with the operator pending, or a
  `vim.operate` commit with it, becomes `Cancel`.
- A register named before `v` is dropped, as nvim drops it (`tests/register_before_visual.rs`), so `"bviwgr`
  puts `"`, as the plugin's `v:register` would be; the test covers that and `viw"bgr`, which puts `b`.
- The jumplist assertion counts entries (no jump added) and compares the whole list with the twin's: entries
  on replaced rows move with the edit as for visual `P`.
- No existing test changed.
- Carried T42 F21 (`visual/block.rs`): the block yank's corners already resolve past a short row's end
  without normal-mode clipping (`corner` in `operate_block`). Checked against nvim 0.12.4 for both endpoints:
  `abcd|e|ij` (0,2) `<C-v>jy` gives `'[` (0,1) `']` (1,1); `ab|efgh` (1,3) `<C-v>ky` (0,2)/(1,3);
  `<C-v>khy` (0,1)/(1,3); `a|bcde` (1,3) `<C-v>k$y` (0,1)/(1,4); fe-vim gives the same. Nothing to change.
- Review round 1 (F1-F4): the block correction follows Vim's `split()` (only an empty first or last line is
  left out) and, for a several-row linewise register, keeps its rows plus the empty one `setreg(x, '',
  "a\<C-v>")` adds, 0 columns wide; every block case in the test was run in nvim 0.12.4 with the plugin's
  correction and `gv"xp` and matches. A repeated block is bounded by `operator::MAX_PUT` before it's built
  (`replace_with_register_refuses_a_block_too_big`). `visual::replace` takes `visual::Over`, whose selection
  carries the block columns `visual::repeat` supplies, so `.` keeps a block's columns over short rows and tabs.
- `crates/fe-vim/src/repeat.rs` (outside the card's listed scope) changed by one guard: `.` after `gr` from
  `"1`-`"8` keeps the register, as repeat.vim does, instead of advancing it as a put's `.` does (F2).
- `.` after a visual `gr` takes as much text as the selection had (the spec's wording, and fe-vim's `.` after
  any visual change); the plugin's visualrepeat.vim reselects with `1v`, which nvim sizes by the text the put
  left, so it isn't used as an oracle there.

