# The Debug tool window (debugger D9, T1852)

The debugger record (`docs/decisions/r1b9dw3g0g-debugger.md` §5 "Panels",
§11 D9, §13 "B3 design first") and the design system's Debugger section
("The Debug tool window") ask for Variables, Watch, Call Stack and
Breakpoints as one bottom tool window. This file keeps the calls made
building it (`crates/fe-workbench/src/debug_panel.rs` and
`debug_panel/{stop,rows,breakpoints}.rs`).

## Calls

- **Registered as Runs is.** The panel is one more `terminal_dock` tool
  window (`workbench.rs`), a `DockItem` in `dock.rs`, with its focus handle
  in the bottom tools' focus-out list and in `workbench_chrome.rs`'s
  `BOTTOM_BUTTONS`, `focus_bottom_panel` and `bottom_panel_focused`, so the
  rail button lights while it shows, `dock.grow`/`dock.shrink` resize the
  bottom dock from it, and a rail click hides it as Runs' does. It isn't in
  `RESTORABLE`: a relaunch has no session, and the rail has no Debug button
  until one starts, so a restored Debug tab would have nothing behind it.
- **The rail button joins after Runs.** kiln-shell's rail actions could only
  be appended, which put Debug after Terminal. `DockState::add_action_after`
  (kiln-shell) inserts it after Runs' action; an id already there is replaced
  in place, as `add_action` does, so a locale change re-adding the actions
  keeps it where it is. It joins on the window's first session
  (`DebuggerEvent::State(Launching)`), as §5 has it; the design's other
  trigger, a root with a debug configuration, is left for the run widget's
  card, which lists configurations. Its dot is `BadgeTone::Accent` (hare's
  fur) while a session is stopped, named "Debug, stopped".
- **Shown once per session, without focus.** `focus_on_show` is false: the
  stop takes the caret to its line, and stealing focus into the panel at
  launch would take it from there. `debug.show_panels` focuses it. "Once
  per session" is per `Debugger::session_id`, not per `Launching` state: an
  emulated restart launches the adapter again in the same session and
  leaves a hidden window hidden and a picked tab picked (review F9).
- **Two tabs, no Console yet.** The design's header is Variables,
  Breakpoints and Console in the bottom panel's own tab strip. The bottom
  dock's strip lists tool windows, so the window draws its own row of tabs
  (`height::CONTROL`, 26 px) under it; Console joins it in D10. The session
  label ▾ is left for multi-session (D23): one session per window today.
- **Its own fetches.** The session fetches the top frame's scopes and their
  variables itself (fe-dap's `StopData`), but tells no event when they land
  (`DebuggerEvent` has no variant for them, and adding one breaks
  `debug_runs.rs`'s exhaustive match, out of scope). The panel asks for the
  selected frame's scopes and an expanded node's children itself. The cost
  is one `scopes` and one `variables` request per stop, both small.
- **Children once per stop, by reference; expansion by path.** A stop's
  `variablesReference`s are good for that stop only, so fetched children
  are kept by reference with the stop and dropped with it; whether a node
  is expanded is kept by its path (scope or watch, then names), so a node
  expanded at one stop is expanded, and fetched once, at the next.
- **Paging.** An adapter that gives a node's `indexedVariables` above 100 is
  asked `filter: indexed`, `start`, `count` 100 at a time, each "Show 100
  more" asking for the next page; its named children are asked for once
  with `filter: named` and listed first, unless it counts them 0
  (`namedVariables` is optional, so an absent count still asks), for a
  scope, a variable and a watch alike (review F3, rounds 1 and 2). An adapter that doesn't count
  them is asked once and shown 100 at a time.
- **Exception filters are the window's, per adapter.** The record keeps no
  filter choice in the state file, so a toggle lasts for the window: kept on
  the `Debugger` by adapter name, sent with `setExceptionBreakpoints` at
  once while a session is live, given to each later session of that
  adapter (`SessionOptions::exception_filters`), and handed to the live
  session with fe-dap's new `Session::set_exception_filters`, which every
  later configuration of that session sends instead of its options': an
  emulated restart (no `supportsRestartRequest`, or a failed restart)
  launches and configures the adapter afresh (review F4). After a native
  restart (`Restarting` → `Running`/`Stopped`) they're sent again.
- **Watches are strings in `watches`.** `BreakpointStore::watch_expressions`
  reads the strings in the state file's `watches` list and
  `set_watch_expressions` writes them back as a list of strings, through the
  breakpoints' debounced write. An entry that isn't a string is dropped
  once the panel writes the list.
- **Other threads.** The stopped thread holds the frames the stop fetched
  (fe-dap's first page, 20); another thread's are fetched when it expands.
  A thread other than the stopped one reads "· paused": DAP doesn't say
  per thread whether it runs without `allThreadsStopped`, and every adapter
  we ship stops all threads. Past a page, a "Show more frames" row asks for
  the next (`startFrame`), and `debug.frame_up` past the last frame fetched
  asks for it and selects the caller once it comes, unless a frame was
  selected meanwhile, even the same one (review F5, F13). `totalFrames` is
  optional and may overcount, as the protocol warns, so a stack has more
  until a page comes back shorter than asked for, or the total is reached;
  the row names how many only when the adapter gave a total.
- **Frame selection.** Selecting a frame, or activating the selected one
  again, reveals its line (review F8); a new selection re-scopes Variables
  and Watch. The reveal and the caller band run once the workbench's update
  ends and are dropped if another selection or another stop came first;
  each selection gives up the previous frame's open still under way,
  a frame with no file too, so a late read never opens over a later
  choice (review F2). Selected from the tool window, focus goes back to it
  once the file shows, as VS Code's call stack keeps it, so the keys keep
  walking frames.
- **Frame arrows in rows.** The current frame's row carries ▶ and a selected
  caller's ▷, in the accent, as text in a 12 px slot: the gutter's arrow
  glyphs are fe-gpui's painter, not an element a row can reuse.
- **Breakpoints tab rows** read an open file's rows and lines from its
  first editor's buffer in the window (`BreakpointStore::placements_in`),
  so its unsaved edits move them as the gutter shows them and the
  session's verification pairs by the line on disk; a closed file's come
  from disk, its code read off the main thread and dropped whenever the
  store changes; a read started before a change lands nothing (review F10). Problems' grouping (`problems_panel.rs`) isn't reachable
  as an API, so its file row's look (file icon, muted path, count chip) is
  copied.
- **Keys.** Tab and shift-tab go round the parts the tab shown draws: the
  row of tabs (left and right switch tabs, as a click does), Call Stack,
  Watch, the Add watch input and Variables, or the Breakpoints tree (review
  F6). Variables' empty state draws none of its trees, so only the tabs are
  reached then; the program running on while the Add watch input has
  focus moves focus and the keys to the tabs (review F11). Watch scrolls its
  selected row, and the Add watch row, into view (review F12). Each
  tree keeps a selected row: up/down move, right/left expand and collapse,
  enter acts (select a frame, open a breakpoint's line, show more), space
  toggles a breakpoint's or filter's checkbox, delete removes a watch. A
  first stop that switches to Variables moves the keys to Call Stack
  (review F7). The Add watch input takes typing and the caret's
  `text_input` commands (its context joins the caret keys' hosts in
  `fastedit.toml`), enter adds the watch and escape goes back to Watch
  (review F1). With vim on,
  the dock-leave keys name the window's context, `debug_panel`, as they
  name `runs_panel` (`fastedit.toml`, and fe-gpui's context pins).

## Limits

- The Call Stack column is 280 px (`width::CALL_STACK_DEFAULT`), not yet
  resizable.
- A watch's or variable's value isn't editable (Set Value is later, §5).
- `sourceReference` frames are listed, faint, but don't open (later, §5).

## Mac look check (C8)

Run on the Mac (Apple Silicon, 2× display) on 2026-11-16 with
`cargo run -p fe-workbench --example screenshots -- audit <dir>`, the
scene `crates/fe-workbench/examples/screenshots/audit/debug_panel.rs`:

```
ok      debug_panel_dark probes (3)
ok      debug_panel_light probes (3)
```

The scene: a session of the fake adapter stopped on the loop's body of
`src/main.rs`, the bottom panel showing the Debug tool window (shown by
the session's launch), on Variables. Call Stack has the stopped thread
("main · stopped: breakpoint") and its frame, selected, with the filled
arrow and `main.rs:9` faint; Watch a watch with a value and type and one
that fails in shinsha with ✕; the Add watch input; Locals expanded, a
`HashMap` value ending in an ellipsis with its type faint at the right;
Globals collapsed. The rail's Debug button sits after Runs with the hare's
fur dot. Yohen and Hakuji both read as the design's artboards: 24 px rows,
the hairline between the columns, names in the property colour.

Its probes, in logical pixels on flat fills (so they hold at 1× as at 2×;
the scene reads no `SCREENSHOTS_ALLOW_OTHER_SCALE`, and nothing in it
skips): the tool window's ground (`surface.panel`) at (400, 500), Call
Stack's hairline (`border.subtle`) at (561, 500), and the selected frame's
row (`list.row_selected_inactive`) at (540, 423), measured from the
captures with a stdlib PNG reader.
