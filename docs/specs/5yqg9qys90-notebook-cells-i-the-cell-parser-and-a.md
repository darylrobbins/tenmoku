---
task_uid: "01m7th4f03me51tz5yqg9qys90"
task: "T697"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "notebook_cells_tests::splits_on_percent_lines_and_markdown_variants: `# %%`, `#%%`, `# %% title`, `# %% [markdown]` split a file into cells; `# %%%`, a marker mid-line and one inside a triple-quoted string do not; a file with no marker has no cells."
    required: true
    manual: false
  - id: "C2"
    text: "notebook_cells_tests::cell_source_keeps_file_line_numbers: cell_source gives the trimmed code and the file line of its first row."
    required: true
    manual: false
  - id: "C3"
    text: "notebook_kernel_tests::namespace_persists_across_cells: through the real kernel script and the interpreter python_interpreter resolves, `x = 2` then `print(x * 3)` streams `6`; the last expression's repr comes back as a result and None gives none."
    required: true
    manual: false
  - id: "C4"
    text: "notebook_kernel_tests::error_traceback_uses_file_lines: `raise ValueError` in a cell at file line 14 reports ename, evalue and a traceback naming that file and line 14, with no kernel frames."
    required: true
    manual: false
  - id: "C5"
    text: "notebook_kernel_tests::output_caps_and_queue: output over 1 MiB is truncated with a dropped-bytes note, queued executes run FIFO, a queue over 256 refuses the rest, interrupt stops a running `while True` cell and keeps the queue."
    required: true
    manual: false
  - id: "C6"
    text: "notebook_kernel_tests::kernel_death_fails_running_and_queued_cells: killing the child delivers Died once and fails the running and queued cells; shutdown and drop leave no process in the group."
    required: true
    manual: false
  - id: "C7"
    text: "The design choice (own stdlib-only kernel script, not ipykernel) and the protocol are in notebook_kernel.rs's module doc, with the Jupyter-compatible message names; nothing outside the scope changes."
    required: true
    manual: false
---

# Notebook cells I: the # %% cell parser and a persistent per-file Python kernel process, headless (Mac T1979)

# Notebook cells I: the `# %%` cell parser and a persistent Python kernel process, headless

Phase 1 of 3 of "Python notebook cells in a plain .py file" (no .ipynb): this card has no UI. It builds the two pieces the UI cards (nb2: headers, commands, text and error outputs; nb3: images) sit on. Survey first: `fe_dap::registry::python_interpreter(root, None, exists)` is how the Python tasks pick the project's interpreter (`<root>/.venv/bin/python`, else `python3`; `task_defs.rs::python_tasks` shows the call; the `python_environment` setting in fe-config/settings.rs overrides it for the language server, read how `lsp_python_env_tests.rs` resolves it and use the same answer). `run_process.rs` (RunProcess) closes stdin, so the kernel cannot use it; it spawns its own child the way RunProcess does, with `crate::process_group::lead_group` and the group stop (SIGTERM then SIGKILL) and only in a trusted folder (`trust.rs`).

## Decision: our own kernel, not Jupyter
Recommended and implemented here: a small stdlib-only Python script (`notebook_kernel.py`, embedded with `include_str!`, written to the state/cache dir and run as `<python> -u <script>`) that holds one namespace per file and speaks JSON lines over its stdin/stdout. Why not ipykernel/jupyter_client: neither is in most project venvs (a plain `python -m venv` has neither), jupyter_client needs ZeroMQ and a connection-file handshake, and "run a cell" would fail with "install ipykernel" on a fresh project; the stdlib script works on any Python 3.8+ with nothing installed. What we give up: Jupyter widgets, completion/inspect, rich `display_data` beyond what we add. The message names mirror Jupyter's (`stream`, `execute_result`, `error`, `display_data`) so an ipykernel-backed transport can be added later behind the same `Kernel` trait without touching the UI. Record this in the file's module doc.

## Cell parser (`notebook_cells.rs`, pure, no GPUI)
- `parse_cells(text: &str) -> Vec<Cell>`: a cell marker is a line that, after optional leading whitespace-free start (column 0 only), is `# %%` followed by end of line or a space and a title (the VS Code/Jupytext/Spyder form). `# %% [markdown]` (and `# %% [md]`) makes a markdown cell; `#%%` with no space is also accepted (Spyder). `# %%%`, `# %% x` in the middle of a line and a marker inside a triple-quoted string are not markers (use the line scan with a simple string-state check, or ask the syntax tree if one is cheap; document the choice).
- `Cell { marker_row: Option<u32> (None for the preamble before the first marker), rows: Range<u32> (body rows, excluding the marker), kind: Code|Markdown, title: Option<String> }`. Text before the first marker is a code cell (the preamble) only if it is non-blank. A file with no markers has no cells (the feature is off for it).
- A markdown cell's body is the comment lines with the leading `# ` stripped; running it does nothing.
- `cell_source(cell, text) -> CellSource { code: String, first_line: u32 }`: the code with a code cell's rows, trailing blank rows trimmed, `first_line` the file line of the first row, so tracebacks show real file lines.

## Kernel (`notebook_kernel.rs` + `notebook_kernel.py`)
- `Kernel::start(python: PathBuf, cwd: PathBuf, file: PathBuf, sink)` spawns the process; a reader thread parses lines into `KernelEvent`s delivered through a channel the caller polls (no GPUI in this module; the UI card wraps it). `Kernel::execute(id, source, file, first_line)`, `Kernel::interrupt()` (SIGINT on Unix; on Windows `CTRL_BREAK_EVENT` is not available through our spawn, so interrupt is documented as unsupported there and the UI offers Restart), `Kernel::shutdown()` and Drop kill the group.
- Protocol (host to kernel): `{"op":"exec","id":N,"code":"...","file":"/abs/x.py","line":12}`; (kernel to host): `{"id":N,"kind":"stream","name":"stdout"|"stderr","text":"..."}`, `{"id":N,"kind":"result","text":"repr"}`, `{"id":N,"kind":"error","ename":"ValueError","evalue":"...","traceback":["File \"/abs/x.py\", line 14, in <module>", ...]}`, `{"id":N,"kind":"done","ok":true|false}`, plus one `{"kind":"ready","python":"3.12.1"}` at start.
- The kernel runs code in one persistent `__main__`-like namespace (`__name__ == "__main__"`, `__file__` set), `sys.path[0]` the file's folder. The code is `compile`d with the file's name and its line numbers shifted by `first_line`, so a traceback reads like the file. If the last statement is an expression its value's `repr` is the `result` (not for `None`), as a REPL shows. `sys.stdout`/`sys.stderr` are replaced with writers that emit `stream` messages (batched at most every 50 ms or 4 KiB), the protocol uses the original stdout fd saved at startup, and stdin reads (`input()`) raise `EOFError` with a clear message (no stdin UI in this phase). Tracebacks drop the kernel's own frames.
- Caps (constants, tested): a stream longer than `OUTPUT_CAP` (1 MiB per cell run) is truncated with a final `stream` saying how many bytes were dropped; one line longer than 64 KiB is split; a kernel line that is not valid JSON or over 16 MiB ends the kernel with `KernelEvent::Died`.
- Cells run one at a time per kernel; `execute` while one runs queues it (FIFO, queue bounded at 256, the rest refused with an error event), so Run Below can enqueue every cell. A queued cell can be cancelled (`cancel_queued`), and interrupt cancels the running one and keeps the queue unless asked to flush.
- A kernel that exits unexpectedly delivers `KernelEvent::Died { status }` and fails any running or queued cell with it; `Kernel` is then dead and the caller starts a new one (restart = shutdown + start; the namespace is gone, the doc says so).

## Linux
Everything is headless and runs on Linux with `python3` on the PATH (the tests assert it is there; they do not skip). No Windows interrupt.

## Not here (later cards)
Cell headers, commands, keybindings, inline blocks (nb2); images and matplotlib (nb3); `input()` UI, `display()`/`_repr_html_`, completion, an ipykernel transport, a per-file interpreter choice UI.

## Helium filing note

- Lent from the Mac as y151, the user's request (notebook cells NB1-NB3, test coverage COV1-COV3, reading mode).
- **Never touch crates/fe-workbench/src/lsp_consent*.rs** (Mac T1978 and T1701 own it).
- linux-ok: headless tests. The macOS gate runs when the batch lands on the Mac.
- Builds and loops: CARGO_TARGET_DIR inside your worktree, never /tmp.
- Golden and pin tests: a new setting changes the settings schema/template goldens, settings_model.rs and the
  l10n files; a new command changes typed_command_ids_tests.rs and command_catalog_tests.rs; a new binding can
  break binding-count tests in fe-gpui. The lead pre-added the ones it knew of. If another golden or pin
  fails, widen your scope with `rgt task respec --scope` (it replaces the list, so pass the whole list) before
  you finish, and say why in the spec's Implementation notes.
- Workbench-wide settings reach live state only through workbench_root.rs reload_settings; per-editor fields
  ride EditorView::set_wrap_settings/apply_settings. If you need a file that's out of scope for that, respec first.
- Ask the lead (`herdr agent prompt wR7:p1 ...`) before rgt finish if anything in the criteria can't be met as written.
- The kernel is a real python3 (helium has 3.14). Run it off the UI thread with timeouts (a dedicated thread, not a blocking executor worker), and kill it on drop so nothing outlives a test.

## Implementation notes

- `Kernel::start` takes a `KernelSpec { python, cwd: TrustedFolder, file, script, env }` and returns the
  `Kernel`, whose `events()` is the unbounded `async_channel` the caller polls (the "sink"). `cwd` is a
  `TrustedFolder`, made only by `TrustedFolder::check(folder, |f| kiln_shell::trust::decided(f, cx))`, so
  trust is checked without GPUI in the module. `write_script(dir)` writes the embedded script
  (`script_dir()` is `<state_dir>/notebook`); `interpreter(root, python_environment)` is the LSP's
  `find_python_env`, falling back to `python_interpreter`.
- The output caps (1 MiB per run with a final "[output truncated: N bytes dropped]" stream, 64 KiB line
  split) are enforced in the kernel script, which also captures fds 1 and 2 for subprocess output, so the
  note is not a host l10n string; the host ends the kernel on a non-message line or one over 16 MiB.
- The kernel also sends `{"id":N,"kind":"busy"}` (Jupyter's `status: busy`, `KernelEvent::Started`) once a
  cell is its to interrupt; `interrupt` holds the signal until then (review F1). A stdout that closes without
  the process exiting within a second is a death too (`DeathCause::OutputClosed`, F2).
- Refused and abandoned cells come as `KernelEvent::Failed { cell, why }` (QueueFull, Cancelled, Died,
  ShutDown); `Died { status, cause, last_words }` comes once, before the failures. `interrupt(flush)`.
- Design calls: `docs/decisions/5yqg9qys90-notebook-cells-i-the-cell-parser-and-a-p.md`.
- The kernel tests are Unix only (`#[cfg(all(test, unix))]`): they signal processes and interrupt is Unix's.
