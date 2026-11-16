# Stepping and breakpoint commands, the F-keys and the floating debug toolbar (debugger D8, T1850)

The design calls this card made in building D8 of the debugger record
(`docs/decisions/r1b9dw3g0g-debugger.md` §5 "Debug toolbar" and "Run to
cursor", §6, §11 D8, §13 Q4 and B3) to D0's design
(`docs/decisions/skgc7fe5z9-debugger-design.md`, `docs/design/design-system.md`
Debugger), and the Mac look check (C8).

## Calls

- **Three key context flags, not one.** §6 names `debug_stopped`, but its
  F-key table also has "while a session exists" (Shift+F5, Restart) and
  "while running" (F6) rows. The workbench root's `key_context` carries
  `debug_session` while a session is starting or hasn't ended, then
  `debug_stopped` or `debug_running` (`Debugger::key_flags`). They are flags
  on the `workbench` frame, next to the plugin loads' flags, so each
  binding's context (`workbench && debug_stopped`) sits at the depth of the
  menu bar's `workbench` F10, and the later entry wins (kiln's matcher:
  deepest, then later). F11's fullscreen has no context, so the step wins
  over it at any depth.
- **"Stopped" for the keys is literal (review F3).** `debug_stopped` is
  set only while the session is Stopped with stepping enabled, which is D2b's
  control-state query (`Debugger::is_paused`). Resuming or uncertain, the
  session is `debug_running`, so Linux's menu bar and fullscreen keep F10
  and F11, as §13 Q4 says. A first draft also kept stepping keys through
  Resuming and Uncertain, so that F10 pressed again mid-step wouldn't open
  the menu bar. Review round 1 held that against Q4, since Uncertain can
  last. The toolbar test checks the flags, and F10 and F11 on Linux's
  keymap in the window's real context, in all four states.
- **Where each key applies.** F5 (`debug.start`, which continues while
  stopped) applies across the workbench. F9 (`debug.toggle_breakpoint`)
  applies in the editor, so the table's "editor" row is literal. Shift+F5
  and secondary-Shift+F5 apply under `debug_session`, F6 under
  `debug_running`, and F10, F11, Shift+F11 and secondary-F10 under
  `debug_stopped`. Run to cursor is bound in the workbench, not only the
  editor: it uses the active editor's caret, and says so when there is no
  file.
- **A refused control does nothing.** Continue, Pause, the steps, Restart
  and run to cursor ask the session, which refuses any control its
  control-state query has disabled. The command then does nothing: the
  toolbar shows the button disabled, and a toast on every early F10 would
  be noise. With no session they say "No debug session is running", as
  D7's `debug.stop` does.
- **Run to cursor's breakpoint is hidden in the send, not in the store.** It
  goes into its file's `setBreakpoints`, in line order, unless one of the
  user's enabled breakpoints is on its line already. The adapter's answer
  for it is dropped from the session's verification, so the gutter and
  D9's Breakpoints never show it. Its line on disk comes from the store's
  own buffer-to-disk row map (new `BreakpointStore::lines_on_disk_in`), as
  a breakpoint added there would be placed, so an unsaved edit above the
  caret doesn't misplace it.
  - **The Continue waits for the adapter's answer (review F1, rounds 1
    and 2).** The Continue goes only once the send is answered, and only
    if the breakpoint is still that run's (a token per run to cursor). It
    resumes through fe-dap's new `Session::resume_at`, which refuses
    unless the session's stop epoch is still the one it was asked at. The
    check runs under the lock the resume is sent under, so no stop can come
    between them (round 2 found the separate check racy). A send that fails
    or times out is said, as any failed breakpoint update is. The
    breakpoint then goes, the user's set is sent again, and nothing runs.
    An answered breakpoint isn't required to be verified: an adapter holds
    one in code that isn't loaded yet as pending, and binds it on load.
  - **Removals are tracked until answered (review round 2 F5).** A hidden
    breakpoint taken out leaves its file "unconfirmed" until a send of the
    file without it is answered. That covers retirement at a stop, a failed
    run, and a run to cursor replacing an earlier one in another file. A
    send that fails while taking one out keeps the file unconfirmed, and so
    does one a later send superseded. Each run to cursor sends every
    unconfirmed file again, then its own. It continues only once all of
    them are answered and none failed.
  - **Removal.** The first `stopped` event, or move to Stopped (a probe's
    proven stop after a failed resume), at an epoch other than the one it
    was asked in takes it out and sends the file's set again. That covers
    a stop on it, on a user's breakpoint first (C1's test), a step or a
    pause. The session's end also drops it. A Continue the session refuses
    takes it straight back out.
- **Start with arguments: a prompt, then the picker.** The prompt is a
  one-row picker, as `lsp.rename`'s is. Its row reads "Start debugging with
  ‹args›". Input a shell would ask more of refuses the pick and says why:
  a quote left open, or (review F6) a backslash at the end, which escapes
  nothing. The arguments split as a shell splits them (quotes kept
  together, backslash escapes, `\\` a backslash), then
  `debug.start`'s configuration picker shows, and the configuration picked
  starts with them after its own `args`. They're expanded like the
  configuration's, so `${file}` works. LazyVim's `<leader>da` continues a
  stopped session instead. Here it always starts afresh, since its point
  is new arguments.
  - **The arguments are their configuration's (review F4).** Each fresh
    start records them by configuration name: the picker's, with or
    without arguments, and `Workbench::start_debugging`. The reruns
    (`debug.rerun_last` and a debuggee tab's) add that configuration's
    again, as VS Code's restart keeps them. Starting another configuration
    neither carries nor loses them, and a fresh start without arguments
    drops them. A plain `debug.start` clears any left by a dismissed
    picker.
- **Show Session lists the one session** (v1), with its state, from the Runs
  strings (Starting…, Running, Stopped). Picking it opens the stopped file
  at its line. With none, it says no session is running.
- **The toolbar's test is "is there a title bar", not the platform** (D0).
  The workbench draws the bar only without its own title bar
  (`own_title_bar`, which `set_title_bar` records). With a title bar it
  draws nothing: the run widget's session controls are D8b's.
- **It gives way to modals (review F2).** Pickers and prompts open over
  the panes' top, where the bar floats, so it isn't drawn while one shows,
  and the workbench redraws as the modal layer changes. That covers Show
  Session, Start with Arguments and the command palette. The bar is the
  workbench's own overlay, painted after the workspace, so this is
  simpler than moving it under the shell's modal layer.
- **Continue or Pause, by state.** Continue shows while stopped. Pause shows
  while running, resuming, starting or restarting. Both show while
  uncertain. A Continue or step asked for and not yet taken counts as
  resuming (the control-state query says so before the state event comes),
  so the button swaps at once.
- **Tooltips name the key that runs the button's command, with one
  exception.** Continue's tooltip names F5, `debug.start`'s key, which
  continues while stopped. `debug.continue` itself is palette-only, as §6's
  table has no key for it. Each key is read from the live keymap with the
  matching flag set, so a user's rebinding shows.
- **Icons.** Continue is `play`, Pause `pause`, Restart `rotate-ccw` and
  Stop `square`; all four were already in the set. D0's new Lucide icons
  are added for the steps: `redo-dot` (Step Over), `arrow-down-to-dot`
  (Step Into) and `arrow-up-from-dot` (Step Out). They come from 1.52.0 at
  the pinned commit, ISC with no Feather contributors, and their stroke is
  set to 1.75. The label carries `bug` and opens Show Session.
- **Look** (design system, Debugger): the design's "overlay surface" is
  the floating surface `surface.raised`, as the toasts' cards have it (the
  kiln token `surface.overlay` is a scrim, `#00000080` in Yohen). Then the
  floating shadow (`float_shadow`), a `border.default` hairline, 6 px
  radius (`radius::MENU`), 30 px tall (`height::TOOLBAR`) with 4 px padding,
  22 px icon buttons 2 px apart and 6 px before Restart. "4 px below the
  tab strip" is 4 px below the top of the panes' content, which is below
  the breadcrumbs when they show, so the bar never covers them. It's placed in
  prepaint from the editor area as that frame laid it out, as the toasts
  are, so a dock or panel change moves it in the same frame. It occludes
  the editor under it.
- **Public API for hosts.** `Workbench::start_debugging(name)` (the audit
  scene starts a configuration by name), `Debugger::is_paused`,
  `Debugger::key_flags`, `Debugger::run_to_cursor_target`, the
  `DEBUG_SESSION`, `DEBUG_STOPPED` and `DEBUG_RUNNING` constants, and
  `BreakpointStore::lines_on_disk_in`.

## Scope added by respec (revisions 2 and 3)

- `crates/fe-workbench/src/workbench_chrome.rs`: `set_title_bar`
  notifies the workbench, so the toolbar follows the title bar. Whether
  it shows is main's `own_title_bar`, which the agents chip already reads.
- `crates/fe-workbench/src/debug_breakpoints.rs`: `lines_on_disk_in`, for
  run to cursor.
- `crates/fe-workbench/src/debug_session/commands.rs`: the commands, as a
  child of `debug_session.rs`, whose session fields it uses.
- `THIRD_PARTY.md`, `kiln/THIRD_PARTY.md`: the three icons in the Lucide
  row's file list (`crates/fastedit/tests/notice.rs` checks it).
- `crates/fe-dap/src/session.rs`, `crates/fe-dap/src/tests/mod.rs`,
  `crates/fe-dap/src/tests/session_resume_at.rs` (revision 3):
  `Session::resume_at`, for review round 2's F1.
- `crates/fe-gpui/src/keymap_tests.rs`, `crates/fe-gpui/src/vim/tests.rs`:
  both list the contexts or command prefixes of fe-workbench's bindings,
  which fe-gpui's catalog can't check. `debug.` joins the prefixes.

## The Mac look check (C8)

`crates/fe-workbench/examples/screenshots/audit/debug_toolbar.rs` hides the
title bar, as on Linux or macOS without the unified one. It installs fe-dap's
fake adapter as the host's, trusts the folder, sets a breakpoint on the
loop's body in `src/main.rs` and starts the configuration. One step waits off
the main thread, up to 10 s, for the fake to stop. A last step fails the
scene unless the session stopped on that row. The toolbar then shows the
label, Continue, the three steps, Restart and Stop, all enabled. Its token
probes are on the overlay surface and the hairline, checked exactly at the
capture's own scale, so nothing skips on `SCREENSHOTS_ALLOW_OTHER_SCALE`.

The scene opens no demo files (`open_demo: false`). The capture's root is
the temporary folder's unresolved path, `/var/folders/…`, while the
breakpoint store and the session use the resolved one, `/private/var/…`,
so the fake's program path is the resolved one, as an adapter reports it.
The stop then opens `src/main.rs` in a tab of its own. With the demo's
tab open too, the strip showed two `main.rs` tabs, told apart by their
folders. Trust is set a frame before the start, because it applies only
once the root's resolved path is known.

### Recorded (2026-11-15, the lane's Mac: Mac15,11, Retina, captures at 2×)

`cargo run -p fe-workbench --example screenshots -- audit <dir>` printed, for
this card's surface:

```text
wrote <dir>/debug_toolbar_dark.png
ok      debug_toolbar_dark probes (4)
wrote <dir>/debug_toolbar_light.png
ok      debug_toolbar_light probes (4)
```

The toolbar is at x 525–757 and y 62–92: 30 px tall, centred over the
editor area. The pixels at the four points, read back from the captures:

| Point | Yohen | Hakuji | Token |
| --- | --- | --- | --- |
| (527.5, 77) padding left of the label | `#2a211c` | `#ffffff` | `surface.raised` |
| (640, 90) padding below Step Into | `#2a211c` | `#ffffff` | `surface.raised` |
| (657, 62.5) top edge | `#2e251e` | `#e1e3e7` | `border.default` |
| (525.5, 77) left edge | `#2e251e` | `#e1e3e7` | `border.default` |

The last whole audit run (every surface) also failed the probes of 10
other surfaces this card doesn't touch: chrome, settings, shortcuts editor,
changes, git log, problems panel, problems, terminal session, terminal and
the git log card. That drift is already on main, as T1849 found.

