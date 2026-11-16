# T1844: real-adapter tests for fe-dap

Source: `r1b9dw3g0g-debugger.md` §2, §11 D16, §13 Q5.

## Design

- The driver in `crates/fe-dap/tests/real_adapter.rs` speaks the protocol
  directly (FrameReader/Writer, LocalLauncher, typed messages), not through
  `Session`, so it logs every message in arrival order. It sends `launch`
  without awaiting, sets breakpoints on `initialized`, sends `configurationDone`,
  then awaits the launch response.
- debugpy wheel pinned by SHA-256 in `scripts/dap-real.sh` (the pure-Python
  `py2.py3-none-any` wheel, 1.8.22), installed with `--no-deps --no-index` into
  a scratch venv under `mktemp -d`; the test runs it `python -I -m debugpy.adapter`.
- Native test: `lldb-dap` (PATH, `xcrun -f lldb-dap`, `FE_DAP_LLDB_DAP`), else
  codelldb over TCP. Linux skips when `/proc/sys/kernel/yama/ptrace_scope` >= 2,
  naming `kernel.yama.ptrace_scope` and the value.

## C1/C3: debugpy on the Mac (1.8.22, 2026-11-15)

`scripts/dap-real.sh`: `PASS debugpy_breakpoint_step_variables_evaluate [10.8 s]`.

Observed orders (first observations of §2's ordering claims):

- the `launch` response comes AFTER the `configurationDone` response;
- `initialized` comes AFTER the `launch` request is sent and BEFORE the launch response
  (so a client must not await `launch` before `setBreakpoints`);
- the step's `stopped` event comes AFTER the `next` response (`continued` between them).

Full order: `req:initialize evt:output evt:output evt:debugpySockets resp:initialize
req:launch evt:debugpySockets x3 evt:initialized req:setBreakpoints resp:setBreakpoints
req:configurationDone resp:configurationDone resp:launch evt:process evt:thread evt:stopped
req:stackTrace resp:stackTrace req:scopes evt:module x2 resp:scopes req:variables
resp:variables req:next resp:next evt:continued evt:stopped req:stackTrace ... resp:evaluate`.

## C2/C3: lldb-dap on the Mac (Xcode lldb-dap, LLVM 21, 2026-11-16)

Developer Mode was off on 2026-11-15 (lldb printed "Developer mode is currently
disabled." and lldb-dap never answered `launch`); with it enabled,
`PASS lldb_breakpoint_step_variables_evaluate [3.5 s]` (breakpoint at line 3 of the
Rust fixture, `next` to line 4, locals `a`, `b`, `total`, evaluate `total + 1` = 4).

Observed orders:

- the `launch` response comes BEFORE the `configurationDone` response (the opposite of debugpy);
- `initialized` comes AFTER the `launch` response (debugpy: before it), so a client must
  not wait for `initialized` before reading the launch response, nor for the launch
  response before `setBreakpoints` — it must accept both orders;
- the step's `stopped` event comes AFTER the `next` response (`continued` between), as with debugpy;
- many `module` events arrive between `configurationDone` and the first `stopped`.

Full order: `req:initialize resp:initialize req:launch resp:launch evt:initialized
req:setBreakpoints evt:module x2 resp:setBreakpoints req:configurationDone evt:breakpoint
evt:capabilities evt:process resp:configurationDone (module/breakpoint events) evt:stopped
req:stackTrace evt:output resp:stackTrace req:scopes resp:scopes req:variables resp:variables
req:next resp:next evt:continued evt:stopped ... resp:evaluate`.

## C4: dap.yml run

`workflow_dispatch` only works for a workflow on the default branch, so the run
used a temporary `push` trigger on a scratch branch (`t1844-ci`; the commits adding it
are reverted before hand-in). The jobs set `FE_DAP_REQUIRE_ADAPTERS=1`, so a missing
adapter fails. Run 43890617105, both jobs green, the logs showing `[debugpy]` and
`[lldb-dap]` observations on each runner:
https://github.com/darylrobbins/fastedit/actions/runs/43890617105

(An earlier run, 43844060279, was green before the require flag and exact assertions.)

Review fixes: values compared exactly (lldb's repl `(int) $0 = 4` is reduced to the
text after ` = ` explicitly); the Linux ptrace_scope skip still passes under the
require flag, a missing adapter does not.

`.github/workflows/dap.yml` is a new workflow file: it needs the human risk approval at review.
