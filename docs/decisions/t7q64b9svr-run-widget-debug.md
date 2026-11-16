# The title bar's run widget debugs (debugger D8b, T1851)

The design calls this card made in building D8b of the debugger record
(`docs/decisions/r1b9dw3g0g-debugger.md` §13 "B3 design first") to D0's
design (`docs/decisions/skgc7fe5z9-debugger-design.md`,
`docs/design/design-system.md` Debugger, "The run widget"), and the Mac look
check (C5).

## Calls

- **One choice, a task or a launch configuration.** D0's "the run widget
  picks one thing". The choice is `RunChoice::Task(label)` or
  `RunChoice::Debug(name)`, per root, in the window's saved layout under
  `run_tasks` as the task choice was. A task's is still saved as its label,
  so layouts saved before this card restore unchanged. A configuration's is
  saved as `{"debug": name}`, which an older build skips. With nothing
  chosen, or the choice gone, it's the default build task, else the first
  task, else the first configuration. With neither tasks nor configurations
  the widget draws nothing, as with no tasks before.
- **The configurations are `debug.start`'s, read apart from the tasks
  (review F2).** The widget reads them through
  `Workbench::read_launch_configs`, the listing the configuration picker
  shows: the launch file's, `launch.json`'s and the detected ones (D4), each
  with its blockers. Tasks and configurations are two reads, each numbered
  and checked against the root, so a slow configuration read (in a Java
  project with jdtls running it asks for main classes, bounded by D21b's
  wait) never holds up the widget's tasks or Play. The picker opens once the
  tasks are read, with the configurations read so far, and takes the fresh
  ones when they land, keeping its query. A chosen configuration counts
  only once the root's configurations are read.
- **What triggers a configuration read (review F1).** A root change; a
  watcher batch touching the tasks files, `.tenmoku/`, `.vscode/launch.json`,
  `Cargo.toml`, `src/main.rs` or `src/bin/` (Cargo's binary targets, which
  detection lists); the picker opening; and a change in what detection
  reads beside the files: the active editor's language (the current Python
  file is a configuration of its own) and the `[debug]` settings (a user's
  adapter makes a configuration naming it startable). The widget compares
  both on each change it follows, and follows the debugger, which now
  notifies when its settings are reloaded. jdtls becoming ready isn't an
  event the widget hears: its classes show at the next read.
- **A configuration chosen hides a task's run.** D0 gives a chosen
  configuration only 🐞 Debug, so a task still running (from `tasks.run`,
  say) has no ■ in the widget then. It keeps its terminal tab, with its
  stop, and its row in Runs, as D0 says of a task running beside a
  session.
- **The picker has two groups.** Tasks, then Debug configurations, each
  under a faint heading (`UI_SMALL`, muted). A configuration's row has the
  `bug` icon and its source as a faint suffix: `launch.toml`,
  `launch.json`, or "detected". Typing filters both by name, a
  configuration by its source too. Headings show only once the root has a
  configuration, so a tasks-only picker is unchanged (T1539's tests keep
  their keys). kiln-shell's picker has no unselectable rows: a heading is a
  row whose pick is refused (Enter on it does nothing), and the selection
  starts on the first real row after every query. The arrow keys can still
  rest on a heading, which a screen reader then hears as the group's name. A configuration that can't start is listed muted, and
  can be chosen: the widget then says why it can't debug it.
- **Buttons follow the choice** (D0). A task: ▷ (■ while it runs) and, if
  the root has a configuration, 🐞 Debug…, which dispatches `debug.start`:
  its own picker, at the configurations. A configuration: its label carries
  the `bug` icon (between the name and the chevron, the label's row being
  reversed for the chevron), and only 🐞 Debug shows.
- **Debug dispatches `debug.start` with the configuration's name.**
  `debug.start` takes an optional `configuration` argument now (a mouse
  dispatch, like `lsp.run_lens`'s `lens`): with it, the command starts that
  configuration afresh through `Workbench::start_debugging`, which reads the
  list again, asks for trust in an untrusted folder, and toasts if the
  configuration is gone or blocked. Without it, `debug.start` is as before.
  The palette and F5 still give no argument.
- **A configuration that can't start** leaves Debug disabled; its tooltip
  reads "Can't debug: ‹blockers›", the blockers as `launch_defs` words them
  (the picker's "why"). A disabled kiln `IconButton` keeps its tooltip.
- **Keys in tooltips.** Debug… shows F5, `debug.start`'s key, which opens
  the same picker. Debug shows none: no key starts a chosen configuration.
  The session's buttons show the keys the floating toolbar shows, read from
  the live keymap with the debugger's flags (`debug_toolbar::key`).
- **While a session runs, the widget is its controls** (D0), built from the
  floating toolbar's button specs (`debug_toolbar::specs`, now shared), so
  both have the same order, enabled states, names and commands. "Runs" is
  the toolbar's test, `Debugger::is_active`: starting, or not yet ended.
  The label names the configuration, with `pause` while stopped and the
  spinner otherwise, and its accessible name is the toolbar label's ("Debug
  session: main, stopped at line 3"). Then 22 px icon buttons 2 px apart:
  Continue or Pause (both while uncertain), Step Over, Step Into, Step Out,
  6 px, Restart, Stop. The toolbar draws only without a title bar
  (`own_title_bar`), the widget only in one, so a window never has both.
  The widget redraws when the session's look (label, state, each button's
  enabled state) changes: the workbench notifies on every debugger event
  but output.
- **The session menu.** The session label opens a picker-style menu (as Show
  Session's): Show Debugger, Show Adapter Log (`debug.show_log`), and Choose
  Task or Configuration…, which opens the widget's picker. Show Debugger
  dispatches `debug.show_panels` unless the bottom panel already shows the
  Debug tool window, since that command toggles. D0 also lists Show
  Console: the Console tab is D10's and hasn't landed, so the item comes
  with it.
- **Strings and names.** Everything new is in `l10n/en-US/debug.ftl` under
  `workbench-debug-run-…`; the session's names reuse the toolbar's
  `workbench-debug-toolbar-…` messages, which are D0's table's. Every
  control has an accessible name (Debug, Debug…, "Debug configuration:
  ‹name›", the session's). No session change is announced here: D7's
  debugger already says "Debugging…", stops, "Running" and the end.

## Scope added by respec (revisions 2 to 4)

- `crates/fe-workbench/src/debug_toolbar.rs`: `Spec`, `specs`, `key` and
  `label_name` are `pub(crate)`, so the widget's session controls are the
  toolbar's, not a copy.
- `crates/fe-workbench/src/workbench_commands.rs`: `debug.start`'s
  `configuration` argument.
- `crates/fe-workbench/src/workbench_chrome.rs`,
  `crates/fe-workbench/src/workbench.rs`: the per-root choice is a
  `RunChoice` (the layout field's type and its save and restore).

- `crates/fe-workbench/src/debug_session.rs` (revision 4, review F1):
  `reload_debug_settings` notifies the debugger, which the widget follows.

`title_bar.rs` is untouched: the widget still fills the right slot through
`run_widget::install`.

## The Mac look check (C5)

`crates/fe-workbench/examples/screenshots/audit/debug_run_widget.rs` keeps
the title bar the capture shows, installs fe-dap's fake adapter as the
host's, trusts the folder, sets a breakpoint on the loop's body in
`src/main.rs` and starts the configuration, as `debug_toolbar.rs`'s scene
does (the same path and trust traps apply). Three steps on frames of their
own wait for the stop (review F3): while the start still reads its
configuration on the foreground there's no session, and a step returns at
once so the frame between runs that work; once there is one, a step waits
off the main thread, up to 10 s, for the fake to stop. A last step fails
the scene unless the session stopped on that row. The pointer then rests on Restart. Its four token
probes are checked exactly at the capture's own scale, so nothing skips on
`SCREENSHOTS_ALLOW_OTHER_SCALE`.

### Recorded (2026-11-16, the lane's Mac: Retina, captures at 2×)

`cargo run -p fe-workbench --example screenshots -- audit <dir>` printed,
for this card's surface, with and without `SCREENSHOTS_ALLOW_OTHER_SCALE=1`:

```text
wrote <dir>/debug_run_widget_dark.png
ok      debug_run_widget_dark probes (4)
wrote <dir>/debug_run_widget_light.png
ok      debug_run_widget_light probes (4)
```

The widget shows "demo" with `pause`, then Continue, Step Over, Step Into,
Step Out, Restart and Stop, x 798 to 944, 22 px each, 2 px apart and 6 px
before Restart (x 898 to 920). No floating toolbar shows. The pixels at the
four points, read back from the captures:

| Point | Yohen | Hakuji | Token |
| --- | --- | --- | --- |
| (880, 6) the title bar beside the widget | `#120e0b` | `#f2f2f4` | `surface.chrome` |
| (895, 19) the 6 px before Restart | `#120e0b` | `#f2f2f4` | `surface.chrome` |
| (900, 10) Restart, hovered, above its icon | `#2b221d` | `#ebecef` | `list.row_hover` |
| (918, 28) Restart, hovered, below its icon | `#2b221d` | `#ebecef` | `list.row_hover` |
