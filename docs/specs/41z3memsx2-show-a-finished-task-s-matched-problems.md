---
task_uid: "01m6ezve641md6dv41z3memsx2"
task: "T419"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test task_problems_panel_tests::finished_task_feeds_the_panel (cargo nextest run -p fe-workbench) passes: when a Cargo task's session exits with rustc errors in its snapshot text, the Problems panel lists them under the file with row, column, severity and message, labelled with source 'cargo build'"
    required: true
    manual: false
  - id: "C2"
    text: "Test task_problems_panel_tests::rerun_replaces_the_previous_set (cargo nextest run -p fe-workbench) passes: a second run of the same task replaces its problems (not appends), a run of a different task keeps both sources, and a clean run clears its own"
    required: true
    manual: false
  - id: "C3"
    text: "Test task_problems_panel_tests::closing_the_tab_clears_them (cargo nextest run -p fe-workbench) passes: closing the task's terminal tab removes its problems; LSP diagnostics in the same panel are unaffected"
    required: true
    manual: false
  - id: "C4"
    text: "Test task_problems_panel_tests::clicking_a_task_problem_opens_the_location (cargo nextest run -p fe-workbench) passes: activating a row opens the file at that row and column like an LSP problem, including for a file that is not open"
    required: true
    manual: false
  - id: "C5"
    text: "Test task_problems_panel_tests::task_problems_do_not_mark_editors (cargo nextest run -p fe-workbench) passes: task problems add no squiggles or gutter marks to open editors (they are panel rows only) and the status-bar counts include them only if the existing counts already include the panel's rows"
    required: true
    manual: false
  - id: "C6"
    text: "Existing problems_panel_tests, problems_related_tests and problems_unopened_tests pass unchanged"
    required: true
    manual: false
---

# Show a finished task's matched problems in the Problems panel, replaced on each run and cleared with the task tab (Mac T1331)

## Problem
f8b runs tasks in a terminal; f8d parses output. Connect them to the Problems panel (crates/fe-workbench/src/problems_panel.rs), which today takes only LSP `FileDiagnostic`s (`Sent`/`Unopened`).

## Change
On `Exited`, read the session snapshot's full text (`Snapshot::text()`, including scrollback), run `match_output`, and hand the result to the panel as a keyed source (`ProblemsOp::SetSource { key, problems }` / `ClearSource`), rendered as extra file groups merged with LSP problems for the same path. Keep the change inside the panel's existing row model: if that needs more than adding one variant to `Sent`-like items, stop and block with the finding. Not in scope: editor squiggles for task problems, live matching while running, background tasks.

## Implementation notes

- `Snapshot::text()` has only the visible rows, split where the grid wrapped them, so a build's early errors and any location too long for a row would be lost. The scope gained `crates/fe-terminal/src/session.rs` for `Terminal::output_text()`, which has the scrollback and joins wrapped lines, and its test file.
- The row model gained one variant, `Target::Position`. A file's `Sent` now holds each problem's `Target`, so a merged file's buffer rows keep `Target::Range` and `Row` and rendering don't change. `FileDiagnostic` is `#[non_exhaustive]` outside `fe-lsp`, so a task's problem can't be one.
- Columns decode in each tool's own unit (`ColumnUnit`), checked against real compilers: rustc counts chars, go and clang bytes, gcc display cells (tab stops of 8, wide characters as two). `tsc` and ESLint weren't installed to check; they count UTF-16 by their JavaScript columns. gcc and clang share a format, so clang is told by its `N errors generated.` line. Display width needs `unicode-width`, already in the lock file, as a direct dependency.
- The source API is `ProblemsPanel::set_source` and `clear_source`, not `ProblemsOp` variants: `ProblemsOp` is `Copy` and names commands, and these carry a list.
- A source is keyed by terminal tab, not label, because two tabs can share a label while one is running. `terminal_panel.rs` isn't in scope, so a tab is tracked by its session's view, which `run_task` compares before and after `add_task`. The state lives on the panel, as `workbench.rs` isn't in scope either.
- A rerun's old problems stay listed until it exits.
- Task problems sort by byte column in the file's text: a live buffer's, an unopened file's read for its server's rows, else the file read off the main thread with the output.
- Known limits: gcc and clang print the same `file:row:col: error: message` line, so nothing on a line says which compiler wrote it. Without clang's `N errors generated.` line (`-fno-caret-diagnostics` drops it) clang's byte columns read as display columns, and one such line makes a gcc diagnostic beside it read as bytes. The row is right in both cases; the column is wrong only on a row with tabs or multibyte text. A reliable fix needs the task to say which compiler it runs, such as a `clang` problem matcher in `task_problems.rs`: a follow-up, not this card. Past `MAX_PROBLEMS` the rest are dropped with a log line. While an unopened file's text is read, a task row merged into it shows its row alone, like the file's other rows.
