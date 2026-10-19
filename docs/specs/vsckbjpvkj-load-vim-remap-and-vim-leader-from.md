---
task_uid: "01m58zqejrn57bhtvsckbjpvkj"
task: "T217"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "vim_remap_setting (top level, crates/fe-config/tests/vim_remap_setting.rs): [[vim.remap]] entries with mode as a string or a list load with recursive false by default, vim.leader defaults to \\, a mode outside n x v o is a located schema error, and the template still loads as all defaults"
    required: true
    manual: false
  - id: "C2"
    text: "vim::remap_setting_tests::remaps_reach_every_editor: set_vim_remaps gives the set to open editors and to an editor made or switched on later, and a later call replaces it"
    required: true
    manual: false
  - id: "C3"
    text: "vim::remap_setting_tests::bad_entries_are_reported: an entry with an unknown \u003cname\u003e comes back as 'vim.remap[N]: …' and the other entries still apply"
    required: true
    manual: false
  - id: "C4"
    text: "settings.schema.json is regenerated (the golden test passes) and settings-template.toml shows a commented [[vim.remap]] example; main.rs applies remaps at startup and on a settings reload that changed vim.remap or vim.leader"
    required: true
    manual: false
---

# Load [[vim.remap]] and vim.leader from settings.toml into every editor's vim state, live on reload (Mac T873)

{
  "mode": "inline",
  "body": "## Problem\n\nThe user's first answer (`docs/decisions/vim-remaps.md`, decided 2026-10-17) puts the TOML\ntable first, then the vimrc subset. The record's card 2 covers the table and its reload.\nVim's settings are fe-config's `VimSettings` (`crates/fe-config/src/settings.rs`), the\n`[vim]` table of `settings.toml`. That type is checked against its schema\n(`settings.schema.json`) and the template test (`tests/template.rs`).\n`crates/fastedit/src/main.rs` applies each one on startup and in `follow_settings`, through\nfe-gpui setters such as `set_vim_jump_on_s` (`crates/fe-gpui/src/vim.rs`). Those setters\nstore the value on the `Vim` global and on every editor's `VimState`, and `sync_vim` applies\nit to a state made later.\n\n## Change\n\n- **fe-config.** `VimSettings` gains two fields:\n  - `leader: String`, default `\"\\\\\"`, documented as vim notation (`\"<Space>\"` works).\n  - `remap: Vec<VimRemap>`, default empty.\n\n  `VimRemap { mode: VimRemapModes, from: String, to: String, recursive: bool }`, with\n  `#[serde(default)]` on `recursive` (false). `mode` is one of `\"n\" \"x\" \"v\" \"o\"` or a list\n  of them, as a schema enum, so a bad mode is a located schema error. `v` means `x`. `i`\n  and `c` aren't in the enum yet. Each field has a doc comment, which becomes the schema\n  description. Regenerate `crates/fe-config/settings.schema.json`, and add a commented\n  example to `crates/fe-config/settings-template.toml`:\n\n  ```toml\n  [[vim.remap]]\n  mode = \"n\"\n  from = \"<leader>w\"\n  to = \":w<CR>\"\n  ```\n- **fe-gpui.** `pub fn set_vim_remaps(cx: &mut App, settings: &VimSettings) -> Vec<String>`\n  builds `RemapSet::new(&remaps, &settings.leader)`. It keeps the set as an `Arc` on the\n  `Vim` global, gives it to every editor's state (`VimState::set_remaps`), and returns each\n  `RemapIssue` as `vim.remap[{index}]: {message}`. `sync_vim` gives the set to a state it\n  makes, as it does `jump_on_s`. Export it from `crates/fe-gpui/src/lib.rs`'s\n  `pub use vim::{…}` (the only lib.rs change; T816 and T858 hold lib.rs, hence the\n  dependencies).\n- **fastedit.** `main.rs` calls `set_vim_remaps` at startup, next to `set_vim_jump_on_s`,\n  and in `follow_settings` when `vim.remap` or `vim.leader` changed. It logs each problem\n  as a warning under the `fastedit` target. Showing these in the config problems list, at\n  their line, is the vimrc file card's job, which owns a config source for both files.\n  A settings file that fails to parse keeps the last good set, as every setting does.\n\nTests: the new `crates/fe-config/tests/vim_remap_setting.rs`, and the new\n`crates/fe-gpui/src/vim/remap_setting_tests.rs`, declared in `vim.rs` next to\n`jump_setting_tests` (`#[cfg(test)] mod remap_setting_tests;`).",
  "ref": "",
  "ref_blob": ""
}
