---
task_uid: "01m6svc843gv1dp3j99edq4r5n"
task: "T492"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "quickfix_panel_tests::copen_shows_the_panel_with_the_list_rows and quickfix_panel_tests::cclose_hides_the_panel"
    required: true
    manual: false
  - id: "C2"
    text: "quickfix_panel_tests::cnext_moves_the_highlight_and_scrolls"
    required: true
    manual: false
  - id: "C3"
    text: "quickfix_panel_tests::enter_on_a_row_opens_the_item_at_its_position"
    required: true
    manual: false
  - id: "C4"
    text: "quickfix_panel_tests::lopen_shows_the_panes_location_list"
    required: true
    manual: false
  - id: "C5"
    text: "ex_quickfix_window::copen_and_lopen_parse_to_quickfix_open and ex_quickfix_window::cclose_and_lclose_parse_to_quickfix_close; the T-day-one assertion in ex_day_one.rs no longer expects OpenProblems for :copen"
    required: true
    manual: false
  - id: "C6"
    text: "vim_lazyvim_preset_tests::lazyvim_layout_is_the_table (with rows for \u003cleader\u003exq, \u003cleader\u003exl, ]q, [q) and vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent pass"
    required: true
    manual: false
  - id: "C7"
    text: "cargo clippy --workspace --all-targets -- -D warnings and cargo nextest run -p fe-vim -p fe-workbench -p fastedit pass"
    required: true
    manual: false
---

# Show the quickfix and location lists in a panel for :copen and :lopen, with LazyVim's <leader>xq, <leader>xl, ]q and [q (Mac T1406)

# Show the quickfix and location lists in a panel for :copen and :lopen, with LazyVim's <leader>xq, <leader>xl, ]q and [q

## Problem
`:copen` and `:lopen` still show the Problems panel, which has nothing of the quickfix list in it. There is nowhere to see, click or pick from the list the earlier cards fill, and LazyVim's `<leader>xq`, `<leader>xl`, `]q` and `[q` are unmapped (lazyvim.vimrc lists `xl` and `xq` as unmapped).

## Change
- New `crates/fe-workbench/src/quickfix_panel.rs`: a dock panel, registered as `references_panel.rs` registers its sidebar (`SIDEBAR`) and rendered like its flat rows: one row per item, `path:row:col  text` with the file name dimmed, severity tint, the current item highlighted and scrolled into view as `:cnext` moves, click or Enter opens the item (the landing code of the model card), the title (`:grep foo`, `:make`) and `n items` in the header. The panel shows the quickfix list, or the focused pane's location list for the `l` forms; a header switch is not needed. Vim keys follow the references panel's `vim` key-context flag.
- Commands in `quickfix_commands.rs`: `quickfix.show`, `quickfix.toggle`, `location.show`, `location.toggle`. fe-vim: `:copen`/`:cwindow` and `:lopen`/`:lwindow` now parse to `HostCommand::Quickfix(Open { location })` and `:cclose`/`:lclose` to `Close { location }` (ex.rs line 883's `OpenProblems`/`CloseProblems` arms are replaced for these names; `OpenProblems` stays for nothing else, so remove it if it ends up unused, in both host matches). Update the existing `crates/fe-vim/tests/ex_day_one.rs` assertion at line 71, which expects `:copen` to be `OpenProblems`.
- crates/fastedit/presets/lazyvim.vimrc (the preset tests fail if the table and the file disagree, so both change together): `nnoremap <leader>xq <Cmd>fe:quickfix.toggle<CR>`, `<leader>xl` -> `location.toggle`, `]q` -> `quickfix.next`, `[q` -> `quickfix.previous`; remove `xl` and `xq` from the "Unmapped" comment list. Rows added to `TABLE` in `crates/fastedit/src/vim_lazyvim_preset_tests.rs`.
- Run Tasks with failures still use the Problems panel; `<leader>xx` stays on Problems (LazyVim's Trouble diagnostics).

## Tests (new files)
- `crates/fe-workbench/src/quickfix_panel_tests.rs` (`quickfix_panel_tests::...`): `copen_shows_the_panel_with_the_list_rows`, `cnext_moves_the_highlight_and_scrolls`, `enter_on_a_row_opens_the_item_at_its_position`, `lopen_shows_the_panes_location_list`, `cclose_hides_the_panel`.
- `crates/fe-vim/tests/ex_quickfix_window.rs` (bare ids): `copen_and_lopen_parse_to_quickfix_open`, `cclose_and_lclose_parse_to_quickfix_close`.
- The existing `vim_lazyvim_preset_tests::lazyvim_layout_is_the_table` (its `TABLE` gains the four rows) and `vim_lazyvim_preset_tests::unmapped_keys_are_listed_and_absent` (it fails if a key stays in the Unmapped list after being mapped) pass.
- Screenshot: none.

## Implementation notes

- Scope widened (respecs 2 and 3): registering a sidebar panel needs `dock.rs` (its `DockPanel`, `PANELS`, `DockOp::ShowQuickfix`, `Dock::set_quickfix`) and `sidebar_panels_tests.rs` (the panel list); the Workbench's typed commands live in its one `#[kiln::command]` block in `workbench_commands.rs`, which calls into `quickfix_commands.rs`; `crates/fe-vim/tests/ex_quickfix.rs` asserted `:copen` was `OpenProblems`. `lib.rs` is claimed by another task, so `quickfix_panel.rs` is declared from `dock.rs` with `#[path]`, as `problems_panel.rs` is: the tests' full path is `dock::quickfix_panel::quickfix_panel_tests::…`.
- `]q` and `[q`: the default keymap bound `] q` / `[ q` to `diagnostics.next` / `previous` in an editor in vim's normal mode (the Problems list "in quickfix's place"), and the keymap wins over vim maps (`docs/decisions/vim-remaps.md` rule 1), so the preset's `]q` would never have run. They are now `quickfix.next` / `quickfix.previous`, as nvim 0.11's defaults (`:cnext`, `:cprevious`), with or without the preset (`crates/fe-keymap/keymaps/fastedit.toml`, and its tests `vim_bracket_pairs.rs` and `day_one_bindings.rs`, respecced in). `]d` / `[d` stay on diagnostics.
- `HostCommand::OpenProblems` and `CloseProblems` were used by nothing else, so they're gone; `:copen` and friends are `QuickfixAction::Open` / `Close` with `QuickfixCommand::location`. Checked against nvim 0.12.4: `:copen [N]` and any range are a height (accepted, unused), `:copen 0` E939, `!` E477 for all four, `:cclose` takes no range (E481) or argument (E488). `:cwindow` / `:lwindow` parse to `Open` as the spec says: the panel shows an empty list as it shows one with items, where nvim's `:cwindow` would close the window for a list with no valid items.
- `:lopen` in a pane with no location list says `E776: No location list` and shows nothing, as nvim does. The panel keeps showing the location list of the pane that opened it; `:lclose` hides it only when it shows the active pane's list, `:cclose` only when it shows the quickfix list. A row picked from a location list lands in its pane, activated first.
- Keys are handled in the panel's own key-down listener (Up/Down/Enter; with vim on `j`, `k`, `gg`, `G`, `o`), so no keymap bindings or binding-count changes; the key context is `quickfix_panel`, with the `vim` flag while vim is on.
- Carried T482 F5: `collection_len` now skips `[:name:]`, `[=x=]`, `[.x.]` and nested `[…]` sets inside a collection; a nested set that never closes means the enclosing one can't, so the scan stays linear per collection, and nesting past 32 levels makes `[` a plain member (no unbounded recursion). Tests in `ex_quickfix_window.rs` (`vimgrep_collections_hold_classes_and_nested_sets`), since `ex_quickfix_fill.rs` isn't in scope.
