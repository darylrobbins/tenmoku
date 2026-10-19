---
task_uid: "01m58zqf0jcfe7nz5zmjyzp43x"
task: "T219"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "config_source_reaches_diagnostics (top level, kiln/crates/kiln/tests/config_source.rs): a source passed to kiln::Config::source is installed, reloaded on a batch naming its path, and its diagnostics are in ConfigDiagnostics"
    required: true
    manual: false
  - id: "C2"
    text: "vim_remaps::tests::vimrc_and_table_merge: the vimrc's maps and the [[vim.remap]] entries both apply, the table wins for the same mode and from, and a missing vimrc is no problem"
    required: true
    manual: false
  - id: "C3"
    text: "vim_remaps::tests::problems_at_their_lines: each VimrcIssue is a Warning at the vimrc's line, and a [[vim.remap]] entry's key-notation problem is a Warning at that entry's line in settings.toml"
    required: true
    manual: false
  - id: "C4"
    text: "vim_remaps::tests::reload_keeps_last_good: editing the vimrc reloads the maps through a batch; an unreadable vimrc keeps the last good maps with one Error at line 1"
    required: true
    manual: false
  - id: "C5"
    text: "fe_config::paths::user_vimrc_path is \u003cconfig dir\u003e/vimrc, and main.rs no longer logs remap problems itself"
    required: true
    manual: false
  - id: "C6"
    text: "The F2 limit in the spec's Known limit section is documented in the module docs of crates/fastedit/src/vim_remaps.rs; problem positions for a project file that does not change between SettingsStore::load and VimRemaps::new are exact (the C3 test pins them)"
    required: true
    manual: false
---

# Load ~/.config/fastedit/vimrc as vim remaps, hot reloaded, with its problems and the [[vim.remap]] table's at their lines in the config problems list (Mac T875)

{
  "mode": "inline",
  "body": "## Problem\n\nThe vimrc parser card reads the vimrc subset from text. The settings card loads\n`[[vim.remap]]` but can only log its key-notation problems. `docs/decisions/vim-remaps.md`\n§1(b) and §7 (decided 2026-10-17) want more. `~/.config/fastedit/vimrc` should be read and\nhot reloaded. A file with problems should keep its good lines. Each problem should go to the\nconfig problems list (`crates/fe-workbench/src/config_problems.rs`, the status bar's\n\"N config problems\"), and confirming it should open the file at its line. This is the\nrecord's card 6, host half.\n\nThe list shows `kiln_platform::config::ConfigDiagnostics`, which kiln's config service fills\nfrom its sources (`ConfigSource`: `paths`, `reload`, `diagnostics`, in\n`kiln/crates/kiln-platform/src/config.rs`). The service already takes any source through\n`ConfigService::source`. But fastedit builds its config through `kiln::Config`\n(`kiln/crates/kiln/src/setup.rs`, used by `crates/fastedit/src/config.rs`), which has no way\nto add a source. T841 (extensions discovery) also edits `setup.rs`, so this card follows it.\n\n## Change\n\n- **kiln.** In `kiln/crates/kiln/src/setup.rs`, add `Config::source(self, source: impl\n  ConfigSource) -> Self`, passed to the `ConfigService` that `init` builds. Test it in the\n  new `kiln/crates/kiln/tests/config_source.rs`: a source's diagnostics reach\n  `ConfigDiagnostics`, and a batch naming its path calls its `reload`.\n- **fe-config.** `paths.rs` gains `user_vimrc_path() -> Option<PathBuf>`:\n  `<config dir>/vimrc`, next to `user_settings_path`.\n- **fastedit.** `crates/fastedit/Cargo.toml` gains the `fe-vim` and `toml` workspace\n  dependencies (`Cargo.lock` follows). New `crates/fastedit/src/vim_remaps.rs` (`mod\n  vim_remaps;` in `main.rs`) holds `VimRemaps: ConfigSource`. Its paths are the vimrc and the user's settings file.\n  - On install and on a batch naming either path, it reads the vimrc (a missing file is\n    empty, with no problem) with `fe_vim::parse_vimrc`. It takes `vim.remap` and\n    `vim.leader` from the settings in use.\n  - It builds one set: the vimrc's maps first, then the TOML entries, so for the same mode\n    and `from` the TOML wins. It applies the set through fe-gpui (`set_vim_remaps` takes\n    the merged records; change its signature as needed).\n  - Its `diagnostics()` hold a Warning for each `VimrcIssue`, at the vimrc's line, column 1.\n    They also hold one for each TOML `RemapIssue`, at that entry's `from`/`to` key line in\n    `settings.toml`, found with `toml::de::DeTable` spans as fe-config's `key_offset` finds\n    keys.\n  - `main.rs` passes `.source(VimRemaps::new(...))` where `config::config(...)` is used, and\n    drops the settings card's direct `set_vim_remaps` calls and logging (this source now owns\n    them).\n  - A vimrc that can't be read (permissions) keeps the last good maps, with one Error at\n    line 1.\n- `docs/decisions/vim-remaps.md` needs no change. `TESTING.md` needs no change.\n\nTests: `kiln/crates/kiln/tests/config_source.rs`, and unit tests in `vim_remaps.rs`\n(`#[cfg(test)] mod tests`) driving the source with explicit batches, as kiln's config tests\ndo (`watch_with(batch_channel())`).",
  "ref": "",
  "ref_blob": ""
}

## Known limit (lead decision, 2026-10-19; NOT part of this card)

Review round 3 kept one open finding (F2): `VimRemaps` rereads the settings files to place diagnostics instead of retaining the settings store's accepted text per layer. It matters only when a project settings file changes between `SettingsStore::load` and `VimRemaps::new` (or the first watcher batch rejects it): the project remap and leader warnings can then fall back to the user file and sit at the wrong line. The mapped behaviour itself (which maps apply, last-good on a bad reload) is unaffected. Retaining each layer's accepted text or spans in the store and exposing the snapshot to `VimRemaps` is a separate card; document the limit in the module docs of `crates/fastedit/src/vim_remaps.rs`. Review findings that only ask for that are out of scope for this card.

## Implementation notes

- `set_vim_remaps(cx, remaps: &[Remap], leader: &str) -> Vec<RemapIssue>` now takes the merged
  `fe_vim` records; the `vim.remap` to `Remap` conversion (`v` as `x`) moved to `vim_remaps.rs`.
  `crates/fe-gpui/src/vim/remap_tests.rs` was added to the scope to follow the new signature.
- `VimRemaps` records what it applied in a `RemapsInUse` global, and applies again only when the
  records or leader change, so an edit that changes no map doesn't replay held keys.
- A TOML entry's warning is at its `from` key when the left side alone can't load, else at `to`;
  its key is `vim.remap[N].from` or `.to`.
- The source's paths are the vimrc and every settings layer (`SettingsStore::paths`, the user's
  first). A layer's `vim.remap` array replaces an earlier one's, so a table problem is placed in
  the last layer that sets `vim.remap` (and a leader problem in the last that sets
  `vim.leader`); a batch naming any of them places the problems again (review F2). It places
  them in each layer's text as the settings store last took it: a file the store rejects (any
  Error of the store's for it) keeps its last good table, so the source keeps that file's last
  accepted text too, empty until it has loaded once (review round 2, F2).
- `kiln::init` returns and logs an app source's diagnostics as they are once it's installed,
  through a wrapper that records them in `install` (review round 2, F5).
- A `vim.leader` that can't be read is one warning at `vim.leader`; the entries naming
  `<leader>` are left out, each with a warning, and the rest of the set, the vimrc's included
  (its `<leader>`s already put in), loads (review F1).
- The vimrc and settings files are read as kiln reads config files: opened without blocking,
  checked to be regular files once open, at most 1 MiB. A vimrc that isn't a regular file, is
  too big or isn't UTF-8 is unreadable as a permissions error is (review F3).
- The vimrc is read again on install and on any batch naming one of the source's paths
  (review F4).
