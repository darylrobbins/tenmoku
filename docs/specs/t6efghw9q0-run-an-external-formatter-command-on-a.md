---
task_uid: "01m6ezvac01262t3t6efghw9q0"
task: "T414"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test external_format_tests::stdin_formatter_returns_its_stdout (cargo nextest run -p fe-workbench) passes: a fake Runner given command [\"fmt\"] receives the buffer text on stdin and its stdout becomes the formatted text"
    required: true
    manual: false
  - id: "C2"
    text: "Test external_format_tests::file_placeholder_is_replaced (cargo nextest run -p fe-workbench) passes: {file} in any argument becomes the buffer's path; with stdin = false the runner is given a temp copy to rewrite and the rewritten content is the result, and the temp copy is removed"
    required: true
    manual: false
  - id: "C3"
    text: "Test external_format_tests::nonzero_exit_is_an_error_with_stderr (cargo nextest run -p fe-workbench) passes: exit 1 yields Err whose message is the first non-empty stderr line plus the program name, and the buffer is not touched"
    required: true
    manual: false
  - id: "C4"
    text: "Test external_format_tests::timeout_kills_the_child (cargo nextest run -p fe-workbench) passes: a runner that never answers is cancelled at timeout_ms (paused clock) and reports a timeout error"
    required: true
    manual: false
  - id: "C5"
    text: "Test external_format_tests::missing_program_is_not_found (cargo nextest run -p fe-workbench) passes: a spawn error of kind NotFound is FormatError::NotFound(program), distinct from a failed run, so the caller can fall back"
    required: true
    manual: false
  - id: "C6"
    text: "Test external_format_tests::minimal_edit_keeps_the_surroundings (cargo nextest run -p fe-workbench) passes: output equal to the input is Unchanged; output differing in the middle yields one edit covering only the differing span after trimming the common prefix and suffix on char boundaries"
    required: true
    manual: false
  - id: "C7"
    text: "Test external_format_tests::real_runner_runs_a_process (cargo nextest run -p fe-workbench) passes: the real Runner with command [\"cat\"] returns its input (unix only, gated with #[cfg(unix)]; no other test spawns a process)"
    required: true
    manual: false
---

# Run an external formatter command on a document's text behind a runner seam, with timeout, exit-code and stderr handling and a minimal edit from the output (Mac T1325)

## Problem
f7a adds the settings; nothing can run a formatter yet.

## Change
New crates/fe-workbench/src/external_format.rs (declare it in lib.rs), pure of GPUI except for the executor:
- `trait Runner { fn run(&self, spec: RunSpec) -> Pin<Box<dyn Future<Output = Result<RunOutput, io::Error>>>> }` with a real implementation on `smol::process`/`std::process` run through `cx.background_spawn` (whichever fe-workbench already depends on; add no new crate), and a fake in the test file only.
- `format_text(runner, &FormatterSettings, path, text) -> Result<Option<(Range<usize>, String)>, FormatError>`: builds the argv (`{file}` replacement), pipes text on stdin or uses a temp copy next to nothing important (std temp dir, same extension, removed afterwards), enforces `timeout_ms` with the executor timer and kills the child, maps exit status != 0 to `FormatError::Failed { program, stderr_line }` and a NotFound spawn error to `FormatError::NotFound`. Output with non-UTF-8 bytes is `Failed`. Working directory: the buffer's parent (or the project root when the buffer has no path; unsaved buffers use stdin only and `{file}` becomes a placeholder name from the language's extension).
- `diff_to_edit(old, new) -> Option<Range+String>`: trims the common prefix and suffix on char boundaries so carets and folds past the edit survive. (Row-level diff is not wanted: the imara Histogram is quadratic on interleaved changes.)
Out of scope: wiring into Format Document and save (f7c), format selection.

## Implementation notes

- `format_text` also takes the `BackgroundExecutor` (the timer and the temporary copy's file work), and its path is a `BufferPath`: `Saved(path)`, or `Unsaved { root, extension }`, which is where an unsaved buffer's working directory and placeholder come from. "Unchanged" is `Ok(None)`. The real runner is `ProcessRunner`, which waits on a thread of its own rather than through `background_spawn`: the executor's timer needs a worker, so hung formatters on the fixed pool could hold up their own timeouts (review y04nsc F1).
- `{file}` becomes an `OsString`, so a path that isn't UTF-8 reaches the program as it is. With `stdin = true` it's the buffer's path; with `stdin = false` the copy's. An unsaved buffer uses stdin whatever `stdin` says, and its `{file}` is `untitled.<extension>` under the root.
- The copy keeps the buffer's file name, in a 0700 directory of its own under the system's temporary directory (a shared directory otherwise shows the text to anyone), and both go on every way out, a dropped format too. `tempfile` is only a dev-dependency, so this is by hand.
- A bare program is looked up with `LoginPath::resolve`, as language servers are, not by `execvp`: that skips a `PATH`'s relative entries, which in a buffer's directory could run a program a cloned repository ships.
- Beyond the card: exit 0 with no output from text that isn't blank is `Failed` (a tool that formats in place, set up with `stdin = true`, would otherwise empty the buffer), and stdout, stderr or a rewritten copy past 64 MiB is `Failed`. A working directory that's gone is `Failed`, not `NotFound`, though spawning reports both alike.
- The kill is the only part of the real runner no committed test covers, since C7 allows one process and that one must not hang. It was checked by hand with a throwaway test: a child ignoring `SIGTERM` goes at the `SIGKILL`, the leader is reaped, 8 MiB through `cat` and a program that never reads stdin don't deadlock, and `yes` ends at the cap.
- `timeout_ms` covers reading the copy back too, whose open doesn't block and which must be a regular file (F2). A bare program that a searched directory has as a file this user may not run is `Failed`, not `NotFound`, so it isn't a quiet fallback (F3).
- Known limits, for f7c: a run still going as the app quits outlives it unless the caller drops it, and the first run may pay for starting the login shell (up to 2 s) out of its `timeout_ms`.
