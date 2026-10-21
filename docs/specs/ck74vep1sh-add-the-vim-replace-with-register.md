---
task_uid: "01m55rk09qv93xabck74vep1sh"
task: "T164"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test vim_replace_with_register_setting (top level, in crates/fe-config/tests/vim_replace_with_register_setting.rs) passes: vim.replace_with_register defaults to false, true round-trips from a user file and from a project file, and the schema documents it; fe-config's schema and template tests pass with the regenerated crates/fe-config/settings.schema.json and settings-template.toml."
    required: true
    manual: false
  - id: "C2"
    text: "The integration test replace_with_register_gives_gr_to_vim (top level, in crates/fe-keymap/tests/vim_replace_with_register_keys.rs) passes: on macOS and Linux, with vim.replace_with_register in the editor's context, g r r, g r n, g r a, g r i and g r t match no binding in vim normal mode and g r a none in vim visual mode, while g d, g shift-d, ctrl-], g shift-o and shift-k resolve as T900 binds them; T900's vim_ide_keys tests pass unchanged."
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test vim::jump_setting_tests::replace_register_setting_tests::the_setting_gives_gr_to_vim (new crates/fe-gpui/src/vim/replace_register_setting_tests.rs, declared in crates/fe-gpui/src/vim/jump_setting_tests.rs) passes: in a real EditorView with vim on, after set_vim_replace_with_register(true) the keystrokes y i w, j, g r r replace the second row with the yanked word and the unnamed register still holds it; an editor opened after the call has it on; after set_vim_replace_with_register(false), g r w changes nothing."
    required: true
    manual: false
---

# Add the vim.replace_with_register setting (default false), applied live, which gives gr, grr and visual gr to ReplaceWithRegister instead of nvim 0.12's language-server gr keys (Mac T911)

Filed on argon from Mac T911 (card ~/coordination/argon-cards/zzq/T911.json rev 1; note zzq, Vim parity after the user's decisions). Mac task uid 01m55rh3b2mrd0xr5z58e0qg5h. Base: argon trunk 7edfe4f (= mac/main c7b6a04 synced). Corpus cases are recorded with nvim 0.12.x; unit tests are named by full module path, integration tests by bare fn name. Argon deps: T163 (Mac T910), T162 (Mac T901). Held in the inbox: waits until Mac T896, Mac T884, Mac T889, Mac T876 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T911)".

## Problem

The previous card gives fe-vim ReplaceWithRegister's `gr{motion}`, `grr` and visual `gr` behind
`VimState::set_replace_with_register`, and a `vim.replace_with_register` context flag while it is on. Nothing
sets it, and T900's keymap rows still take `g r r`, `g r n`, `g r a`, `g r i` and `g r t` for the language
server, so `grr` and visual `gr` would never reach vim. The user decided on 2026-10-17 that an opt-in setting
`vim.replace_with_register` (default false) gives `gr` to ReplaceWithRegister instead of T900's keys, as
VSCodeVim's `vim.replaceWithRegister` does.

## Change

- **fe-config.** `VimSettings` (`crates/fe-config/src/settings.rs`) gains `replace_with_register: bool`,
  default false, documented for the schema as `jump_on_s` is: "true: `gr{motion}`, `grr` and visual `gr`
  replace text with a register's contents, as the ReplaceWithRegister plugin does, instead of running nvim
  0.12's language-server `gr` keys (`grr` references, `grn` rename, `gra` code action, `gri`, `grt`)."
  Regenerate `crates/fe-config/settings.schema.json` and add the commented default to
  `crates/fe-config/settings-template.toml` beside `# jump_on_s = false`. A project file may set it (it
  changes keys, not what runs).
- **fe-gpui.** `pub fn set_vim_replace_with_register(cx: &mut App, on: bool)` in `crates/fe-gpui/src/vim.rs`,
  as `set_vim_jump_on_s` is: kept on the `Vim` global, set on every editor's `VimState`, applied by
  `sync_vim` to a state made later; exported from `crates/fe-gpui/src/lib.rs`'s `pub use vim::{…}`.
- **fastedit.** `crates/fastedit/src/main.rs` calls it at startup and in `follow_settings`, next to
  `set_vim_jump_on_s`.
- **Keymap.** In `crates/fe-keymap/keymaps/fastedit.toml`, T900's `g r r`, `g r n`, `g r a`, `g r i` and
  `g r t` rows (normal and, for `g r a`, visual) gain `&& !vim.replace_with_register` in their context, so
  with the setting on every `g r` key reaches vim. `g d`, `g shift-d`, `ctrl-]`, `g shift-o` and `shift-k`
  keep their bindings either way.

Tests: a new `crates/fe-config/tests/vim_replace_with_register_setting.rs`; a new
`crates/fe-keymap/tests/vim_replace_with_register_keys.rs` modelled on T900's `tests/vim_ide_keys.rs`; and a
new `crates/fe-gpui/src/vim/replace_register_setting_tests.rs`, declared in the existing
`crates/fe-gpui/src/vim/jump_setting_tests.rs` by `#[path = "replace_register_setting_tests.rs"] mod
replace_register_setting_tests;`, in a real EditorView.

Order: settings.rs, main.rs, lib.rs, jump_setting_tests.rs and fe-gpui's vim.rs are changed by T894 (via
T896, which depends on it), T884, T889 and the remaps chain (T876); fastedit.toml by T900 and T901. This
card depends on the fe-vim card, T901 (after T900), T896, T884, T889 and T876. T410, T842, T848, T855 and
T820 also touch main.rs, settings.rs or fastedit.toml for unrelated work; the scheduler keeps them apart.
