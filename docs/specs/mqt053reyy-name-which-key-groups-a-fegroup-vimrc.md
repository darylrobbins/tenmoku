---
task_uid: "01m6rjmmwasgqg4qmqt053reyy"
task: "T1395"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "a_named_prefix_titles_its_group, an_unnamed_group_keeps_its_count, names_are_per_mode, a_later_name_wins and a_malformed_fegroup_line_is_an_issue pass"
    required: true
    manual: false
  - id: "C2"
    text: "group_table_loads_and_validates (crates/fe-config/tests/vim_which_key_group_setting.rs) passes"
    required: true
    manual: false
  - id: "C3"
    text: "vim_lazyvim_groups_tests::the_preset_names_every_group_it_has_maps_in passes"
    required: true
    manual: false
  - id: "C4"
    text: "vim::which_key_tests::a_named_group_shows_its_name passes: +file/find appears and +2 does not"
    required: true
    manual: false
  - id: "C5"
    text: "vim_lazyvim_preset_tests table test passes with \u003cleader\u003eE still mapped to dock.show_project"
    required: true
    manual: false
---

# Name which-key groups: a :fegroup vimrc command and [[vim.which_key_group]] setting and the lazyvim preset's group names

## Problem

The user (2026-11-06): "It supports naming sections." LazyVim's popup shows a group as `+buffer`, `+code`,
`+file/find`, `+git`, `+quit/session`, `+windows`. Ours shows `+2`, `+6`, `+5`
(`WhichKeyLabel::Group(usize)`, `crates/fe-vim/src/which_key.rs`; `title()` in
`crates/fe-gpui/src/vim/which_key.rs:316` prints `+{count}`). Nothing in a vimrc, `[[vim.remap]]` or the
`lazyvim` preset can name a prefix.

Also in the same popup `e` and `E` both read "Show Project". That is not a label bug: the preset maps
`<leader>e` and `<leader>E` to the same command (`crates/fastedit/presets/lazyvim.vimrc:25-26`;
`TABLE` in `crates/fastedit/src/vim_lazyvim_preset_tests.rs`). LazyVim's `e` is the explorer at the root dir and
`E` at the cwd; Tenmoku has one project root, so `E` has no distinct meaning.

## Change

1. **A name for a prefix.** New vimrc command `:fegroup {keys} {name}` (case-insensitive, like the other Ex
   commands the vimrc reads; `{name}` is the rest of the line), plus a settings table
   `[[vim.which_key_group]] keys = "<leader>f"; name = "file/find"; mode = "n"|"x"|list` (default `"n"`,
   same mode forms as `[[vim.remap]]`). Both produce `WhichKeyGroupName { mode, keys: Vec<VimKey>, name }`
   records loaded beneath the vimrc and `[[vim.remap]]` in the same layering the preset uses (later wins).
   The vimrc parser reports a malformed line as an issue like its other commands.
2. **Carry it to the popup.** `WhichKeyLabel::Group` gains the name: `Group { count: usize, name: Option<String> }`.
   `VimState::which_key_for` fills it for a held map prefix by looking the keys-so-far plus the entry's key up in
   the group names for the current mode. A named group's title is `+{name}`; unnamed stays `+{count}`.
   Keymap chords (`g`, `]`, `ctrl-w`) are out of scope here: they keep `+{count}`.
3. **Preset names.** `crates/fastedit/presets/lazyvim.vimrc` names LazyVim's groups that the preset has maps for:
   `<leader>b` buffer, `<leader>c` code, `<leader>f` file/find, `<leader>g` git, `<leader>gh` hunks,
   `<leader>q` quit/session, `<leader>s` search, `<leader>u` ui, `<leader>w` windows, `<leader>x`
   diagnostics/quickfix. Only prefixes that have a map.
4. **`<leader>E`.** Keep `<leader>E` mapped (LazyVim users reach for it); it shows the same "Show Project" label as `e`, since there is one project root. Do not remove it from the preset or `TABLE`.
   See the question below; if the user wants to keep it, label it instead, which this card does not build.
5. Settings schema and template (`settings.schema.json`, `settings-template.toml`) document the new table.

## Scope

- `crates/fe-vim/src/vimrc.rs`, `crates/fe-vim/src/lib.rs`, `crates/fe-vim/src/which_key.rs`, `crates/fe-vim/src/remap.rs`
  (the record and its lookup), `crates/fe-vim/src/state.rs` only if the group names are stored there
- `crates/fe-gpui/src/vim/which_key.rs` (`title()` and the `Group` arms; keep changes there small, card A owns
  its layout)
- `crates/fe-config/src/settings.rs`, `crates/fe-config/settings.schema.json`, `crates/fe-config/settings-template.toml`
- `crates/fastedit/src/vim_remaps.rs` (feed the groups from the preset and settings to `set_vim_remaps` or a
  sibling), `crates/fastedit/presets/lazyvim.vimrc`, `crates/fastedit/src/vim_lazyvim_preset_tests.rs`
- new tests: `crates/fe-vim/tests/which_key_groups.rs`, `crates/fe-config/tests/vim_which_key_group_setting.rs`,
  `crates/fastedit/src/vim_lazyvim_groups_tests.rs` (declared beside `vim_lazyvim_preset_tests`)

## Tests

- `which_key_groups::a_named_prefix_titles_its_group`: `:fegroup <leader>f file/find` with `<leader>ff` mapped:
  the `f` entry is `Group { count: 1, name: Some("file/find") }`.
- `which_key_groups::an_unnamed_group_keeps_its_count`, `::names_are_per_mode` (n vs x), `::a_later_name_wins`,
  `::a_malformed_fegroup_line_is_an_issue` (no name; no keys).
- `vim_which_key_group_setting::group_table_loads_and_validates` (modes as string and list, bad keys rejected).
- `vim_lazyvim_groups_tests::the_preset_names_every_group_it_has_maps_in`: every prefix of two or more keys
  that the preset maps beneath `<leader>` has a name, and no name is for a prefix with no map.
- `vim_lazyvim_preset_tests` existing table test unchanged for `<leader>E`.
- `vim::which_key_tests::a_named_group_shows_its_name` in an existing file's new test (fe-gpui): `+file/find`
  appears, `+2` does not.
