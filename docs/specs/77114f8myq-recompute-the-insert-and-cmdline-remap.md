---
task_uid: "01m5q2w3152e8yaq77114f8myq"
task: "T223"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "After SharedKeymap::update binds or unbinds an insert or cmdline remap's left side (or a prefix of it), VimRemaps::diagnostics() gains or loses the keymap-conflict warning without a reload: a test in vim_remaps_tests.rs fails at the base and passes"
    required: true
    manual: false
  - id: "C2"
    text: "Existing keymap and vim_remaps tests pass unchanged; cargo nextest run -p kiln-platform -p fastedit and clippy -D warnings are clean; no Cargo change"
    required: true
    manual: false
---

# Recompute the insert and cmdline remap keymap-conflict warnings when SharedKeymap::update swaps the keymap (T209 review F2 follow-up)

## Problem

Follow-up to T209 (review F2, medium). The insert/cmdline remap keymap-conflict warnings refresh only when `ConfigSource::reload` runs. A keymap swapped by `SharedKeymap::update` (extension bindings) does not call the sources or republish diagnostics, so a conflict an extension binding introduces or removes is not shown until the next reload.

## Change

Add a keymap-change observer to `SharedKeymap` (kiln-platform `keys.rs`) and use it to recompute and republish `ConfigDiagnostics` from `VimRemaps` (`crates/fastedit/src/vim_remaps.rs`), so the warnings follow an `update`. Test: an `update` that binds an insert remap's left side adds the warning, and one that removes the binding clears it, without a reload.
