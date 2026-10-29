---
task_uid: "01m5qnxvt1m45pyj4adf2fygda"
task: "T1148"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fastedit test vim_lazyvim_preset_tests::lazyvim_preset_parses_cleanly: parse_vimrc of the bundled file reports no issue and every \u003cCmd\u003efe:id\u003cCR\u003e id is a command the catalog knows"
    required: true
    manual: false
  - id: "C2"
    text: "vim_lazyvim_preset_tests::lazyvim_layout_is_the_table: the file's (keys, command) pairs equal the test's table for find, search, explorer, buffer, code, git, problems, window and quit keys (the card body lists them)"
    required: true
    manual: false
  - id: "C3"
    text: "vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent: each LazyVim key the file lists as unmapped is absent from the set"
    required: true
    manual: false
  - id: "C4"
    text: "vim_lazyvim_preset_tests::space_ff_runs_quick_open: with the preset on, HeadlessVim typing \u003cSpace\u003eff leaves the fe:quick_open.toggle invocation, and \u003cSpace\u003ew waits for the window group's next key"
    required: true
    manual: false
---

# vim.preset lazyvim: LazyVim's core leader layout (find, search, explorer, buffers, code, git, problems, windows, quit) mapped to existing fastedit commands

## Problem

The user said on 2026-10-24 that LazyVim's default leader menu ships as an opt-in preset, `vim.preset = "lazyvim"` (T1146 is the mechanism). This card is the preset's content: LazyVim's core leader layout mapped to fastedit commands that exist, as a bundled vimrc-subset file. Space is the leader; the which-key popup (T1137) shows the menu.

## Current state

`crates/fastedit/presets/lazyvim.vimrc` (T1146) holds one line. The keymap's command ids that fit LazyVim's layout exist (`crates/fe-keymap/keymaps/fastedit.toml`, `crates/fe-workbench/src/workbench_commands.rs`); a map is `nnoremap <leader>ff <Cmd>fe:quick_open.toggle<CR>`.

## Change

Fill `lazyvim.vimrc` (normal mode, `nnoremap`, plus `xnoremap` where LazyVim has a visual form), only with commands the catalog knows:

- find: `<leader>ff` and `<leader><space>` `quick_open.toggle`; `<leader>fr` `workbench.open_recent`; `<leader>fn` `workbench.new_file`; `<leader>fp` `workbench.open_folder`; `<leader>ft` and `<leader>fT` `terminal.toggle`.
- search: `<leader>/` and `<leader>sg` `search.show`; `<leader>sr` `search.show_replace`; `<leader>ss` `outline.show`; `<leader>sS` `nav.workspace_symbols`; `<leader>:` and `<leader>sc` `command_palette.toggle`.
- explorer, buffers: `<leader>e` and `<leader>E` `dock.show_project`; `<leader>bd` `workbench.close_tab`; `<leader>bb` and `<leader>,` `workbench.next_tab`.
- code: `<leader>cr` `lsp.rename`; `<leader>ca` `lsp.code_actions`; `<leader>cf` `lsp.format_document` (and visual `lsp.format_selection`); `<leader>cs` `outline.show`; `<leader>co` `lsp.organize_imports`; `<leader>cd` `diagnostics.next`.
- git: `<leader>gg` and `<leader>gs` `git.show_changes`; `<leader>gb` `git.toggle_blame_annotations`; `<leader>ghs` `git.stage_hunk`; `<leader>ghr` `git.revert_hunk`.
- diagnostics: `<leader>xx` `problems.show`.
- windows (LazyVim's `<leader>w` is the window group): `<leader>-` `pane.split_down`; `<leader>|` `pane.split_right`; `<leader>wd` `pane.close`; `<leader>ww` `pane.focus_next`; `<leader>w=` `pane.equalize`.
- window navigation (LazyVim keymaps.lua, normal mode): `<C-h>` `<C-j>` `<C-k>` `<C-l>` move focus left/down/up/right, reaching the side and bottom docks as Track A T1132/T1133 do (lead decision 2026-10-24: bare ctrl-h/j/k/l stay unbound in plain vim mode and come only with this preset). Depends on T1133.
- quit, ui: `<leader>qq` `app.quit`; `<leader>uw` `editor.toggle_soft_wrap`.

The unmapped LazyVim keys (no fastedit command) go in a comment block at the file's end and in the PR body: picker-backed ones (`<leader>sk` keymaps, `<leader>sh` help, `<leader>sd` diagnostics picker, `<leader>sb` buffer grep, `<leader>fb` buffer list, `<leader>fg` git files, `<leader>fc` config files), `<leader>l` lazy, `<leader>L` changelog, `<leader>K`, `<leader>cm` mason, `<leader>xl/xq/xt` (location, quickfix, todo lists), `<leader>ur`/`<leader>ud`/`<leader>un` toggles (redraw, diagnostics, notifications), `<leader>wm` zoom, `<leader>bo`, `<leader><tab>` tab pages and `<leader>qs` sessions. Check each id against the catalog; an id that does not exist moves to the unmapped list.

## Tests

New `crates/fastedit/src/vim_lazyvim_preset_tests.rs` (declared by one `#[path]` line in `vim_remaps.rs`): the file parses with no issue; every id is known; a table of (keys, command) equals the file (so the list above is the contract); each unmapped key listed in the file is absent from the set; with the preset on and the leader `<Space>`, `HeadlessVim` typing `<Space>ff` leaves the `fe:quick_open.toggle` invocation for the host.
