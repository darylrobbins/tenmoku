# Debug sessions in the workbench (debugger D7, T658)

Design calls for card D7 of `docs/decisions/r1b9dw3g0g-debugger.md`, where
§§1-12 leave room. §13 wins over both.

## Where the session lives

`workbench.rs` isn't in D7's scope, so the session isn't a `Workbench` field.
`debug_session::init` makes one `Debugger` entity per window as its workbench
is made (`observe_new`, as `debug_breakpoints::init` hands editors to the
store), keyed by the workbench's entity id in a global and dropped with it.
`Workbench::debugger(cx)` and `debug_session::debugger_of(&workbench, cx)`
find it. Dropping it drops the `Session`, which stops it.

One session per window (§13: multi-session is D23's): starting a
configuration stops the session there is first.

## The API later cards build on

- `DebuggerEvent`: `Starting(name)` and `StartGivenUp` for a start under
  way, `State(SessionState)`, `Stopped(StopLocation)` (epoch,
  thread, top frame, file, row from 0), `Output(OutputBody)` and
  `Breakpoints(path)`.
- `Debugger::state`, `starting`, `controls` (fe-dap's `Controls`; while a
  start is under way only Stop, which gives it up; all off with neither), `stop_location`, `verification(path)` (per breakpoint: line sent,
  verified, message, adapter line, id; kept for the session's life, replaced by
  a later reply for the file or a `breakpoint` event naming the id),
  `configuration`, `session` and `log`.

D6 draws the band and the arrow from `Stopped` and `stop_location`; nothing in
D7 draws them (the spec's order change).

## Starting

Each start is numbered from the moment it's asked for (a pick, a rerun
before its configurations are read, a debuggee tab's rerun) and is the
debugger's start under way until its session launches or it's given up.
Every later step (the listing read, the pre-launch exit, the resolution)
first checks it's still the start under way. Stop, untrusting, a root change
and a newer start give it up; a pre-launch task whose tab closes or reruns
before it exits (the seam then hands `None`) gives it up too.

- The configurations are read off the main thread with fe-dap's registry: the
  host's specs (`DebugHost`, a global tests set to the fake launcher), the
  user's `[debug.adapters]` and the built-ins, looked up on the login shell's
  `PATH`.
- Trust is checked when a configuration is picked, again before its session is
  resolved (after a pre-launch build) and again just before `Session::start`.
- A pre-launch task, and a detected Cargo configuration's build (a task of the
  Cargo source in the Build group, labelled by its command line), run through
  `Workbench::run_task_reading`, the new seam in `task_run.rs`: the task's
  ordinary tab and matcher, and at exit its code and its terminal's
  `output_text` (wrapped lines joined) handed to the caller. The program is the
  last `compiler-artifact` message for a `bin` target of the configuration's
  binary.
- The launch arguments: the adapter's `launch_defaults`, then `name`,
  `program`, `args`, `cwd`, `env`, `stopOnEntry`, the console key (§4),
  `initCommands` extended with the spec's, then `adapter_args` as written.
- A `cwd` launch_defs left to expand (`Cwd::Expand`) is checked at the start,
  by its words alone, as launch_defs checks the others: outside the root, it
  doesn't start.
- Breakpoints go in `SessionOptions::breakpoints`, each file's enabled ones by
  line on disk (`BreakpointStore::enabled`), lines only (conditions are D17's),
  read from the store in the same main-thread step that starts the session and
  subscribes to the store, so no change falls between.
- A read run's output comes from the pty, not the grid: fe-terminal gained an
  output tap (`OutputTap`, `Terminal::spawn_tapped`, the pty loop handing it
  each read before parsing) and fe-terminal-view `TerminalView::tap_output`,
  set before the session's first layout. The seam's caller says which lines it
  keeps (`KeepLine`: a cargo build keeps lines that may be artifact messages,
  any other task none), so what's kept is bounded however long the build
  prints, and no scrollback or wrapping loses it. The respec added fe-terminal's
  session, pty loop and lib, fe-terminal-view's view, and a tap test.
- Adapters are resolved for the workspace root (a root `.venv`'s Python, the
  toolchain `rustc` runs in), the program's cwd kept for its launch. A named
  adapter gets the debuggee's language: `adapter_args.sourceLanguages`' first,
  a `.py` program's Python, else Rust in a Cargo project, so lldb-dap gets the
  Rust formatters there as a detected configuration does.

## The debuggee's tab

`TerminalPanel::add_debuggee` shares `add_task`'s tab (its `TaskTab` gains a
`debuggee` flag) for the presentation: named for the configuration, kept after
exit, check mark or failure toast, started even when it isn't the active tab.
It never sets `last_task`, and `add_task`, `task_run` and `stop_task` skip it.
It reuses the configuration's tab whatever its state: a running one is hung
up, since its session was stopped to start the new one. The argv runs directly
(`TerminalOptions::shell` is the argv), never through a shell. A variable the
adapter asks to remove (a `null` value) goes through `/usr/bin/env -u NAME --
argv`, which execs the program, on Unix (alacritty's pty has no removal). A
program whose path holds `=`, which `env` would take for an assignment, is
exec'd by `/bin/sh -c 'exec "$0" "$@"'` after `env`: the path and arguments are
positional parameters, never read as shell text. On Windows, which has no
`env`, a runInTerminal asking for a removal is refused, so the program never
runs with a variable the adapter meant to keep from it.

A `runInTerminal` runs only for the current session, while it isn't stopping
(Stop, untrust and a root change set that synchronously), in the window's
root, which is trusted. The reply waits for the tab's process to start, up to
10 s (`TERMINAL_START`), checking the session each tick, and carries its pid.
A tab whose program hasn't started when its session stops or ends, or when the
wait runs out, closes, so its lazy start never runs it later. Whether it has
started is asked of the terminal when the close runs
(`TerminalPanel::close_unstarted`), not taken from the reply's polling, so a
tab whose process started between two polls keeps its tab and output; the
adapter is then told it failed if its session is over.

The panel keeps the root as the window opened it (a symlink perhaps); a rerun
compares both resolved. The tab's rerun is a Rerun Debug Session button beside the strip's `+` while
a debuggee tab is active, passed or not, and the Rerun choice of its failure
toast: both `TerminalPanel::rerun_debuggee`, which emits
`DebuggeeRerun { name, root }` (a separate event type: a new
`TerminalPanelEvent` variant would break `workbench_chrome.rs`'s match); the
debugger reruns the configuration by name as `debug.rerun_last` does. The
failure toast of a debuggee tab offers Rerun beside Show.

## Stops

The debugger checks the session's stop epoch before and after every event it
handles. Whenever it has moved on (a new `stopped`, a resume, a partial
`continued` or the shown thread's exit, which fe-dap turns into a retained
stop that stays Stopped), the last stop's place and any opening of its file
are over, and `State` is emitted again with the state as it is then (unless
the event was a state change, which is told already). `stop_location`
answers only for the session's current epoch, so a stop whose stack is slow,
empty or failed never shows the last one's.

A stop's file opens at once if it's in a tab. Otherwise it's read off the main
thread (`loader::open`), and lands (`Pane::open_at` and `Pane::file_read`, so
the pane's own read is dropped) only if its stop is still current, with the
caret then placed synchronously; the read task goes with the stop.

## Sending breakpoints

The session subscribes to its root's store. `Changed` and `Saved` send the
file's set (`setBreakpoints`) once the session is configured (Running,
Stopped, Resuming or Uncertain); before then the file waits and goes when it
is. `Moved` (unsaved edits) sends nothing. Each send is numbered per file, and
a reply is kept only if it answers the last send for its file (equal line sets
don't make an older reply current). The session's own sends, as it
configures, are answered against the set it started with; one that comes after
a send of the host's (a restart sending its first set again) is stale, and the
store's set is sent again over it.

A request that fails while the session carries on (fe-dap's `Failed`: a
Continue, a step, configuration, a host `setBreakpoints`) is said in a warning
toast with fe-dap's text as its detail, except a cancellation, `notStopped`,
and a stale or ended session's.

## Failures

A launch, `initialize` or `launch` failure whose message names ptrace on Linux,
or a native adapter's (lldb-dap, codelldb, delve, or one for Rust, C or C++)
"operation not permitted" or "permission denied", reads the scope through
`DebugHost::ptrace_scope` (`/proc/sys/kernel/yama/ptrace_scope` in the app) and
names `kernel.yama.ptrace_scope` and its value. On macOS, `get-task-allow`,
"hardened runtime" or a native adapter's "not allowed to attach" gets the
signing toast. Any other failure is "The debug session <name> failed" with
fe-dap's error as the detail; an adapter exiting by itself has a toast too.

## Settings

A window's debugger starts with the app's settings (`kiln::GlobalSettings`):
the app's first `give` to a window skips `reload_settings` when they equal the
app's. Then `reload_settings` hands `[debug]` over, for the sessions started
after it: `log_payloads` sets the session's `LogMode`, the user's adapters go
to the registry, and `timeouts` go to fe-dap. For that, fe-dap's
`SessionOptions` gained `timeouts: Timeouts { request, launch }` (defaults the
old constants; `configurationDone` takes `request`), its timeout errors carry
how long they waited, and `LocalLauncher::with_connect_timeout` takes
`connect`: the respec added fe-dap's session, launcher and tests to D7's
scope.

## Known limits

- A restart (D8) re-sends the breakpoints the session started with; D8 sends
  the store's current set after it.
