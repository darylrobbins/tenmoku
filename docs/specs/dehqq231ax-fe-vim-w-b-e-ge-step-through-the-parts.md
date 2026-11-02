---
task_uid: "01m6c9t3z38kf1vbdehqq231ax"
task: "T265"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test subword_motions_step_through_names (crates/fe-vim/tests/subword_motions.rs) passes: with set_subword_motions(true), w over 'fooBarBaz HTTPServer snake_case utf8Decode' stops at each subword start the card's rules give (Bar, Baz, HTTP, Server, snake, case, utf, 8, Decode), b and ge go back through them, e stops on each subword's last char, and counts take that many"
    required: true
    manual: false
  - id: "C2"
    text: "Test subword_motions_equal_word_motions_on_plain_words passes: on texts whose keyword runs have no inner boundary, each subword motion alone, under d c y with counts, in visual mode and repeated with ., gives exactly what its word motion gives (including dw at a row's end, cw as ce, and a count running out at the buffer's end)"
    required: true
    manual: false
  - id: "C3"
    text: "Test subword_motions_under_operators passes: dw, cw, d2e and dge inside fooBarBaz and snake_case_name take the subword spans the card's rules give, and . repeats them"
    required: true
    manual: false
  - id: "C4"
    text: "Test subword_motions_off_by_default passes: without set_subword_motions w b e ge are word motions; W B E gE are WORD motions either way"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-vim passes, every *_matches_neovim corpus test unchanged"
    required: true
    manual: false
---

# fe-vim: w b e ge step through the parts of camelCase, PascalCase, snake_case and digit-joined names while VimState::set_subword_motions is on, under operators, counts, visual mode and dot-repeat as word motions do (Mac T1290)

## Problem

The user approved CamelCase / subword motions (CamelCaseMotion, vim-wordmotion, nvim-spider) as a built-in vim
feature on 2026-11-01, preferring opt-in, non-conflicting defaults. fe-vim has no subword motion
(`rg -i 'subword|camel' crates/fe-vim` finds nothing).

Key design (decided here): w b e ge themselves step by subwords, but only when switched on
(`VimState::set_subword_motions`, off by default; the `vim.subword_motions` setting is a separate card).
- CamelCaseMotion's documented `,w ,b ,e` would shadow `,` (repeat the last `f`/`t` backwards) with a 1 s
  timeout on every `,`; `<leader>w`-style keys collide with the LazyVim preset (`<leader>w`, `<leader>b`,
  `<leader>e`).
- Off by default, plain vim and the nvim oracle corpora are unchanged; on, `W B E gE` still move by WORDs,
  as with vim-wordmotion and nvim-spider's usual setup.
- fe-vim remaps are key-to-key (`crates/fe-vim/src/remap.rs`), so a motion with no key of its own couldn't be
  remapped to; the setting is the opt-in. The text objects (`iS` `aS`) are a separate card.

## Change

Subwords, in `crates/fe-vim/src/feature/subword.rs` (new, declared in `crates/fe-vim/src/feature.rs`; it defines
no `FEATURE`): inside a run of keyword chars (fe-vim's word class), a subword boundary falls
- between a lowercase letter and an uppercase one (`fooBar` → `foo|Bar`);
- before the last uppercase letter of an uppercase run followed by a lowercase one (`HTTPServer` → `HTTP|Server`);
- between a letter and a digit, either way (`utf8Decode` → `utf|8|Decode`);
- around each run of `_` inside the keyword run. The `_` run belongs to no subword: motions pass over it as they
  pass over blanks (`w` on `snake_case` stops on `c`). A keyword run that is only `_` is one subword.
Letter case is Unicode's (`char::is_uppercase`/`is_lowercase`). Non-keyword runs, blanks, empty rows and row ends
are exactly as for `w b e ge`.

Motions: new `Motion::SubwordForward`, `SubwordBackward`, `SubwordEnd`, `SubwordEndBackward` (`crates/fe-vim/src/args.rs`,
serde `subword_forward` …), classed, counted and failing as `WordForward`, `WordBackward`, `WordEnd` and
`WordEndBackward` are. Make it hold by construction: generalise the `big: bool` that `motion::Cursor::words`
and its callers take into a granularity (word, WORD, subword) and pass it through every place that special-cases
the word motions: `crates/fe-vim/src/motion.rs` (`word_fails`, the windowed-read list), `crates/fe-vim/src/operator.rs`
(the exclusive `w` rule under an operator, `cw` as `ce`, `Span::Move` for a count that runs out after moving),
`crates/fe-vim/src/insert.rs` (`change_word`), `crates/fe-vim/src/line_motions.rs` (`ge`),
`crates/fe-vim/src/visual.rs` (the row-end `b` and `e` cases) and `crates/fe-vim/src/g_operators.rs` (the `gq`/`gw`
`w` rule). So `dw`, `cw`, `yw`, `3w`, `d2e`, `vge` and `.` on a subword behave as on a word.

Keys: `VimState::set_subword_motions(on)` (defined in `feature/subword.rs` as `jump.rs` defines
`set_jump_on_s`; its flag a field in `crates/fe-vim/src/state.rs`), and `HeadlessVim::set_subword_motions`
(`crates/fe-vim/src/headless.rs`). While on, the reducer (`crates/fe-vim/src/reducer.rs`, the `'w'`/`'b'`/`'e'`
arms and `g` `e`) gives the Subword variants in normal, visual and operator-pending modes; `Pending` carries the
flag, set by `VimState::feed`. `vim.move` with a subword motion works whether it is on or off.

No nvim oracle exists for subwords: plain tests in a new `crates/fe-vim/tests/subword_motions.rs`. Check every
helper on empty rows (codex bounds-checks them).

Note: `crates/fe-vim/src/visual.rs` is in T1274's scope and `state.rs` in T659's; both are lent (helium, neon) and
blocked here, so expect a textual merge when their stacks land. Ready this card after helium's T1274 lands if you'd
rather avoid it.

## Implementation notes

- `motion::Granularity` (`Word`, `BigWord`, `Subword`) replaces `big: bool` in `Cursor::words` and `ge`'s
  `word_ends_back`; `Motion::word()` maps each of the twelve word motions to its base motion (`WordForward`,
  `WordBackward`, `WordEnd`, `WordEndBackward`) and granularity, and every special case listed above matches on that,
  so a subword motion can't miss one. The text objects (`iw` `aW`) keep their `big: bool`.
- The word loops compare `Cursor::unit` (the class, but `Blank` for a `_` run that subwords pass over) and also stop
  where `Cursor::splits` says a subword ends inside a keyword run. For `Word` and `BigWord`, `unit` is `class` and
  `splits` is false, so those motions are unchanged.
- `subword::passed_over` measures a `_` run once and caches it on the `Cursor` (`underscores`), so stepping along a
  long `_` run stays linear.
- Known limit: on a huge row read in windows (over 64 KiB), a keyword run cut by a window edge without a class change
  (`motion::inert_edge`, only past 128 KiB with no class change) can't see across the edge: the `HTTP|Server` rule and
  the `_`-run check there see only the window.
- `subword_motions_equal_word_motions_on_plain_words` reuses one `HeadlessVim` per setting, swapping in a fresh model,
  `VimState` and registers before each case: building the registry takes ~1 ms, more than the cases themselves.
