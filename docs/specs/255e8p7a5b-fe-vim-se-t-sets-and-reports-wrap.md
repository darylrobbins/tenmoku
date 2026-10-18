---
task_uid: "01m55ce5m5pdqb6s255e8p7a5b"
task: "T147"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test set_reports_options_as_nvim_does (top level, in crates/fe-vim/tests/ex_set.rs) passes: through HeadlessVim, :set ts?, :set ts, :set sw?, :set et?, :set wrap?, :set nowrap?, :set clipboard? and :set wrap? after :set invwrap leave nvim 0.12.x's exact messages (as probed, in the spec), and :set ts=8 sw=2 then :set ts? reports 8."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test set_changes_the_editor (top level, in crates/fe-vim/tests/ex_set.rs) passes: each form (opt, noopt, invopt, opt!, opt=N, opt:N, several in one :set) changes EditorModel::soft_wrap(), tab_size() and indent_style() and the register clipboard flag as the spec says, and the refused forms (noexpandtab with shiftwidth not 0 or tabstop, opt\u0026, opt+=N, :set alone, an option fastedit doesn't model) leave a message and change nothing."
    required: true
    manual: false
  - id: "C3"
    text: "The integration test ex_set_matches_neovim (top level, in crates/fe-vim/tests/ex_set_oracle.rs) passes on crates/fe-vim/tests/corpus/ex_set.toml, at least 10 cases recorded with nvim 0.12.x, each a :set then an edit it changes (\u003e\u003e after sw=2, \u003e\u003e after noet with ts=4 sw=4, \u003e\u003e after et with sw=4, a ts out of range failing), and :set lives only in crates/fe-vim/src/ex/set.rs and its table.rs entry."
    required: true
    manual: false
---

# fe-vim: :se[t] sets and reports wrap, tabstop, shiftwidth, expandtab and clipboard on the editor, with nvim's messages, instead of E492 (Mac T825)

Filed on argon from Mac T825 (card ~/coordination/argon-cards/zzk/T825.json rev 1; note zzk, the Vim breadth track). Mac task uid 01m55bjcs2rq19z5vnz94233e5. Base: argon trunk (mac/main 76125d0 synced). Behaviour probed with nvim --clean --headless 0.12.6. Argon dep: T146 (the table chain adds its entries to ex/table.rs one at a time). Title the host commit "... (Mac T825)".

## Problem

`:se[t]` is E492 in fe-vim at Mac main 92c57c57 (listed as not planned). The user approved it on
2026-10-17 in the "Vim breadth" track, "only what fastedit's settings model supports". fastedit's editor
model carries exactly these of nvim's options, per editor: soft wrap (`EditorModel::set_soft_wrap`,
nvim's `wrap`), the tab width (`set_tab_size`, `tabstop`), the indent unit (`set_indent_style`:
`IndentStyle::Spaces(n)` is `expandtab` with `shiftwidth=n`, `Tabs` is `noexpandtab` with shiftwidth
equal to tabstop), and fe-vim's `Registers::set_clipboard_unnamed` (`clipboard=unnamedplus`, or empty).
Probed with `nvim --clean --headless` 0.12.6 (ts=4 sw=4 et, the oracle's settings), the messages
(`execute()`):

- `:set ts?` gives `  tabstop=4`, and so does `:set ts`; `:set sw?` `  shiftwidth=4`; `:set et?`
  `  expandtab`; `:set wrap?` `  wrap`, and after `:set invwrap` `nowrap`; `:set clipboard?`
  `  clipboard=`. (Each is preceded by a line break in `execute()`'s capture: the message is the text
  after it.)
- `:set ts=8 sw=2` sets both, no message; then `:set ts?` gives `  tabstop=8`.
- `:set nowrap?` reports, not sets: `  wrap` while wrap is on.
- `:set foo` is `E518: Unknown option: foo`.

## Change

New `crates/fe-vim/src/ex/set.rs`, an entry in `crates/fe-vim/src/ex/table.rs` (the seam Mac T822 adds;
`set`, shortest `se`, edits: false). `:se[t] {arg} …`, each blank-separated argument in turn, stopping at
the first that fails with its error (those before it stay set, as in nvim):

- Options: `wrap`; `tabstop`/`ts` (1 to 16, as the editor settings' `tab_size` allows; outside it nvim's
  error for a bad number); `shiftwidth`/`sw` (0 meaning tabstop); `expandtab`/`et`; `clipboard`
  (`unnamedplus` or empty; `unnamed`, the selection register, refused as not supported).
- Forms (`:help :set`): `{bool}`, `no{bool}`, `inv{bool}` and `{bool}!` set, clear and toggle; `{opt}?`
  (and a number or string option alone) leaves nvim's message in `VimState::message`, the formats above;
  `{opt}={value}` and `{opt}:{value}` set.
- What fastedit can't hold is refused with a message, changing nothing: `noexpandtab` (or `sw=N` under
  it) when shiftwidth isn't 0 or equal to tabstop (nvim would mix tabs and spaces there; fe-vim's indent
  unit is one or the other), `{opt}&` and `{opt}+=`/`-=`/`^=`, `:set` alone and `:set all`.
- Any other option name: "{name}: not supported yet" (`ExError::Unsupported`), not E518, since most such
  names are real nvim options fastedit doesn't model.
- These change the editor, not the saved settings; the host may set them again (a settings reload). A
  range is nvim's error (probe `:2set ts?`).

Tests: a new `crates/fe-vim/tests/ex_set.rs` through `HeadlessVim` for the messages above (exact strings,
probed), the model's state after each form (`soft_wrap()`, `tab_size()`, `indent_style()`, the register
clipboard flag) and the refusals; and the oracle pattern for what `:set` does to later edits: a new corpus
`crates/fe-vim/tests/corpus/ex_set.toml` (header comment as the other corpora; an error ends a case),
recorded into `ex_set.expected.json` by the ignored `record_corpus_with_neovim` in `tests/oracle.rs`, and a
new `crates/fe-vim/tests/ex_set_oracle.rs` modelled on `tests/ex_lines_oracle.rs`. Touch nothing in ex.rs,
lib.rs or fe-gpui.

## Lead decision (argon, 2026-10-18, review round 2)

- F1: the mapping onto the editor's single `IndentStyle` is accepted as the spec's model:
  - `sw=0` is `None` and follows ts;
  - `sw=N` is `Spaces(N)`;
  - `noet` is allowed only when sw is ts or 0;
  - `ts` is refused under `noet`.
  
  Distinguishing `sw=0` from a fixed sw equal to ts would need a separate shiftwidth field on `EditorModel`. That is outside this card, and no follow-up is filed.
- F3: C3 no longer asks for "an insert after et". `HeadlessVim` forwards insert-mode `<Tab>` to the host, and fe-vim has no insert-time use of these options, so the oracle can't record such a case.

## Implementation notes

- `>>`, `:>` and the blockwise `>` did not read the editor's indent style (spaces, one tab size wide); C3 needs them to, so `operator::shift_unit` and `indent_fill` now give them shiftwidth and tabs under `noexpandtab`.
- The editor has no separate "shiftwidth=0": `sw=0` is the unset indent style (`None`, following tabstop), and `:set sw?` reports tabstop then, not 0. Under `noexpandtab` shiftwidth is tabstop, so `:set ts=N` is refused there (set `expandtab` first).
- `HeadlessVim` forwards `<Tab>` in insert mode, and nothing else in insert mode reads these options without a language, so the corpus has an insert followed by a shifted line, not a typed Tab.
- `tests/support/oracle.lua` resets ts, sw, et, clipboard and wrap before each case, since `:set` outlives its buffer.
- `Registers::clipboard_unnamed()` is new, for `:set clipboard?`.
