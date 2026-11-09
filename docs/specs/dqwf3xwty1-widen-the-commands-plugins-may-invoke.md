---
task_uid: "01m6yhweq69729ntdqwf3xwty1"
task: "T534"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "editor_save_is_plugin_callable_with_its_grants: on the editor registry editor.save runs for a plugin granted buffer.write and fs.write:workspace and is denied naming the missing one otherwise (crates/fe-editor/tests/plugin_access_save.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "plugin_access::plugin_access_tests::each_new_command_runs_with_its_grant_and_is_refused_without: one row per added command: with its grants it runs; without its capability, or without commands for those needing none, it is denied"
    required: true
    manual: false
  - id: "C3"
    text: "plugin_access::plugin_access_tests::open_path_stays_inside_the_workspace: workbench.open_path opens a workspace file at its row and column and denies a path outside the roots, a .. escape and a symlink out"
    required: true
    manual: false
  - id: "C4"
    text: "plugin_access::plugin_access_tests::save_refuses_an_untitled_or_outside_buffer: a plugin editor.save on an untitled buffer or one outside the workspace roots is denied and nothing is written"
    required: true
    manual: false
  - id: "C5"
    text: "plugin_access::plugin_access_tests::a_plugin_cannot_invoke_another_plugins_command: invoking \u003cother id\u003e.\u003cname\u003e is denied"
    required: true
    manual: false
  - id: "C6"
    text: "cargo nextest run -p fe-editor -p fe-workbench and cargo clippy -p fe-editor -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Widen the commands plugins may invoke: editor.save, a new workbench.open_path at a row and column, back and forward, next and previous diagnostic, reveal in the tree and Show Problems, each refused without its grant (Mac T1357)

## Problem

Plugins may invoke only 10 editor commands (`crates/fe-editor/src/plugin_access.rs`: insert, newline,
moves, select all, fold and unfold). They can't save, open a file at a position, or navigate.
Workbench commands are kiln typed commands, which have no plugin authorization at all
(`kiln::commands::dispatch_typed`; `kiln_command`'s `authorize` is crate-private), so none of them is
reachable.

## Change

Add only commands that are safe under the grant model; every other command stays refused by default.

| Command | Needs beyond `commands` | Why it is safe |
|---|---|---|
| `editor.save` | `buffer.write`, `fs.write:workspace` | writes the active buffer to its own file; refused for an untitled buffer or a file outside the workspace roots |
| `workbench.open_path` (new: `path`, optional `row`, `column`) | `fs.read:workspace`, `path` a path arg | opens a workspace file at a position; the registry's path check refuses a path outside the roots, a `..` escape or a symlink out |
| `nav.back`, `nav.forward` | none | moves the caret through history the user made |
| `diagnostics.next`, `diagnostics.previous` | none | moves the caret in the active editor |
| `project_panel.reveal_active` | none | shows the active file in the tree |
| `problems.show` | none | opens a panel |

Not added: running any command by name (a plugin's `commands.invoke` already does that, within the
allowlist), and **another plugin's commands**: a plugin invoking `<other id>.<name>` is refused, so
grants can't be laundered through a second plugin.

- `crates/fe-editor/src/plugin_access.rs`: `editor.save` in `PLUGIN_CALLABLE` with
  `[BufferWrite, FsWrite(Workspace)]`; update the module doc.
- `crates/fe-workbench/src/plugin_access.rs` and `plugin_access_tests.rs` (`mod` in `lib.rs`): a
  `kiln_command::Registry` of plugin-facing entries for the workbench commands above, each declared
  with `allow_plugins` and forwarding to the typed command once authorized, with the `CommandHost`
  policy installed. The bridge's dispatcher (`plugin_bridge.rs`) sends ids this registry has to it,
  and refuses `editor.save` when the active buffer is untitled or outside the workspace roots.
  Nothing in kiln changes (kiln/** is T1268's).
- `workbench.open_path`, "Open Path" (typed, hidden from the palette if it needs args), in
  `workbench_commands.rs`, with its ids where `typed_command_ids_tests.rs` and
  `command_catalog_tests.rs` require.
- `docs/plugins/authoring.md`: the callable commands and what each needs.

## Notes

`editor.save` runs format on save, which may run the user's own configured formatter; that is the
user's configuration, not the plugin's choice of program. Host-independent.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.

## Implementation notes

- `crates/fe-editor/tests/plugin_access.rs` pinned the old 10-command allowlist and asserted
  `editor.save` closed; it was respecced into scope and now lists `editor.save` among the open ones.
- The bridge's `Served` (`plugin_bridge.rs`) holds the editor registry, the workbench's
  plugin-facing registry (`plugin_access.rs`) and the policy; `Served::new` installs the policy on
  all three, so the tests run plugins' invocations through it directly against a stub policy.
- Authorized workbench commands are forwarded with `kiln::dispatch_invocation` in the active
  window, stamped with the focused element's key contexts, or `[workbench]` when nothing in the
  window is focused.
- `workbench.open_path` takes 0-based `row` and `column` (chars), as `Place` counts them, and
  opens through `Workbench::open_document_at`.
- An id neither registry has is refused as `Undeclared` (not `NotFound`) when it is a loaded
  plugin's (`<extension id>.<name>`, the invoker's own included) or another typed command; an id
  nothing knows is still `NotFound`.
- `editor.save`'s workspace check resolves the roots and the file component by component, as
  kiln's path-arg check does, so a dangling symlink counts as its (outside) target; it refuses an
  untitled buffer with `MissingPath`.
- Review round 1 (F2): a save queued behind another, or one with a save hook, prepares its job
  after an await, by when a Save As could have moved the buffer. So a plugin's save is refused
  while `EditorView::is_saving`, and runs with the save hook taken off for its one synchronous
  dispatch (put back after), so its job takes the checked path at once. This departs from the
  Notes: a plugin's save runs no format on save. Validating the destination after hooks would need
  fe-gpui's save path, outside this card.
- Review round 1 (F3): with no focus, or focus on an undrawn element, `dispatch_invocation` sends
  from the window root, which no workbench handler is on; forwarding first focuses where the
  workbench puts focus and refuses if the command's action still isn't available there.
