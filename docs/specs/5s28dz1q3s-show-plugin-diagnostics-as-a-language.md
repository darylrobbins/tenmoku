---
task_uid: "01m6j054wzye55fj5s28dz1q3s"
task: "T1355"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "plugin_diagnostics::plugin_diagnostics_tests::published_diagnostics_show_in_the_editor_and_the_problems_panel: a published error is in the editor diagnostics (so squiggle, inline diagnostic and diagnostics.next reach it) and in the Problems panel under its file with its source, or the plugin name when it has none"
    required: true
    manual: false
  - id: "C2"
    text: "plugin_diagnostics::plugin_diagnostics_tests::server_and_plugin_diagnostics_coexist: a fake language server diagnostic and a plugin diagnostic on one buffer both show; clearing either leaves the other"
    required: true
    manual: false
  - id: "C3"
    text: "plugin_diagnostics::plugin_diagnostics_tests::unloading_reloading_or_closing_clears_them: each removes the plugin diagnostics from editors and the panel"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass, the existing lsp diagnostics and problems panel tests unchanged"
    required: true
    manual: false
---

# Show plugin diagnostics as a language server diagnostics show: squiggles, inline diagnostics and Problems panel rows, alongside the servers own for the same buffer

## Problem

The diagnostics a plugin publishes (`fe_plugin::diagnostics`) reach no editor. They should look like a
language server's: squiggles, inline diagnostics (T1317), next and previous diagnostic, and Problems
panel rows.

## Change

- An editor keeps one `DiagnosticSet`, keyed by one type pair: `EditorModel::apply_diagnostics`
  replaces a set keyed by other types. LSP keys it by `(ServerId, (session, incarnation))`
  (`crates/fe-workbench/src/lsp.rs`, the `LspEvent::Diagnostics` and `DiagnosticsCleared` arms), and
  `ServerId` can't be made outside fe-lsp. So `crates/fe-workbench/src/diagnostic_owner.rs` defines
  `DiagnosticOwner { Server(ServerId), Plugin(u64) }` and an identity enum
  (`Lsp(session, incarnation)`, `Plugin(load)`), and `lsp.rs`'s apply, clear and active-identity
  sites use them. Nothing else in the LSP path changes.
- `crates/fe-workbench/src/plugin_diagnostics.rs` and `plugin_diagnostics_tests.rs` (`mod` in
  `lib.rs`), hooked from `plugin_host.rs`: on each `DiagnosticStore` change, apply the plugin's sets
  (merged) to every editor of the buffer under `Plugin(<plugin key>)`; clear them on unload, reload
  and close. A diagnostic without a `source` shows the plugin's name.
- The Problems panel lists an open buffer's diagnostics from the buffer, so it needs no change; plugin
  diagnostics are for open buffers only.
- `docs/plugins/authoring.md`: a Diagnostics section.

## Notes

`problems_panel.rs` is untouched, so this is apart from T1331's task problems. Host-independent.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.
