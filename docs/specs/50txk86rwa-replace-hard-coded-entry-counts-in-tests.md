---
task_uid: "01m5aq8cq8a7hdr050txk86rwa"
task: "T1009"
rev: 3
type: "chore"
complexity: "m"
criteria:
  - id: "C1"
    text: "No test in the named files asserts an exact entry count of a growing catalog (bindings, FEATURES, Icon::ALL, EditorColors::KEYS, theme captures, template key lists, per-OS binding counts); rg for the old literals returns none."
    required: true
    manual: false
  - id: "C2"
    text: "Each replaced assert still fails on a real defect: for each of bindings, features, icons, theme keys and template keys, a scratch duplicate entry and a scratch missing entry make the test fail (record in the hand-in; do not commit the defect)."
    required: true
    manual: false
  - id: "C3"
    text: "Adding one binding, one Icon, one Feature and one editor setting at the base needs no test edit beyond its own new row (shown by the same scratch additions passing)."
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-gpui -p fe-vim -p fe-config -p kiln-ui -p kiln -p kiln-platform passes; no Linux-only or macOS-only step is required (Linux-OK)."
    required: true
    manual: false
---

# Replace hard-coded entry counts in tests with invariants so cards stop conflicting

## Problem
Parallel cards on different hosts collide on hard-coded counts that nearly every card bumps, so every host sync and Mac stack re-resolves the same conflicts by hand (argon lead, 2026-10-19). Replace each exact-count assert with an invariant that does not change when an entry is added, keeping the test's intent. Linux-OK: no criterion needs macOS (kiln tests run headless under gpui's test context).

## Asserts to fix (found with rg; recheck at the base, fix any other exact-count of the same kind in the named files)
- crates/fe-gpui/src/command_tests.rs:62 `bindings.len() == 280` (and the comment tallying 75/149/24/5/5/2): assert no duplicate (keys, context) pairs, every binding's command is registered/known, and each group the later asserts in that test check is non-empty; drop the tally.
- crates/fe-gpui/src/keymap_tests.rs:250 `bindings.len() == 280` and `filter(is_window_key).count() == 32`: the zip with OLD_TABLE already checks row by row, so assert bindings.len() >= OLD_TABLE.len() with the zip covering every OLD_TABLE row (no silent truncation), no duplicates, window keys all parse. Do not require equality to a literal.
- crates/fe-vim/src/feature.rs:164 `FEATURES: [&Feature; 20]`: make it `&[&Feature]` (or derive N) so adding a feature does not edit the type. Add or keep a test that features are unique (distinct names) and each has its registered commands/hooks reachable; the feature_stubs test must still break for a stub.
- kiln/crates/kiln-ui/src/tests/assets.rs:11 `Icon::ALL.len() == 56`: assert Icon::ALL has no duplicate names/paths and every icon is bundled and parses (already below it); the line 29-33 bundled-set comparison keeps catching a missing or orphan SVG.
- crates/fe-config/tests/template.rs: the editor and terminal key lists spelled out as literal arrays, which break whenever a setting is added. Derive the key list from Settings::default() serialized (plus the known unset-by-default keys), and assert each is present as a commented default in USER_SETTINGS_TEMPLATE with the default value (the loop already does); also assert the template has no commented key that is not a setting. Keep schema_matches_golden in crates/fe-config/src/tests/settings.rs as is (it already compares generated schema to the checked-in file); touch that file only if it has an exact count.
- crates/fe-config/tests/indent_guide_colors.rs:21 and tests/editor_palette.rs:56 `EditorColors::KEYS.len() == 27`; tests/editor_palette.rs:64 and tests/ember_theme.rs:39 syntax capture count `== 25`: assert KEYS has no duplicates and every bundled theme defines exactly the keys in KEYS (the sorted-equality assert at editor_palette.rs:~52 is the model); assert the three bundled themes' capture sets are equal to each other and to the code's capture list rather than a literal.
- Per-OS keymap binding counts in kiln/crates/kiln/tests/gallery.rs:~76, host.rs:~128, default_keys.rs:~402 (`let bindings = if os == MacOs { 7 } else { 8 }` and similar): replace with: the Linux/Windows keymap is the macOS keymap plus exactly the F10-style window keys (compare as key sets, assert the difference is that named set), no diagnostics, no duplicate bindings. Same for kiln/crates/kiln/tests/setup.rs:125/157/161 and typed_tables.rs:125 only if they are counts of a default catalog; leave counts of a test's own hand-built keymap (1, 2, 3) alone.
- Fixed array types `[T; N]` on const tables that cards extend: kiln/crates/kiln-platform/src/menus.rs:170 NATIVE_KEYS, crates/fe-lsp/src/completion.rs:345 NAMES, kiln/crates/kiln/examples/gallery/pages/mod.rs:45 Page::ALL: make each a slice type (or derive N) when it is a list cards append to. Leave crates/fe-terminal-view ansi [Hsla;16] and fe-lsp semantic_tokens legend arrays (protocol-fixed or paired with a legend) alone unless a count assert there is exact.

## Scope
The files above plus every test file touched. Do not change product behaviour.

## Conflicts
Checked with rgt task conflicts: T925 (fe-lsp completion; may touch crates/fe-lsp/src/completion.rs NAMES) overlaps only if the NAMES change is made; drop that item if T925 is still running. T1006 (crates/fe-gpui/src/vim/incsearch_tests.rs) does not overlap.

## Implementation notes
- fe-vim: `FEATURES` is `&[&Feature]`. Features have no name, so the new `tests/feature_list.rs` reads the source: every file under `src/` that defines `pub(crate) const FEATURE: Feature` must be listed in `FEATURES` exactly once (by module name), and the list read must be as long as `FEATURES`.
- kiln: the per-OS binding counts in `gallery.rs`, `host.rs` and `default_keys.rs` share `tests/support/platform_keymaps.rs`. Off macOS, the keymap must equal macOS's binding for binding (keys, command, context, args and layer), with macOS's `cmd` read as `ctrl` (`secondary`). The only exceptions are F10's `menu_bar.open`, present off macOS only, and the `{ macos, default }` substitutions each caller names (kiln's `KILN`, the gallery's own). No keys may be bound twice in one context of a layer. Scratch: kiln's F10 changed to F9 fails `host` and `default_keys`. The menu counts (3, 4, 2) are left alone: they count an example's own menus, not a catalog that cards extend.
- `crates/fe-lsp/src/completion.rs` `NAMES` is left alone. It is fixed by the LSP protocol (`CompletionItemKind` 1 to 25), not a list that cards add to, and T925 touches that file.
- `Page::ALL` is a slice now, so `examples/gallery-screenshots/main.rs` iterates it with `.iter().copied()`.
- Themes: `editor_palette` checks the captures of all three bundled themes (dark, light, ember) against `fe_syntax::Highlight::all()` minus an explicit `UNCOLOURED` list, the highlights left to a dotted parent's colour or to the foreground. Scratch: dropping `label` from all three themes fails `bundled_palettes`.
- Scratch runs (not committed), over the 14 tests touched:
  - Duplicates all fail their check:
    - a second `left` row in `keymaps/fastedit.toml` fails `keymap_tests::default_keymap_matches_the_old_table` and `command_tests::table_is_the_only_binding_source`;
    - `&increment::FEATURE` listed twice fails `feature_list`;
    - a second variant named `file-music` fails `every_icon_is_bundled_and_parses`;
    - a repeated `[editor]` key or `[syntax]` capture in the light theme fails `editor_palette` and `indent_guide_colors`;
    - a repeated `# tab_size` line fails `template_is_all_defaults`;
    - a second `up` row in kiln's `keymap.toml` fails `host` and `default_keys`.
  - Missing entries all fail their check:
    - dropping the `left` row fails `default_keymap_matches_the_old_table`;
    - dropping `&surround::FEATURE` fails `feature_list`;
    - an orphan SVG fails `bundled_files_match_the_enum`;
    - dropping a light `[editor]` key or capture fails `editor_palette`;
    - dropping `# tab_size` fails `template_is_all_defaults`;
    - dropping kiln's F10 row fails `host` and `default_keys`.
  - Additions all pass with no test edit (once the settings schema golden is regenerated with `UPDATE_GOLDEN=1`):
    - a `workbench` binding at the end of `keymaps/fastedit.toml`;
    - a new feature module and its row;
    - a new `Icon` and its SVG;
    - a new editor setting and its template line;
    - a new kiln binding.
