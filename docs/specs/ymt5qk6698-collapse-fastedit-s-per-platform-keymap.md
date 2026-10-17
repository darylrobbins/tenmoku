---
task_uid: "01m53dbt8f85qn71ymt5qk6698"
task: "T120"
rev: 1
type: "refactor"
complexity: "s"
criteria:
  - id: "C1"
    text: "The four per-platform keymap and menu files are deleted; crates/fe-keymap/build.rs generates DEFAULT_MACOS, DEFAULT_LINUX, DEFAULT_MACOS_MENUS and DEFAULT_LINUX_MENUS from keymaps/fastedit.toml and menus/fastedit.toml, and tests/one_keymap.rs, unedited, passes on macOS and Linux"
    required: true
    manual: false
  - id: "C2"
    text: "No binding or menu item is lost: the diff to keymaps/fastedit.toml and menus/fastedit.toml is comment lines only, and the decision file shows each platform's resolved bindings and menu bar (in one_keymap.rs's form) from base's deleted copies equal to head's generated text on macOS, Linux and Windows"
    required: true
    manual: false
  - id: "C3"
    text: "crates/fastedit/tests/shipped_config.rs: kiln::testing::assert_clean_config passes on config::shipped (the Config main.rs starts from, without the user's files) after config::register_commands, on macOS and Linux"
    required: true
    manual: false
  - id: "C4"
    text: "Every existing test passes unedited on macOS and Linux, including the ~40 that parse fe_keymap::DEFAULT_MACOS or DEFAULT_LINUX"
    required: true
    manual: false
---

# Collapse fastedit's per-platform keymap and menu copies into one table checked by kiln::testing on every platform (Mac T707)

## Problem

Phase 3 of the Kiln app layer the owner approved on 2026-10-13 (design record `docs/decisions/kiln-app-layer.md`).
fastedit already ships one keymap and one menu table: `crates/fe-keymap/keymaps/fastedit.toml` and
`crates/fe-keymap/menus/fastedit.toml`, with `keys = { macos = .., default = .. }`, `secondary` and `platform`
filters, loaded by `config()` in `crates/fastedit/src/main.rs` (`.keymap(DEFAULT_KEYMAP_PATH, DEFAULT)`,
`.menus(DEFAULT_MENUS_PATH, MENUS)`). But four hand-kept per-platform copies sit beside them:

- `crates/fe-keymap/keymaps/default-macos.toml` (1090 lines) and `default-linux.toml` (1134), behind
  `fe_keymap::DEFAULT_MACOS` / `DEFAULT_LINUX`, which about 40 tests in fe-keymap, fe-gpui and fe-workbench parse
  to get one platform's bindings on any host (`KeymapLayer::parse(.., DEFAULT_MACOS, None)`,
  `load_keymap(DEFAULT_MACOS, None)`, `KeymapStore::load(DEFAULT_MACOS, ..)`);
- `crates/fe-keymap/menus/default-macos.toml` (366) and `default-linux.toml` (333), behind
  `DEFAULT_MACOS_MENUS` / `DEFAULT_LINUX_MENUS`, read only by `crates/fe-keymap/tests/one_keymap.rs`.

`one_keymap.rs` checks the one table resolves to exactly the copies, so every binding change is made three times:
Mac T480 (landed on argon as 43dad95b) added the same 28 keymap lines and 19 menu lines to each of the three
files, and T434, T523 and T557 each carry all six files in their scope for the same reason. And nothing checks the
`Config` fastedit actually ships with `kiln::testing::assert_clean_config` (T674): `crates/fastedit/tests/menus.rs`
checks the menus on macOS only.

## Change

No command ID or key context is renamed; every existing test stays unedited; fastedit's keymap stays a TOML
table (no `kiln::keymap!`/`kiln::menus!`); nothing on the paint path changes.

1. **Generate the copies at build time.** Add `crates/fe-keymap/build.rs` (with `toml_edit` as a
   `[build-dependencies]` entry in `crates/fe-keymap/Cargo.toml`) that reads `keymaps/fastedit.toml` and
   `menus/fastedit.toml` and writes, into `OUT_DIR`, `keymap-macos.toml`, `keymap-linux.toml`, `menus-macos.toml`
   and `menus-linux.toml`: each table resolved for that OS at the TOML level, with the rules
   `KeymapLayer::parse_for` documents (`kiln-core/src/keymap.rs`):
   - a `[[bindings]]`, `[[menus]]` or `[[menus.items]]` entry whose `platform` (`"macos"`, `"!macos"`, a list
     such as `"linux, windows"`) excludes the OS is dropped, and `platform` is removed from those kept;
   - `keys = { macos = .., default = .. }` becomes the OS's string (its `default` for an OS not named; an entry
     with neither is dropped);
   - the `secondary` modifier in a keystroke becomes `cmd` on macOS and `ctrl` on Linux.

   Anything else in an entry is copied as it is. An unknown platform name or a malformed `keys` table panics the
   build with the file and entry. Emit `cargo:rerun-if-changed` for both tables and `build.rs`.
2. In `crates/fe-keymap/src/lib.rs`, the four constants keep their names and `&str` type and become
   `include_str!(concat!(env!("OUT_DIR"), "/keymap-macos.toml"))` and so on; their docs say they're generated
   from the one table. Delete the four per-platform files.
3. Update the header comments of `keymaps/fastedit.toml` and `menus/fastedit.toml` (comment lines only: no
   binding, item or blank-line structure changes) to say fe-keymap's build generates each platform's text from
   them, and `TESTING.md` (its two `default-linux.toml` mentions, ~lines 335 and 349) to name the one table as
   resolved on Linux. Leave `docs/decisions/linux-menu-bar.md` as the record it is.
4. **Check the shipped config on every platform.** Move `config()` and `log_keymap_report()` out of
   `crates/fastedit/src/main.rs` into a new `crates/fastedit/src/config.rs`, split as:
   - `pub fn register_commands(cx: &mut App)`: today's `.commands(..)` body
     (`fe_gpui::init_with_keymap(cx, Keymap::default())`, `fe_workbench::init(cx)`);
   - `pub fn shipped(settings: SettingsStore) -> Config`: everything the app owns (log target,
     `.commands(register_commands)`, catalog, keymap, reload hook, menus, menu shortcut context, settings store,
     theme mode and extensions);
   - `pub fn config(settings: SettingsStore) -> Config`: `shipped(..)` plus the user's theme and keymap paths,
     exactly as today; `main.rs` calls it as before.

   Add `kiln = { workspace = true, features = ["test-support"] }` to `crates/fastedit/Cargo.toml`'s
   `[dev-dependencies]`, and a new `crates/fastedit/tests/shipped_config.rs` that includes `src/theme.rs` and
   `src/config.rs` by `#[path]` (as `kiln/crates/kiln/tests/host.rs` includes the host's files; `#[allow(dead_code)]`
   on the modules), and in a `#[gpui::test]` calls `config::register_commands(cx)` then
   `kiln::testing::assert_clean_config(&config::shipped(SettingsStore::load(None, None)), cx)`. It must not read
   the developer's own `keymap.toml` or theme, which is why it checks `shipped`, not `config`.

If `assert_clean_config` reports a diagnostic on today's tables, stop and report it rather than editing a binding:
the table must stay as it is in this card.

## Order

Depends on Mac T480 and T398 (in host stack 10: T480 edits all six TOMLs, T398 edits `main.rs`) and T434 (in flight
on neon, edits all six TOMLs). T523 and T557 (inbox) list the four copies in their scope: once this lands they only
need `fastedit.toml` and the menu table. T435 and T511 do not overlap (no TOML, and this card does not touch
`workbench.rs`). T681's typed tables are not used: fastedit's tables stay TOML.
