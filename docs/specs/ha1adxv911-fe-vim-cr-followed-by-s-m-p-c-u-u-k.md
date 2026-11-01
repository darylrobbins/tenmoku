---
task_uid: "01m6c9syhqsh3v4vha1adxv911"
task: "T264"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test coercions_match_abolish (crates/fe-vim/tests/abolish.rs) passes: every coercion char on fooBar, HTTPServer, foo_bar, foo-bar, utf8Value and a Unicode word gives abolish's transform (e.g. crs HTTPServer -\u003e http_server, crc foo_bar -\u003e fooBar, crm foo_bar -\u003e FooBar, crt foo_bar -\u003e Foo Bar), the caret at the word's start"
    required: true
    manual: false
  - id: "C2"
    text: "Test coercion_counts_repeat_and_undo passes: 2crs on 'fooBar bazQux' coerces 'fooBar ' only; . repeats the coercion and count at a new caret; one u undoes it; two carets change both words in one undo step"
    required: true
    manual: false
  - id: "C3"
    text: "Test coercion_cancels_and_keeps_registers passes: cr\u003cEsc\u003e and crz change nothing and leave normal mode; crs on an already snake_case word makes no edit and no undo step; no register (unnamed, 0-9, -) changes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-vim passes, including tests::feature_list::features_list_every_feature_module_once"
    required: true
    manual: false
---

# fe-vim: cr followed by s _ m p c u U - k . <Space> or t coerces the word under the caret to snake_case, MixedCase, camelCase, UPPER_CASE, dash-case, dot.case, space case or Title Case as vim-abolish does, with counts and dot-repeat (Mac T1289)

## Problem

The user approved vim-abolish's case coercion as a built-in vim feature on 2026-11-01: `crs` snake_case, `crm`
MixedCase, `crc` camelCase, `cru` UPPER_CASE, `cr-` kebab-case, `cr.` dot.case, `cr<space>` space case, on the
word under the cursor, dot-repeatable, with counts as abolish does. fe-vim has nothing for it today: `c` then `r`
cancels (the reducer's `'r'` arm, `crates/fe-vim/src/reducer.rs:635`, only starts `r{char}` with no operator,
and `replace_register::key` takes `r` only after `gr`).

`cr` collides with nothing: no feature takes `r` after `c` (`rg "'r'" crates/fe-vim/src`), the bundled keymap
`crates/fe-keymap/keymaps/fastedit.toml` has no `c r` chord, and the LazyVim preset maps only `<leader>cr`
(`crates/fastedit/presets/lazyvim.vimrc:32`).

nvim has no abolish, so there is no oracle: plain fe-vim tests.

## Change

A feature module, `crates/fe-vim/src/feature/abolish.rs` (see `crates/fe-vim/src/feature.rs`; listed in
`FEATURES`), modelled on `surround.rs`'s `cs`:

- Keys: its `key` hook takes `r` in normal mode after the `c` operator and waits on a `FeaturePrefix` named
  `abolish` (`vim.pending_kind` `abolish`). Its `prefix` hook takes the coercion char and commits the new operator
  `Operator::Coerce { to: char }` (`crates/fe-vim/src/args.rs`, serde name `coerce`) over
  `TextObject::InnerWord` with the pending count; `<Esc>` or any other char cancels and changes nothing.
- Coercions, as abolish's `g:Abolish.Coercions` (`c`/`m`/`p`/`s`/`_`/`u`/`U`/`-`/`k`/`.`/`<Space>`/`t`), with
  abolish's transforms (Vim's `\u` `\l` `\d` are ASCII; case mapping is Unicode):
  - snake (`s`, `_`): `::` → `/`; insert `_` between an uppercase run and an uppercase+lowercase pair
    (`HTTPServer` → `HTTP_Server`) and between a lowercase letter or digit and an uppercase one; `.` and `-` → `_`;
    lowercase all.
  - camel (`c`): `-` → `_`; then if there is no `_` and some lowercase letter, lowercase the first char only;
    else drop each `_` uppercasing the char after it, lowercasing chars not after one.
  - mixed (`m`, `p`): camel, first char uppercased.
  - upper (`u`, `U`): snake, uppercased. dash (`-`, `k`): snake with `_` → `-`. dot (`.`): `_` → `.`.
    space (`<Space>`): `_` → ` `. title (`t`): space case with the first char of each `[0-9A-Za-z_]` run
    uppercased.
- What it covers: `{count}iw`, as abolish's `{count}g@iw` does: `2crs` on `fooBar bazQux` coerces `fooBar ` (the
  word and its blank). Each caret acts; one undo step for all; a word the coercion leaves unchanged makes no edit.
  The caret goes to the start of the covered text; `'[` and `']` mark the new text.
- `.` repeats it at the caret then, with the same coercion and count (through the operator's normal repeat).
- Registers are left as they were. (abolish's yank-and-put leaves the old word in `"0` and `"-`; a deliberate
  difference, documented in the module doc.)
- `Operator::Coerce` gets arms wherever the compiler asks for them (`crates/fe-vim/src/operator.rs`,
  `crates/fe-vim/src/visual/operate.rs`, `crates/fe-vim/src/visual/block.rs`, `Target::writes_numbered`); there's no
  visual-mode `cr` (abolish maps none).

Tests: new `crates/fe-vim/tests/abolish.rs`, through `HeadlessVim`.

## Implementation notes

- `cr` hands `vim.operate` an `Operator::Coerce` over `{count}iw`; `operator::operate_over` routes it to
  `feature::abolish::operate` before the selection check, which refuses a selection or a motion target, so the
  visual arms are `unreachable!`.
- Title case uppercases a `[0-9A-Za-z_]` char only where abolish's `\<\w` would: not after a word char (vim's
  default 'iskeyword', which takes non-ASCII letters), so `émile_zola` gives `émile Zola`, not `éMile Zola`.
- Case mapping is one char for one by nvim's tables (`edit::lowered`/`edit::raised`), as `tolower()` and
  `toupper()`: `cru` on `straße` gives `STRAẞE`.
- Carets whose ranges overlap coerce once; ranges that only touch (`2cr` from adjacent words) stay apart, each
  coerced as its own string, so camel's "has a `_`" test reads one word.
- With no caret's text changed there is no undo step, but carets still go to the word start and `'[` `']` are
  set, as the case operators set them over unchanged text.
