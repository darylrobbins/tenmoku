---
task_uid: "01m6j08gmvyhtp4gc0sfgxe6zp"
task: "T278"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "plugin_events::plugin_events_tests::buffer_lifecycle_reaches_a_subscribed_plugin: opening, editing, saving and closing a file deliver BufferOpened, BufferChanged (replaced ranges, inserted lengths, version after), BufferSaved and BufferClosed in order with the bridge buffer id; kinds the plugin did not subscribe to are not queued. A save names the plugin version of the text it wrote when it finishes within 256 changes of it; one finishing later isn't sent (documented limit, follow-up T288)"
    required: true
    manual: false
  - id: "C2"
    text: "plugin_events::plugin_events_tests::focus_selection_and_config_events_arrive: switching tabs sends FocusChanged, moving the caret sends SelectionChanged with at most one queued per buffer, and a settings reload sends ConfigReloaded"
    required: true
    manual: false
  - id: "C3"
    text: "plugin_events::plugin_events_tests::a_plugin_that_falls_behind_never_slows_typing: with a plugin that never drains, 1000 typed characters complete without waiting on it and it then sees lagged followed by the latest change"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Send plugins the editor events: buffer opened, changed, saved and closed, focus and selection changes, and config reloads, without ever waiting on a plugin (Mac T1347)

## Problem

A loaded plugin hears nothing from the editor: `kiln_plugin::events` (`HostEvent`, `EventQueue`,
`Recipient::deliver`) is never fed.

## Change

- `crates/fe-workbench/src/plugin_events.rs` and `plugin_events_tests.rs` (`mod` in `lib.rs`),
  hooked from `plugin_host.rs` and the workbench's buffer lifecycle in `workbench.rs`:
  `BufferOpened`, `BufferChanged` (the edits as replaced ranges and inserted lengths, and the version
  after), `BufferSaved` and `BufferClosed`, with the bridge's buffer ids; `FocusChanged` when the
  focused editor's buffer changes; `SelectionChanged`; `ConfigReloaded` on a settings or extensions
  reload. Pushed only to plugins subscribed to that kind (`EventQueue::subscribe`), delivered on each
  plugin's own thread.
- Never blocks the app: the per-plugin queue (`limits::EVENT_QUEUE`, 256) drops into `lagged` when
  full, as the queue already does; selection changes are coalesced so at most one per buffer waits.
  Nothing on the typing or paint path waits on a plugin.

## Notes

Host-independent. Fixture: `kiln/crates/kiln-plugin/tests/fixtures/events.wat` or a WAT component in
the test file.

Project rules: keep tests lean and fast; new tests go in new files; the scope lists exact files.

## Implementation notes

- **Subscribing.** kiln-plugin's `events.subscribe` import is a stub (its store can't reach the
  queue), so `plugin_host`'s linker shadows it with one that subscribes the plugin's own queue, held
  in its store. The tests' plugin, a WAT component in `plugin_events_tests.rs`, subscribes in its
  `init` through it.
- **Delivery.** `HostedPlugin` isn't kiln's `runner::Plugin`, so `Recipient` can't wrap it: each
  plugin has an `Inbox` (its `EventQueue` and a non-blocking wake onto its thread's channel). The
  thread delivers what waits after each call, one event per `handle-events` call, letting a command
  call handed in meanwhile go first. A delivery that fails unloads the plugin and stops it on the
  main thread, as a failed command does. Unloading closes the inbox at once.
- **Sources.** `plugin_events` follows the open editors only while a plugin is loaded: `track_editors`
  (workbench.rs), window activation and any window closing, however its workbench is still held
  (`App::on_window_closed`, plugin_host.rs), sync opens, closes and focus; each editor is
  observed for its notifications (its buffer's `fe_buffer::Subscription` drained into changes and
  saves, then the focused editor's selection) and for focus coming and going, since showing a tab
  focuses it without notifying the workspace. Reloads come from kiln's `ChangedFiles`, set
  once each batch of config files has reloaded, whatever it changed: the settings, an extension, its
  themes, keymaps or menus. A plugin loaded while buffers are open hears each as opened, and the focus.
- **Focus.** The focused buffer is that of the editor holding keyboard focus in the active workbench
  window: none when focus moves to a panel, the terminal or the palette, or the window deactivates.
  The selection sent is that editor's.
- **Versions.** Each change bumps the bridge's plugin version once (`plugin_bridge::advance`), so a
  change's version is the one the bridge's `edit` takes. The bridge keeps each buffer's last 256
  versions with their plugin versions, so a save finishing after more typing names the version it
  wrote, and its `open_editors` first sends the events not yet sent, so a plugin's read never counts
  a version the events then name differently; the focused buffer's selection likewise drains its
  buffer first. **Limit:** a save finishing more than 256 changes after the version it wrote finds
  that version evicted and isn't sent (logged at debug); T288 follows up (review round 2, F1). These, and `open_editors` becoming `pub(crate)`, are the
  only edits to `plugin_bridge.rs`, outside the listed scope.
- **C3.** Typing in one buffer coalesces into one waiting change however far behind the plugin is,
  so the test gives the listener a queue of one: `b.txt`'s open fills it, the first keystroke's
  change pushes it out (`lagged` 1), and the other 999 merge into that change.
