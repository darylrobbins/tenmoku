---
task_uid: "01m6j08gs6bt2nqy8wqbejka01"
task: "T279"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "plugin_paint::plugin_paint_tests::pushed_decorations_reach_the_editor: a set pushed against a version shows in every editor of the buffer at its range mapped through later edits; clear, unload and closing the buffer remove it"
    required: true
    manual: false
  - id: "C2"
    text: "plugin_paint::plugin_paint_tests::status_items_show_and_run_their_command: set-item shows its text and tooltip on its side of the status bar in every window; a click runs its command as the plugin and a command its grants do not allow is refused; remove-item and unload take it away"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Show plugin decorations in editors and plugin status items in the status bar, a click running the item command as the plugin (Mac T1348)

## Problem

A plugin's `decorations` and `status` imports answer `unimplemented` in the app: fe-plugin's
`DecorationStore` and `StatusItems` exist but no editor or status bar reads them.

## Change

- `crates/fe-workbench/src/plugin_paint.rs` and `plugin_paint_tests.rs` (`mod` in `lib.rs`), hooked
  from `plugin_host.rs` and, for editors as they open and close, `workbench.rs`:
  - **Decorations.** One app-wide `DecorationStore`, with a `BufferVersions` over the bridge's buffer
    ids. Each editor of a buffer gets `store.provider(buffer)` through
    `EditorModel::add_decoration_provider`, removed with the editor; `DecorationStore::close` when the
    buffer closes. Unloading a plugin already drops its sets; the editors are notified to repaint.
  - **Gutter entries** (`set-gutter`) are kept by the store but not drawn: the gutter has no seam for
    them without fe-gpui changes; a follow-up card if wanted.
  - **Status items.** One app-wide `StatusItems`; each window's status bar shows them with
    `StatusBar::set_item` and `remove_item`, left or right by their alignment, re-synced when they
    change. A click runs the item's command as the plugin (`StatusItems::activate`), authorized as
    `commands.invoke` is. If `StatusBar`'s API isn't enough, stop and report rather than widen scope.

## Notes

Host-independent: model-level assertions (the editor's decoration query, the status bar's items), no
screenshots. Fixtures as fe-plugin's own `tests/decorations.rs` and `tests/status.rs` drive these
imports.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.

## Implementation notes

- **Versions.** `BufferVersions::at` needs the snapshot a plugin was shown, which fe-buffer can't
  rebuild from a newer one, so `plugin_paint` keeps the last 16 per buffer: each editor's as it
  catches up with a change (after the events of it are sent, so the plugin version is the one they
  name), each open buffer's on a sync, and each read a set makes. Several edits between two
  notifications keep only the last; a set against one in between is refused as a conflict, for the
  plugin to set again, as `BufferVersions` allows.
- **Repaint.** The store and the items have no change notification, so the linker's accessors for
  the `decorations` and `status` imports mark the call, and once it returns the plugin's thread
  asks the main thread (a bounded channel of one, coalescing) to notify the editors and sync the
  bars. Unloading, a load replacing an older one, a plugin stopping in a call, and a load failing
  after its slices attached (an `init` that drew, then failed: `Paint::forget` in
  `LoadContext::load`) drop its sets and items. That last is
  `plugin_host::plugin_host_paint_tests::what_a_failed_init_drew_goes_with_it` (review round 1
  F1), a new test file beside the others: the decorations half runs the load on a thread of the
  test's, since a gpui test runs background tasks on its own thread, which the bridge needs free.
- **Splits.** `EditorModel::split` carries providers over, so an editor seen for the first time has
  any provider of its buffer's other editors removed before it gets its own: no decoration twice.
- **Clicks.** The bar's items run kiln-shell invocations, which also reach typed and plugin commands
  with no grant check, so a plugin item's command is `workbench.click_plugin_status_item` naming
  the item; the workbench's handler runs the item's own invocation (actor `Host(<plugin>)`, source
  `Mouse`). An editor command is validated on the editor's registry under the plugin host's policy,
  as `commands.invoke`'s `Served::run` does, then dispatched on the window's active editor; one of
  the plugin's own registered commands runs as the palette runs it (`commands.invoke` can't reach
  plugin commands, but an item naming its own plugin's command is the common case); anything else
  is refused. `StatusItems::activate` isn't used: it dispatches straight onto a registry's target,
  and the editor's commands run through the view.
- **Scope.** `plugin_bridge.rs` (`CallDeadline` takes the call's `ProgramBudget`, for T278 F1),
  `plugin_host_tests.rs` (one line: `LoadContext` has a `paint` field), and one regression test
  each in `plugin_bridge_tests.rs` (F1) and `plugin_events_tests.rs` (F6), beside the helpers they
  use.
- **Carried findings.** F6: `set_plugin_extensions` tells running plugins the config reloaded,
  covering a workspace watcher's reload that publishes no `ChangedFiles`; the `ChangedFiles`
  observer stays for content-only reloads, and the queue coalesces the two. F1: the bridge's
  deadline is moved back by the call's program runs, as the runner's is.
