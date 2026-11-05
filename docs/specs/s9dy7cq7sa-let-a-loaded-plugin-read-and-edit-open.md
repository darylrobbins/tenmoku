---
task_uid: "01m6j08gggjwnt1cs9dy7cq7sa"
task: "T277"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "plugin_bridge::plugin_bridge_tests::buffer_ids_follow_open_buffers: a buffer has one id in every split and window while open; after it closes a read of that id is not-found and the id is not reused"
    required: true
    manual: false
  - id: "C2"
    text: "plugin_bridge::plugin_bridge_tests::a_granted_plugin_reads_edits_and_proposes: with the editor guest, read gives the text and version, edit applies one undo step with origin Host(id), a stale version is conflict, and propose shows a proposal in the editor"
    required: true
    manual: false
  - id: "C3"
    text: "plugin_bridge::plugin_bridge_tests::a_plugin_invokes_an_allowlisted_editor_command: editor.insert from a plugin granted buffer.write inserts at the active editor caret, observed with actor Host(id) and source Script; without buffer.write it is denied naming buffer.write; editor.save is denied"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench -p fe-gpui and cargo clippy -p fe-workbench -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Let a loaded plugin read and edit open buffers and invoke allowlisted editor commands: BufferAccess over the workbench buffers and a dispatcher onto the editor registry under the plugin policy (Mac T1346)

## Problem

T1062 loads plugins with fe-plugin's editor slices unimplemented: `buffer.read`, `buffer.edit` and
`buffer.propose` answer `unimplemented`, and `commands.invoke` has no `Dispatcher`, so the editor
commands open to plugins (`crates/fe-editor/src/plugin_access.rs`, 10 today) can't be reached.

## Change

- `crates/fe-workbench/src/plugin_bridge.rs` and `plugin_bridge_tests.rs` (`mod` in `lib.rs`),
  installed into each plugin's `HostState` from `plugin_host.rs`:
  - **Buffer ids.** A stable `u64` per open buffer: the same id in every split and window showing
    it, kept while it stays open, never reused in a session. Events and decorations use these ids.
  - **`fe_plugin::editor::BufferAccess`** over the open buffers: `snapshot` (text, version, path,
    language), `edit` through `Buffer::edit_from` with `Origin::Host("<id>")` (a stale version is a
    `conflict`), and `propose` as a `ProposedChange` the user reviews in the editor. It is called on
    the plugin's thread and must not wait on the main thread past the call's deadline: share the
    buffer handle, or hand off with a bounded wait; say which in the module doc.
  - **`kiln_plugin::commands::Dispatcher`**: a plugin's `Invocation` (actor `Host(<id>)`, source
    `Script`) runs on the active editor of the active window through `RunCommand`; the editor
    registry authorizes it against the `CommandHost` as `PluginPolicy`.
- `crates/fe-gpui/src/commands.rs`: a narrow hook to install that policy on the editor registry,
  which is built once in the `EDITOR_COMMANDS` thread-local (for example a forwarding
  `PluginPolicy` the registry is built with and a `pub fn set_plugin_policy`). Nothing else in
  fe-gpui.

The allowlist itself doesn't change here; the allowlist card widens it.

## Notes

Host-independent. Fixtures: the prebuilt `crates/fe-plugin/tests/guests/editor/editor.wasm` (its
`read`, `edit` and `propose` commands) and a WAT component that calls `commands.invoke`, as
`kiln/crates/kiln-plugin/tests/fixtures/commands.wat` does.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.

## Implementation notes

- **Hand-off, not a shared handle.** Buffers are `Rc<RefCell<Buffer>>` on the main thread, so each
  `BufferAccess` call is a job on a channel the main thread drains, and the plugin's thread waits
  for it until the running call's deadline (`plugin_bridge::CallDeadline`, set and cleared beside
  `HostState::deadline` in `Guest::guarded`), and no longer, started or not (`Handoff`). The
  main thread doesn't start a job once its deadline has passed or the plugin stopped waiting; one
  it started runs to its end, as nothing there stops halfway, and its answer is dropped if the
  plugin has gone. A read that times out reads as not found (the trait's `snapshot` has no
  error), by which time the call has overrun anyway.
- **Open buffers** are the tabs' editors and the diff views' working sides
  (`Workbench::file_editors`), so a buffer a diff still shows keeps its id when its tab closes.
- **Versions.** fe-buffer's `Version` has no public number, so a buffer's plugin version starts at
  1 and goes up by one each time its `Version` is seen to have changed; it is tracked with the id.
- **Authorizing invocations.** The `EDITOR_COMMANDS` registry is built with a forwarding
  `InstalledPolicy` (`fe_gpui::commands::set_plugin_policy`), which the bridge points at the
  `CommandHost`. A refusal there is only logged by the view, so to answer the plugin with it the
  bridge first validates the invocation against its own `fe_editor::commands::registry()` under
  the same policy (the view's extra commands are none of them open to plugins), then dispatches
  `RunCommand` on the active editor's focus handle. An invocation that passed that check but that
  no observer saw run (a read-only view refusing an edit) answers `failed`.
- **Proposals** go to an editor of the buffer, the active window's first; the proposal's title is
  not shown yet, as `EditorView::set_proposals` takes the change alone.
- **Scope.** `plugin_host_tests.rs` changed by two lines: its `LoadContext` literal gains the new
  `bridge` and `limits` fields, and `NoInvoke`, which it used and which nothing else needs now, is
  replaced by a bridge's dispatcher. `PluginHost::limits` (default `Limits::default()`) lets the
  bridge tests give calls a long deadline, as they pump the main thread from the test thread.
