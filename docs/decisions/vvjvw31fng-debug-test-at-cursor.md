# Debug Test at Cursor (T1856, debugger D13): design calls

Card: `docs/specs/vvjvw31fng-debug-test-at-cursor-for-cargo-and.md`. Record:
`docs/decisions/r1b9dw3g0g-debugger.md` §7, §11 D13, §13.

## The seam

`test_runnables.rs` resolves a test to a `ResolvedTest` (label, program, args, cwd,
env, `TestFramework`), §7's sketch. `test_command` is now `resolve_test(…).task()`, so
Run's command lines are byte-for-byte what they were (test_commands_tests unchanged).
`test_commands.rs`'s `Workbench::resolve_test_then` is the one path both
`tests.run_at_cursor`/`tests.run_row` and `debug.test_at_cursor` go through: save the
window's files, resolve off the main thread, hand the test over if the root is
unchanged. A `cfg(test)` `ConsumedTests` global records what each one consumed, which
C3 compares.

Two additions to §7's sketch, made in the framework rather than parsed back from args:

- `TestFramework::Cargo` carries `exact: bool` besides `package, target, name`. A
  module's tests, and an `#[rstest]`/`#[test_case]` function's cases, run without
  `--exact` (as Run runs them); a plain test gets `[name, "--exact", "--nocapture"]`.
- `target: Option<CargoTarget>`. `None` is a module file under `src` that both the
  library and a binary could hold (Run leaves cargo to filter every target). Debug
  needs one test binary, so it refuses this case with a toast instead of guessing.
  For a library, where Run passes no target flag, Debug builds `--lib`.

## Cargo

- **Message format (lead-approved respec of C1):** the build is `cargo test -p <pkg>
  <target options> --no-run --message-format=json-render-diagnostics`, not plain
  `json`. Its artifact messages are the same JSON. The difference is that rustc's
  diagnostics are rendered to stderr, so a failed build shows its errors in the build's
  tab and the Cargo matcher puts them in Problems. D7's detected builds do the same.
- The build runs through `run_task_reading` (trust is checked there, plus its tab and
  its matcher) in the resolved `cwd`, where Run runs `cargo test`.
- The executable is the last `compiler-artifact` with `profile.test == true` whose
  target's kind matches (any library kind for `Lib`; the kind and name otherwise) and
  that names an executable and `manifest_path`. D7's `launch::cargo_artifact` only knows
  `bin` targets, so `debug_test.rs` has its own reader rather than changing
  `debug_session/launch.rs` (out of scope).
- The session is a launch configuration of its own, named for the test ("Test: adds"):
  `program` the executable, `cwd` and `CARGO_MANIFEST_DIR` the manifest's directory,
  adapter the Rust default (lldb-dap, else codelldb), started with
  `start_debug_configuration`. That function checks trust, and checks it again before
  the launch.
- **Staleness (review F2):** each Debug Test gets a generation when it's invoked,
  before saving and resolving. That generation is checked after resolution, at the
  build's exit, and again in the deferred launch. A newer Debug Test moves it on, and
  so does any `DebuggerEvent::Starting`: the debug test follows the window's debugger
  from its first invocation, so starting an ordinary configuration during a test build
  stops that build from replacing the session. A new root has the same effect. A
  `cfg(test)` `ResolveGates` in `resolve_test_then` holds resolutions back so a test
  can deliver them in reverse order. A failed build toasts D7's "pre-launch task … failed". A build that
  names no matching executable toasts its own error.

## pytest

debugpy with `module = "pytest"` and `args = [node_id]`, in the resolved `cwd` (the
root, where Run runs pytest). The node id is the one Run passes. Run's own
`--tb=native` stays Run's.

## Entry points

- **Lens:** a test's lens items are `▷ Run test`, `Debug`, then the history text. That
  reads `▷ Run test · Debug · passed in …` and puts Debug next to Run as the B3 entry
  says. Modules keep `▷ Run tests` alone. `Debug` sends `debug.test_at_cursor` with
  `row`, as Run's item sends `tests.run_row`: one command with an optional `row`, not a
  second palette-hidden command. Scope: `tests/test_lens_history.rs` pinned the item
  positions, so the card was respecced to add it.
- **Failure detail:** a `Debug this test` button after `Ask an agent`, disabled in an
  untrusted root like that one. A test's name doesn't say its package or target, so
  (review F1) it looks at every place the name could be (`test_sources`, Open test's
  walk, up to 64 places). It resolves each one with `resolve_test` and keeps only the
  place whose `CargoTarget::binary_id(package)` and exact name are the failed
  `TestId`'s. If no place matches it says the test isn't found; if more than one
  matches it refuses to choose.
- `<leader>td` in the LazyVim preset (LazyVim's "Debug Nearest").

## Known limits (lead-approved)

- **The lens's Debug item has no accessibility node** (review F3, C5 respecced). Lens
  items are text that fe-gpui's editor element paints, with only a mouse hitbox, and
  this is true of every lens provider, so the fix belongs in fe-gpui: T1964 makes code
  lens items screen-reader buttons. Keyboard and screen-reader users reach the same
  action through "Debug: Debug Test at Cursor" in the palette and `<leader>td`. The
  failure detail's Debug this test is a kiln-ui Button with its name.

- `debug.rerun_last` after a test session looks up the root's configurations by the
  test's name and says it's gone. Rerunning a test session needs a hook in
  `debug_session.rs` (out of scope); `<leader>td` / the lens do the rerun meanwhile.
- `debug.stop` doesn't give up a test build that's still running (the debugger's start
  begins after the build). Closing or rerunning the build's tab does, and so does a
  newer Debug Test.

## Carried finding

T1576 F9 (module keys) was already fixed on main by T1876 (`ModuleKey` with binary and
path apart, test `tests::test_run_view_module_keys`). This card touches
`test_run_view.rs` only to add the button.
