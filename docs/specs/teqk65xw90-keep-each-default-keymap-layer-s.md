---
task_uid: "01m5nytzqr8zban6teqk65xw90"
task: "T306"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Reset Keybinding on a command whose kiln or extension default is unbound by another platform's user entry restores the default correctly and is no longer disabled; tests cover this and fastedit's own defaults unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "Headless, Linux-OK"
    required: true
    manual: false
---

# Keep each default keymap layer's platform-aware source so Reset Keybinding works for kiln and extension defaults (follow-up to Mac T1100)

## Problem
T299 (Mac T1100) narrowed C4 at review round 3: kiln's KeymapStore keeps non-app default layers resolved for this OS only, so Reset Keybinding cannot recognise another platform's unbind of a kiln or extension default; the tab disables Reset ('Edit keymap.toml') in that case.

## Change
kiln (kiln/crates/kiln/src/contributions.rs, kiln/crates/kiln-platform/src/config.rs) keeps each default layer's platform-aware source, or per-OS layers; the tab passes them to write_keymap_on so Reset works for every default layer.

## Tests
Reset of a command whose kiln or extension default is unbound by another platform's user entry restores correctly and the disabled state is removed. Headless, Linux-OK.

## Implementation notes

- kiln-platform's `SharedKeymap` keeps every platform's default layers beside the store
  (`set_platform_defaults` / `platform_defaults`). `platform_defaults` is `None` once the
  store's defaults no longer equal the ones kept for this platform, so a change made without
  them reads as missing, not stale.
- kiln resolves the keymap layers (bundled, contributed, the app's, extensions') for each
  `Os::ALL` and keeps them, both when `init` hands the store over (`contributions::start`)
  and after each re-resolve (`contributions::resolve`). setup.rs isn't touched: `start`
  computes the other platforms from the same config and catalog.
- The tab writes each non-fastedit default layer as one text that holds every platform's
  entries, each one's keys gated on its platform (`keys = { linux = "…" }`), so
  `fe_keymap::write_keymap_on` reads it on any platform as kiln did there. fastedit's own
  layer is still passed as shipped.
- Reset stays refused (fail-closed, 'Edit keymap.toml') only for a store that has no
  platform defaults kept, such as one built without kiln's setup.
- Review round 1 (F1): an extension file can parse on one platform and fail on another
  (`secondary-ctrl-k` holds ctrl twice off macOS), and resolving the other platforms
  overwrote the extensions' shared last-good text. `extensions.rs` now keys last good by
  `(Os, path)`, so each platform keeps its own; scope widened to it and to the new
  `kiln/crates/kiln/tests/extension_platform_keys.rs`, which reloads a real extension
  through that case. `reset_restores_an_extension_default_kept_on_another_platform_only`
  covers Reset of an unbind of keys that are the extension's only on the other platform.
- Review round 1, second pass (F2): the tab built its defaults from this platform's layer
  list, so a layer kept only on another platform (an extension file with an error here and
  no last good version) was dropped. `default_keymaps` now merges every platform's layer
  lists into one order, each layer known by its file and how many of that file's lie under
  it. Covered by `reset_restores_an_extension_default_in_a_layer_on_another_platform_only`
  and kiln's `a_platform_where_the_extension_keymap_never_loaded_has_none`.
