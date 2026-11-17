# The Debug Console (debugger D10, T1853)

The debugger record (`docs/decisions/r1b9dw3g0g-debugger.md` §5 "Debug
Console", §11 D10, §13) and the design system's Debugger section
("Console") ask for the adapter's output by category and a one-line `repl`
input with history and expandable results. This file keeps the calls made
building it (`crates/fe-workbench/src/debug_panel/console.rs`).

## Calls

- **A tab of the Debug tool window, not of the terminal panel.** D0
  (`docs/decisions/skgc7fe5z9-debugger-design.md`) moved the console beside
  the frames it evaluates in, so `terminal_panel.rs` is unchanged. The code
  is `debug_panel/console.rs`, a child of `debug_panel` as `stop`, `rows`
  and `breakpoints` are, rather than a `src/debug_console.rs` beside it:
  it shares the window's private state (the stop, its fetched children,
  expansion, keys and focus), and a sibling module would have needed all
  of that made crate-visible. The tabs read Variables, Breakpoints,
  Console, as the design's header does.
- **Shown on Console as a session launches.** The design: "A session
  opens the tool window on Console once; its first stop switches it to
  Variables unless you picked a tab since." D9 opened it on Variables;
  `session_began` now shows Console, and the first stop's switch is D9's.
  A new session (by `Debugger::session_id`, so not an emulated restart)
  also empties the console, as VS Code gives each session its own; the
  input's history stays for the window.
- **Rows by category.** `console` and no category are the adapter's own
  messages (faint); `stdout` and an adapter's own categories are program
  output (ink); `stderr` is shinsha with a 2 px shinsha edge in the row's
  left padding (`glyph::STROKE`); `important` is kiseto (`status.warning`)
  with ▲. `telemetry` is dropped by fe-dap already
  (`session/actor.rs`), and the console drops it too, so an event that
  reaches it from elsewhere isn't shown.
- **A row a line; streams continued.** An event's text is split at `\n`
  (`\r\n` taken as one), each line a row, so multi-line output keeps its
  rows and a blank line stays a row. Text that doesn't end its line is
  continued by the next event of the same category, as a program writing
  `print("a", end="")` then `print("b")` shows one line `ab`; an event of
  another category, or an evaluation, starts a row of its own.
- **One row height.** Every row of the console's list is 18 px, small
  code's line height: the design's output rows, and also the echo, result
  and child rows, because the list is a `uniform_list` (one height for
  all rows) so that a long log stays cheap to draw. Result and child rows
  are therefore denser than Variables' 24 px rows; the chevrons and
  indents are Variables'.
- **Bounded.** The newest 5000 entries (output lines and evaluations,
  `MAX_ENTRIES`) are kept, the oldest dropped first; a selected row moves
  up with them. The history keeps the newest 100 (`HISTORY_MAX`), and a
  repeat of the last isn't added again.
- **Following the end.** New output scrolls the list to its end if it was
  at its end (or not yet scrollable); scrolled up to read, it stays put.
  An evaluation always scrolls to its echo.
- **Evaluation.** Enter sends `evaluate` with context `repl`, the
  `frameId` of the frame Call Stack has selected (D9's selection, so a
  caller frame's locals are reachable). With no stop shown (the program
  running) it goes with no frame, as VS Code's does, and the adapter
  answers or explains. The request is the console's own: a frame
  selection or a new stop doesn't cancel it. Its result is announced,
  polite, as its value ("read as its value", the design's announcements),
  or as "Error: ‹message›".
- **Results expand while their stop lasts.** A result's
  `variablesReference` is good for the stop it was evaluated in only, so
  its children are fetched into that stop's children (by reference, as
  Variables' are, so a result and a variable of the same reference share
  one fetch) and expand as Variables' do: once per stop, 100 at a time,
  paged like D9's. Once the stop is over the row keeps its value and
  type, and stops expanding; its children go with the stop.
- **History.** Up recalls the previous entry, down the next; up at the
  oldest stays there; down past the newest gives back what was being
  typed before recalling, and does nothing when not recalling.
- **Keys.** Tab and shift-tab go round the row of tabs, the output list
  and the input (D9's traversal). In the list, up and down select a row,
  right and left expand and collapse a result or child, enter toggles it
  or shows more; escape in the input goes back to the list. The input
  takes the caret's `text_input` commands through the same `debug_panel`
  key context as Add watch.
- **`debug.show_console`.** Shows the Debug tool window on Console with
  the input focused (with no session, the list or empty state has the
  keys); if the bottom panel shows the tool window on Console already, it
  hides it, focus going back to the editor if it was in the window, as
  `debug.show_panels` does. Shown on Variables or Breakpoints, it switches
  to Console instead of hiding. Picking Console this way counts as a
  pick, so the first stop leaves it.
- **No session.** The tab shows the design's empty state, "Start a
  session to evaluate expressions.", while it has no rows; with rows left
  from an ended session, they stay and that sentence takes the input's
  place.
- **Accessible names.** The list is "Console output" (`Role::List`), each
  row a `ListItem` named by its text (a blank line "Blank line", an error
  line "‹text›, error output", an important one "‹text›, important"), an
  echo "› ‹expression›", a result "‹value›, ‹type›", a failure
  "Error: ‹message›", children as Variables names them. The input is
  "Evaluate in ‹frame›" for the selected frame's function, else its
  placeholder, "Evaluate an expression". kiln-ui's `TextInput` gains
  `set_name`, as the name follows the frame selection.

## Review findings carried from T1852

- **F12.** `focus_part` now selects the first row if none is, clamps a
  selection past the end, and scrolls it into view (`reveal`, also used by
  up and down), so escape and shift-tab back from Add watch reveal Watch's
  selected row. `watch_keys_scroll_their_row_into_view` checks both.
- **F14.** `pick_tab` calls `leave_hidden_part`, which now moves focus to
  the tool window from either input (Add watch, Console) whenever the
  part shown doesn't draw it. `clicking_a_tab_moves_focus_off_the_input_it_hides`
  clicks Breakpoints with Add watch focused and toggles a breakpoint with
  space; the console's C4 test clicks Variables with the console input
  focused.

## Limits

- ANSI styling (`supportsANSIStyling`) and completions are later (§5).
- Output `group`s (start, startCollapsed, end) are shown as plain rows, and
  an output event's own `variablesReference` (structured output) isn't
  expandable.
- A long line is cut with an ellipsis, its full text in the row's
  tooltip; there's no wrapping or horizontal scroll, and no copy yet.
- The run widget's session menu item "Show Console" (design system) is
  the run widget's card; `debug.show_console` is in the palette.
- Logpoint output is D17.

## Fixed on the way: Watch's rows shrank instead of scrolling

D9's Watch tree caps its height (`WATCH_MAX_HEIGHT`, 160 px) and scrolls
past it, but its rows, a flex column's children, shrank to fit the cap
(measured 11.5 and 14 px rows with 11 watches) rather than overflowing,
so it never scrolled and many watches drew squashed. Its heading, rows
and Add watch row are now `flex_none`. The F12 regression found it: with
the rows squashed every row was "in view".

## Mac look check (C6)

Run on the Mac (Apple Silicon, 2× display) on 2026-11-17 with
`cargo run -p fe-workbench --example screenshots -- audit <dir>`, the
scene `crates/fe-workbench/examples/screenshots/audit/debug_console.rs`,
twice, the second time with `SCREENSHOTS_ALLOW_OTHER_SCALE=1` set (the
scene reads no such variable, and nothing in it skips):

```
ok      debug_console_dark probes (3)
ok      debug_console_light probes (3)
```

The scene: a session of the fake adapter that wrote mixed output as it
started, stopped on the loop's body of `src/main.rs`, the bottom panel
(340 px here, so every row shows) on the Debug tool window's Console tab,
shown by `debug.show_console`. The rows: the adapter's launch message
faint, three lines of program output in ink, the error stream's two
lines (one event) in shinsha with the 2 px edge, an important message in
kiseto with ▲, then `› counts` in muted with its result expanded (its
type faint at the right, the two children in the property colour) and
`› total` with `✕ total isn't defined` in shinsha; the `›` input under a
hairline. Yohen and Hakuji both read as the design describes.

Its probes, in logical pixels on flat fills, so they hold at 1× as at 2×:
the tool window's ground (`surface.panel`) at (900, 340), the error
stream's edge (`status.error`) at (283, 383), and the hairline over the
input (`border.subtle`) at (900, 550), measured from the captures with a
stdlib PNG reader. A first placement of the edge probe between rows
failed both captures, so the probes do judge.
