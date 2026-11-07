---
task_uid: "01m6svbvrf84ekd4eg7qdew5sn"
task: "T489"
rev: 2
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "vim::which_key_icons_tests::every_table_icon_is_a_bundled_icon and ::every_table_command_is_a_catalog_id pass"
    required: true
    manual: false
  - id: "C2"
    text: "a_group_icon_is_carried (in crates/fe-vim/tests/which_key_groups.rs) passes"
    required: true
    manual: false
  - id: "C3"
    text: "vim::which_key_icons_tests::a_row_without_an_icon_keeps_its_title_aligned and ::icons_use_theme_colours_only pass"
    required: true
    manual: false
  - id: "C4"
    text: "vim_lazyvim_groups_tests::every_named_group_has_an_icon passes"
    required: true
    manual: false
  - id: "C5"
    text: "the which_key_icons scene is added to crates/fe-gpui/examples/screenshots and `cargo build -p fe-gpui --example screenshots` builds on Linux; the golden is recorded on the Mac when this lands"
    required: true
    manual: false
---

# Draw an icon per which-key row (Lucide, themed colours), with per-group icons in :fegroup and the lazyvim preset's defaults (Mac T1396)

## Problem

The user (2026-11-06): "They have icon support." Every row in LazyVim's which-key has a coloured icon, per item
and per group (search glass for find, bug for debug, git mark for git, a window glyph for windows). Ours has none.

## Change

1. **Icon on an entry.** `WhichKeyEntry` gains `icon: Option<String>`, a name from the bundled UI set. Sources,
   most specific first:
   - a group's icon from the group record (card B): `:fegroup {keys} {name}` takes an optional trailing
     `icon={name}`; `[[vim.which_key_group]]` an optional `icon = "..."`;
   - an item's icon from its command id, via a table `command_icon(&str) -> Option<&'static str>` in fe-gpui
     (`quick_open.toggle` search, `search.show` search, `terminal.toggle` terminal, `pane.split_*` columns/rows,
     `git.*` git-branch, `lsp.*` code, `workbench.*_tab` files, `app.quit` x, ...). Commands not in the table
     draw no icon but keep the space so titles still line up.
2. **Icons come from the set already bundled for UI chrome**: Lucide (ISC), `kiln/crates/kiln-ui/assets/icons/`,
   drawn through `kiln_ui::file_icons::Icon` as a mask in a text colour. The Material set is file/folder-themed
   and multi-colour, so it isn't used. Add the missing Lucide SVGs unmodified (bug, save, terminal, settings,
   undo, panels, play, flask, bot at least; keep each under 2 KiB) and list them in the `Icon` enum, LICENSE/NOTICE
   are already covered by the set.
3. **Colour.** Tint an icon from the theme's semantic colours (`information`, `warning`, `error`, `success`,
   `link` if present), chosen per icon by a fixed table, not by hashing: no new colour tokens (T1273's screenshot
   run fails new literals). A group's icon and its title share a colour, an item's icon takes its table colour.
4. **Layout.** An icon sits between the arrow and the title in a fixed 1.2em box, drawn only if the list has any
   icon (a popup of built-in `g` keys keeps the card A layout). Respect card A's key column.
5. **Preset defaults.** `crates/fastedit/presets/lazyvim.vimrc` gives each named group an `icon=` (search for
   find/search, git-branch for git, bug for debug, etc.), so the `<leader>` popup resembles LazyVim's.

## Scope

- `crates/fe-vim/src/which_key.rs` (the field; the lookup from card B's group records), `crates/fe-vim/src/vimrc.rs`
  (the `icon=` option), `crates/fe-config/src/settings.rs`, `settings.schema.json`, `settings-template.toml`
- `crates/fe-gpui/src/vim/which_key.rs`, `crates/fe-gpui/src/vim/which_key_icons.rs` (new: `command_icon`, the
  colour table), `crates/fe-gpui/src/vim/which_key_icons_tests.rs` (new, declared in `vim.rs` beside the others)
- `kiln/crates/kiln-ui/src/file_icons.rs` (new `Icon` variants), `kiln/crates/kiln-ui/assets/icons/*.svg` (new SVGs)
- `crates/fastedit/presets/lazyvim.vimrc`, `crates/fastedit/src/vim_lazyvim_groups_tests.rs`
- `crates/fe-gpui/examples/screenshots/scenes.rs` and `golden/which_key_icons.png`, `docs/design-language.md`
  (icon count and the new set line)

## Tests

- `vim::which_key_icons_tests::every_table_icon_is_a_bundled_icon` and `::every_table_command_is_a_catalog_id`
  (ids checked against the host's command catalog; no stale ids).
- `kiln-ui` existing `tests::` icon test that every `Icon` has a parsing SVG with a viewBox covers the new files;
  name it in the card's finish note once found (`rg 'viewBox' kiln/crates/kiln-ui/src/tests`).
- `which_key_groups::a_group_icon_is_carried` (fe-vim, card B's file): `icon=bug` reaches the entry.
- `vim::which_key_icons_tests::a_row_without_an_icon_keeps_its_title_aligned`, and
  `::icons_use_theme_colours_only`.
- `vim_lazyvim_groups_tests::every_named_group_has_an_icon`.
- Screenshot scene `which_key_icons` (leader popup of the lazyvim preset with its commands, vim on) compared with
  its golden; `cargo run -p fe-gpui --example screenshots` passes on macOS.


## Helium note

This card was lent to helium, which runs Linux. C5 (the macOS screenshots run with a committed golden) can't run or pass here, and review fails a criterion it can't see met. Before your first `rgt finish`, respec C5 with `rgt task respec --criteria` (it replaces the whole list, so keep C1-C4 as they are) to: "the which_key_icons scene is added to crates/fe-gpui/examples/screenshots and `cargo build -p fe-gpui --example screenshots` builds on Linux; the golden is recorded on the Mac when this lands". Don't commit a golden made on Linux. The Mac lead records it when landing the batch.

## Implementation notes

- **Group records.** `WhichKeyGroupName` (in `crates/fe-vim/src/remap.rs`, added to scope) carries
  `icon: Option<String>`; `WhichKeyGroupName::parse_with_icon` checks its shape (lowercase letters and digits
  joined by `-`), `parse` is unchanged. `:fegroup {keys} {name} icon={icon}` takes the icon from the line's last
  word only; `icon=` earlier is part of the name. A group's name and icon resolve separately, each the last given
  for the keys (`remap::group_icon`), as which-key.nvim merges specs: a user renaming a preset group keeps the
  preset's icon, and a later `icon` replaces it. As with names, a group's icon applies only where no map ends at
  the key.
- **Settings.** `[[vim.which_key_group]]` takes `icon` (schema pattern as above). A malformed one is a warning at
  `vim.which_key_group[N].icon` and the entry loads without it (`crates/fastedit/src/vim_remaps.rs`, added to
  scope). A well-formed name the bundled set lacks draws nothing (the row keeps its room); fe-vim and fastedit
  can't see kiln-ui's set to warn about it.
- **Items.** fe-gpui fills an item's icon from `which_key_icons::command_icon` as the list is built
  (`which_key_now`, `chord_which_key`), so keymap chords' commands get theirs too. The table is exact IDs
  (`COMMAND_ICONS`) then namespaces (`NAMESPACE_ICONS`: `git.*`, `lsp.*`, `terminal.*`, `tasks.*`, `chat.*`,
  `ai.*`). `editor.undo` and `editor.select_all` are left out on purpose, so the existing `which_key_popup`
  scene (which maps them) draws no icons and keeps its golden.
- **Colours.** `ICON_COLOURS` maps each icon to `information`, `warning`, `error`, `hint` or success; the
  editor theme has no `success` or `link`, so success is `change_added`, the theme's green. A group's title
  takes its icon's colour (information without one, as before).
- **Icons added** (Lucide 1.52.0, the set's pinned commit, unmodified): app-window, bot, bug, flask-conical,
  log-out, panels-top-left, play, redo, save, settings, terminal, text-wrap, toggle-right, undo (all under 1 KiB).
  `wrap-text` is `text-wrap` at that commit. `THIRD_PARTY.md` and `kiln/THIRD_PARTY.md` list them with their
  Feather classification; both rows were stale (the root's lacked `pin.svg`, kiln's was at 60) and now list all
  83. `docs/design-language.md` said 68; there were 69, now 83.
- **kiln-ui test** covering the new SVGs: `tests::assets::every_icon_is_bundled_and_parses` (and
  `::bundled_files_match_the_enum`).
- **Screenshots.** The `which_key_icons` scene loads the preset with `include_str!` and the workbench's command
  titles (`fe_workbench::command_list`). The harness now serves kiln-ui's assets
  (`VisualTestAppContext::with_asset_source`), without which `svg()` draws nothing; no earlier scene draws an
  `svg()`. Its golden, `golden/which_key_icons.png`, is to be recorded on the Mac.
