---
task_uid: "01m7mypbyzk9ssm1604xvr5pqt"
task: "T413"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "Test vim_remaps::vim_lazyvim_preset_tests::lazyvim_layout_is_the_table passes with rows for every §6 \u003cleader\u003ed key (cargo nextest run -p fastedit --bin tenmoku vim_lazyvim)"
    required: true
    manual: false
  - id: "C2"
    text: "Test vim_remaps::vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent passes with \u003cleader\u003edg and \u003cleader\u003edB in the unmapped list"
    required: true
    manual: false
  - id: "C3"
    text: "Tests in vim_remaps::vim_lazyvim_debug_tests pass: each_debug_key_dispatches_its_command under vim.preset = \"lazyvim\", and leader_do_steps_out_and_leader_dO_steps_over (LazyVim's meaning); \u003cleader\u003ede is mapped in normal and visual; the dispatch is proved with HeadlessVim over the settings-loaded preset (vim.preset = \"lazyvim\" through the settings loader): a window-level test that feeds the keys through a gpui editor (fe-gpui/fe-workbench, outside this card's scope; fastedit has no harness for it) is a KNOWN LIMIT carried by a follow-up card"
    required: true
    manual: false
  - id: "C4"
    text: "The preset's commands all exist (vim_preset_tests' unknown-commands check passes)"
    required: true
    manual: false
---

# LazyVim preset: LazyVim's <leader>d debug keys (debugger D12) (Mac T1855)

# LazyVim preset: LazyVim's <leader>d debug keys (debugger D12)

Source: `docs/decisions/r1b9dw3g0g-debugger.md` (the debugger record, approved with amendments 2026-11-13; §13 wins over §§1-12). Size **s**.

## Problem

LazyVim users expect `dap.core`'s `<leader>d` keys.

## What to build (§6 "The LazyVim preset"; §11 D12)

A `" debug` block after `" diagnostics` in `crates/fastedit/presets/lazyvim.vimrc`
with §6's table for the commands D7-D11 add. `<leader>dg` and `<leader>dB` go in the
"Unmapped LazyVim keys" list (D17 maps `<leader>dB`). `<leader>td` is D13's. Note
§6's correction: `<leader>do` is Step **Out**, `<leader>dO` Step **Over**, Pause is
`<leader>dP`.

## Not in this card

`<leader>td` (D13), `<leader>dB` (D17).

## Conventions for every debugger card (§13)

- **Strings.** Every user-visible string goes through the catalogue
  (`docs/decisions/xte5e7hqsw-i18n-design.md` §§ 4-5): `tr!` / `text!`, whole
  messages, plural selectors for counts. Workbench keys are `workbench-debug-…` in
  `crates/fe-workbench/l10n/en-US/debug.ftl` (the first card that needs the file
  adds it to `resources!` in `src/l10n.rs`); fe-dap's are in
  `crates/fe-dap/l10n/en-US/debug.ftl`; fe-gpui's in `crates/fe-gpui/l10n/en-US/debug.ftl`.
  A new command's title is the key `command-<id>` in the same file. Tooltips and
  accessible names count. Tests build expected text through the catalogue.
- **Accessibility.** Every control this card adds has an accessible name (an
  icon-only button names itself), and session changes are announced through
  `kiln_ui::a11y::announce` ("Stopped at line N", "Running", "Session ended").
- **Tests.** New tests go in new files, following today's placement:
  `src/tests/<name>.rs` declared from `src/tests/mod.rs` (ids `tests::<name>::<test>`),
  or an integration test under `tests/` for public API (ids bare, no binary prefix).
- **Design calls** go in your decision file `docs/decisions/<lane>-*.md` (exempt
  from scope). Where §13 and §§1-12 of the record disagree, §13 wins.

Platform: **Linux-OK** (headless tests only; no screenshot goldens move).
