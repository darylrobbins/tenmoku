---
task_uid: "01m6j07wgvq8v7vvcqfbhm8nrg"
task: "T432"
rev: 2
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "publish_replaces_a_set_and_clear_removes_it: publishing a set replaces that set alone on the buffer and clear removes it (crates/fe-plugin/tests/diagnostics.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "diagnostics_need_the_decorations_capability: without decorations both imports are denied and nothing is stored (crates/fe-plugin/tests/diagnostics.rs)"
    required: true
    manual: false
  - id: "C3"
    text: "a_bad_range_or_too_many_changes_nothing: a range off a char boundary or past the text, or over 10000 on one buffer, is invalid-argument and an unknown version is conflict, the stored sets unchanged (crates/fe-plugin/tests/diagnostics.rs)"
    required: true
    manual: false
  - id: "C4"
    text: "unloading_or_closing_drops_the_plugins_diagnostics: DiagnosticStore unload and close remove its sets and notify (crates/fe-plugin/tests/diagnostics.rs)"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-plugin -p tenmoku-plugin and cargo clippy -p fe-plugin -p tenmoku-plugin --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Add the fastedit:editor diagnostics import: a plugin publishes and clears named sets of diagnostics on a buffer at a version, under the decorations capability (Mac T1353)

## Problem

`kiln:extension/types` defines `diagnostic` (range, severity, message, source, code), but no import
lets a plugin report one, so a linter plugin has nowhere to put its findings.

## Change

- `crates/fe-plugin/wit/diagnostics.wit`, interface `diagnostics` in `fastedit:editor@1.0.0`
  (imported by `editor-extension` in `world.wit`), using `kiln:extension/types@1.0.0`:
  - `publish: func(buffer: buffer-id, version: version, set: string, items: list<diagnostic>) -> result<_, error>`
    replaces the plugin's named set on the buffer, ranges in the text at `version`;
  - `clear: func(buffer: buffer-id, set: string) -> result<_, error>`.
  Both need the `decorations` capability (its prompt text, "mark up text and the gutter in your
  files", covers it) rather than a new capability, which would change kiln-command, the warnings
  and the grant records. At most 10,000 diagnostics per plugin per buffer over its sets
  (`decorations::MAX_ENTRIES_PER_BUFFER`). A range off a char boundary or past the text is
  `invalid-argument`; a version the host can no longer read is `conflict`; nothing changes on error.
- `crates/fe-plugin/src/diagnostics.rs` (+ `HostState` slice and `add_to_linker` in `lib.rs`): a
  shared `DiagnosticStore` keyed by plugin, buffer and set, checked against text through the
  decorations module's `BufferVersions`, that the app reads and is notified from, with `unload(plugin)`
  and `close(buffer)`. The default slice answers `unimplemented`, as the others do.
- The test guest (`crates/fe-plugin/tests/guests/editor/src/lib.rs`) gains `diagnose` and
  `undiagnose` commands; rebuild `editor.wasm` with its `build.zsh`.
- `crates/tenmoku-plugin/src/lib.rs`: re-export `diagnostics` beside the other short paths.

## Notes

Adding an import leaves components built against the old world loading, so the template's prebuilt
wasm is untouched. The app shows these in the follow-up card. Rebuilding the guest needs
wasm32-wasip2 (helium has it). Host-independent.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.
