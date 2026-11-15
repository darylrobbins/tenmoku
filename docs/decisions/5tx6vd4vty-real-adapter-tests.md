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

## C2: lldb-dap on the Mac — NOT yet observed

On this Mac `lldb` itself prints "Developer mode is currently disabled." on `run`,
and lldb-dap never answers `launch` (no response, no `initialized`). A human must
enable it (`sudo DevToolsSecurity -enable` or System Settings > Privacy & Security >
Developer Mode, then restart). The lldb test and its ordering record wait on that.

## C4: dap.yml run — not yet done

The workflow needs a push and a `workflow_dispatch`, and a human risk approval at review.
