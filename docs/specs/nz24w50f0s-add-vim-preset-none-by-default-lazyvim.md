---
task_uid: "01m5qphz51ackdc6nz24w50f0s"
task: "T227"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Integration test vim_preset_setting (top level, crates/fe-config/tests/vim_preset_setting.rs): vim.preset defaults to none, accepts lazyvim, rejects another value with a located schema error; the schema golden and template tests pass"
    required: true
    manual: false
  - id: "C2"
    text: "fastedit test vim_preset_tests::preset_off_is_plain_vim: with preset none the RemapSet has no preset maps and the leader is the default backslash"
    required: true
    manual: false
  - id: "C3"
    text: "vim_preset_tests::preset_sits_under_the_users_maps_and_sets_its_leader: lazyvim loads its maps with leader \u003cSpace\u003e; a vimrc or [[vim.remap]] map of the same keys replaces the preset's; an explicit vim.leader beats the preset's"
    required: true
    manual: false
  - id: "C4"
    text: "vim_preset_tests::bundled_presets_name_real_commands: every \u003cCmd\u003efe:id\u003cCR\u003e in every bundled preset file is a command the catalog knows, and a preset issue shows in the config problems list against the preset's name"
    required: true
    manual: false
  - id: "C5"
    text: "A settings reload that changes vim.preset rebuilds the set in open editors (vim_preset_tests::reload_applies_the_preset); existing vim_remaps tests pass unchanged"
    required: true
    manual: false
---

# Add vim.preset (none by default, lazyvim): load a bundled remap file beneath the user's vimrc and [[vim.remap]], with the preset's leader unless vim.leader is set (Mac T1146)

## Problem

The user said on 2026-10-24: Neovim de-facto conventions ship as vim-mode defaults, but LazyVim's own opinions (its default leader menu) must be an opt-in preset, "e.g. `vim.preset = "lazyvim"`; default is plain-vim leader behaviour". This card is the mechanism: a `vim.preset` setting that loads a bundled remap file beneath the user's own. The next card fills the LazyVim file.

## Current state

- `VimSettings` (`crates/fe-config/src/settings.rs:229`) has `leader` (default `\`) and `remap`; remaps load in `crates/fastedit/src/vim_remaps.rs`: the vimrc's records, then `[[vim.remap]]`, built into one `RemapSet` (`:136-185`) and handed to `fe_gpui::set_vim_remaps(cx, remaps, leader)` (`crates/fe-gpui/src/vim.rs:526`). A later map for the same keys replaces an earlier one, so a preset placed first yields to the user's vimrc and settings.
- `parse_vimrc` (`crates/fe-vim/src/vimrc.rs`) reads `nnoremap` lines with `<leader>` and `<Cmd>fe:{command id}<CR>` (`remap.rs:66`); see `docs/specs/vsckbjpvkj-load-vim-remap-and-vim-leader-from.md`.

## Change

- `VimSettings.preset: VimPreset` (`"none"` default, `"lazyvim"`), schema and template comment (settings.rs, settings.schema.json, settings-template.toml).
- New `crates/fastedit/presets/lazyvim.vimrc`, a vimrc-subset file (here one line, `nnoremap <leader>w <Cmd>fe:editor.save<CR>`, so the mechanism can be tested; the next card fills it), included with `include_str!` by `vim_remaps.rs`.
- `vim_remaps.rs`: with a preset, its parsed records come first, before the user's vimrc and `[[vim.remap]]`; its issues are reported as config problems against the preset file name; a settings reload that changes `vim.preset` rebuilds the set live. `vim.leader`: while it is the default `\`, a preset's leader (`<Space>` for LazyVim) applies; an explicit other `vim.leader` wins. Plain vim (`preset = "none"`) is unchanged: no map, leader `\`.
- Test helper (new `crates/fastedit/src/vim_preset_tests.rs`, declared by one `#[cfg(test)] #[path]` line in `vim_remaps.rs`): every `<Cmd>fe:{id}<CR>` in a bundled preset must name a command the catalog knows (`fe_gpui::known_command`, `crates/fe-gpui/src/vim.rs:772`), so a rename fails the test.

## Tests

`crates/fastedit/src/vim_preset_tests.rs`: preset off gives no maps and leader `\`; lazyvim loads its maps with leader `<Space>`; a user `nnoremap <leader>w :w<CR>` in the vimrc replaces the preset's; an explicit `vim.leader = ","` wins; unknown command ids are caught. New `crates/fe-config/tests/vim_preset_setting.rs`: default, enum values, bad value is a located schema error.

## Implementation notes

- Presets are `vim_remaps::PRESETS` (name, text, leader); `vim.preset = "lazyvim"` loads `lazyvim.vimrc` with leader `<Space>`. Its problems, and any keymap conflict of its insert or cmdline maps, are config problems against the file name `lazyvim.vimrc`, at their lines.
- The leader in use is `vim.leader`, or the preset's while `vim.leader` is the literal default `"\\"`. The preset is parsed with `<leader>` as that leader's keys, through a new `fe_vim::parse_vimrc_with_leader` (review round 1: routing it through a Vimscript `let mapleader` string turned `<C-i>` into Tab, `<C-m>` into Enter and an empty leader into `\`, so preset and `[[vim.remap]]` maps disagreed).
- With a preset, the vimrc's `<leader>` also starts as the leader in use, as LazyVim sets `mapleader` before the user's config, so a vimrc `nnoremap <leader>w :w<CR>` replaces the preset's `<leader>w`; a `let mapleader` in the vimrc still changes it from there. With no preset the vimrc's leader starts as `\` whatever `vim.leader` is, as before. So the vimrc is now kept as text and parsed when the set is built, not when it's read.
- The command check uses `fe_workbench::command_catalog()`, the catalog fastedit checks keymap bindings against, rather than `fe_gpui::known_command`, which is private and needs an app.
