# Notebook cells I: the cell parser and a persistent kernel (T697, Mac T1979)

The code is `crates/fe-workbench/src/notebook_cells.rs` (the `# %%` parser)
and `crates/fe-workbench/src/notebook_kernel.rs` with its script
`notebook_kernel.py`; their module docs are the reference, including the
protocol. These are the calls the spec left to the worker, or that it made
and asked to be recorded.

- **Our own kernel, not ipykernel.** A stdlib-only script, embedded with
  `include_str!` and written to `<state_dir>/notebook/` under a name its
  content hashes to, run as `<python> -u <script> <config json>`. ipykernel
  and jupyter_client are in few project environments, jupyter_client needs
  ZeroMQ and a connection-file handshake, and a fresh `python -m venv` would
  answer "run a cell" with "install ipykernel". We give up widgets,
  completion and inspection, and rich display beyond what later cards add.
  Message names follow Jupyter's (`stream`, `error`, `execute_result` as
  `result`, `execute_reply` as `done`), so an ipykernel transport can sit
  behind the same `KernelEvent`s later.
- **The parser scans lines; it doesn't ask the syntax tree.** What can hide
  a marker line is a triple-quoted string, or a one-quote string continued
  by a backslash at a line's end, so the scan tracks which string a line
  starts inside of, skipping comments and one-line strings so their quotes
  don't count. It stays pure and works before a tree exists. A markdown tag
  counts only on its own or before whitespace (`# %% [md]notes` is code).
- **The kernel enforces the output caps, not the host.** All of a cell's
  output reaches the host through the kernel's own writers: `sys.stdout`
  and `sys.stderr`, and pipes on fds 1 and 2 for subprocesses and C code,
  which the kernel reads itself. So one place caps a run at 1 MiB (the note
  "[output truncated: N bytes dropped]" is the last stream, before `done`),
  splits lines at 64 KiB characters, and attributes subprocess output to its
  cell (on Unix it drains those pipes before `done`). The host still ends a
  kernel that sends a line over 16 MiB, or anything that isn't a message.
- **fd 0 reads `/dev/null`.** The protocol reads a private duplicate of the
  original stdin, so a subprocess can't consume the protocol; `input()`
  raises `EOFError` saying there's no stdin.
- **The queue is the host's.** The kernel only ever has one cell: the next
  queued one is written to its stdin when it says `done`. Cancel and flush
  are then edits to a host-side list, and the kernel reads stdin only
  between cells, so an interrupt can't land in the middle of reading one.
- **Interrupt is SIGINT to the group**, as Ctrl-C in a terminal: the cell's
  subprocesses get it too. The kernel's handler raises `KeyboardInterrupt`
  only while a cell is its, and defers it while the main thread is writing a
  protocol line, so a message is never cut short. The kernel says `busy`
  (Jupyter's `status: busy`) once a cell is its; the host holds an interrupt
  of a cell whose `busy` hasn't come, even one given before the kernel was
  up, and sends it then. The host writes the next cell only after `done`,
  which the kernel sends after the cell's run, so a signal sent for one cell
  lands before the next is read.
- **Text belongs to the cell running when it was written.** The batcher tags
  each piece as it's written, so a thread's text written between cells goes
  with a null id however late it's flushed, and each run's cap counts only
  its own text.
- **Trust is a type.** `Kernel::start` takes a `TrustedFolder`, which only
  `TrustedFolder::check` makes, from the folder's canonical path and the
  caller's trust lookup (`kiln_shell::trust::decided`). That keeps GPUI out
  of the module while making an unchecked start impossible to write.
- **The interpreter is the language server's.** `notebook_kernel::interpreter`
  asks `fe_lsp::find_python_env` (the `lsp.python_environment` setting,
  `$VIRTUAL_ENV`, `.venv`, `venv`), as the Python servers do, and falls back
  to `fe_dap::registry::python_interpreter` (`python3`), as the Python tasks
  do.
- **Ending a kernel.** A death (an exit by itself, a bad message, or stdout
  closing without an exit within a second) kills
  the group at once and sends `Died` once, with the end of the kernel's
  stderr, then fails the running and queued cells. Shutdown and drop send
  `SIGTERM`, then `SIGKILL` two seconds later, and fail what's left as shut
  down, with no `Died`. The leader is reaped only after the group's last
  signal.
- **Tests are Unix only.** They kill processes and check groups with
  `kill(2)`, and interrupt is Unix's. They need `python3` on the `PATH` and
  fail without it.
