---
task_uid: "01m55v1bst7jd34csndd3chqfg"
task: "T179"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench unit test problems_panel::problems_panel_tests::unopened::unopened_files_are_listed_and_open_on_click passes: an unversioned publication for unopened src/other.rs lists it with row:col decoded past an emoji, counted in the status item, and a click opens it with the range selected; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test problems_panel::problems_panel_tests::unopened::an_open_file_is_listed_once passes: while src/other.rs is open its buffer's problems are listed and the stored ones aren't; closing it lists the stored ones again"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench unit test problems_panel::problems_panel_tests::unopened::cleared_and_restarted_servers_drop_them passes: an empty publication, and the server's status becoming Restarting, remove the rows"
    required: true
    manual: false
  - id: "C4"
    text: "Disk reads for listed files run off the UI thread (loader::read on the background executor); the problems_panel.rs module docs' known-limit paragraph is replaced by the new rule"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# List language servers' problems in files that aren't open in the Problems panel, read against the file on disk, as VS Code does, instead of open buffers only (Mac T929)

## Problem

LSP completeness track, item 2 (user-approved 2026-10-17). Once the card this depends on lands, fe-lsp reports a
server's diagnostics for files it doesn't have open as `LspEvent::FileDiagnostics { server, uri, path,
encoding, items }`, with raw LSP ranges, and `fe_lsp::decode_file_diagnostics` decodes them against a text.
The workbench ignores the event (its catch-all, `crates/fe-workbench/src/lsp.rs:1598`), so the Problems panel
still lists open buffers only (`crates/fe-workbench/src/problems_panel.rs:25-26`, "a file that isn't open
isn't listed, whatever a server knows of it"). VS Code lists every file a server has published for, and its
status bar counts include them: after `cargo check`, rust-analyzer's errors in files the user hasn't opened
show there.

## Change

1. `crates/fe-workbench/src/lsp.rs`:
   - Keep `file_diagnostics: HashMap<(ServerId, PathBuf), (PositionEncoding, Vec<FileDiagnostic>)>` on the
     workbench's LSP state. `FileDiagnostics` with a `path` (resolved as `lsp::resolve` resolves paths)
     replaces the entry, an empty one removes it; one without a path is ignored. A server's `Status` becoming
     `Restarting` or `Stopped`, and the manager going (the root changing, the gate closing), remove its
     entries. Each change runs `sync_problems`.
2. `crates/fe-workbench/src/problems_panel.rs` `sync_problems`:
   - A stored file that no open editor shows is listed with the open files, in the same path order, its rows
     built from `decode_file_diagnostics` against the file's text on disk, and counted in the status items.
     The text is read off the UI thread with `loader::read`, once per change to the store or to the file
     (the panel re-lists when the read lands; until then the file's rows show without a column). A file that
     can't be read (gone, binary, larger than `DEFAULT_MAX_FILE_BYTES`) is listed with each position as sent
     (`line+1:character+1`).
   - A file some open editor shows is listed from its buffer, as today, never also from the store, so no
     row appears twice.
   - Clicking such a row, or `problems.open` on it, opens the file in the active pane, as quick open does,
     and selects the problem's range, decoded against the opened buffer's text.
   - The module docs' "known limit" paragraph describes the new rule instead.
3. Tests: new `crates/fe-workbench/src/problems_unopened_tests.rs`, declared at the end of
   `crates/fe-workbench/src/problems_panel_tests.rs` as `#[path = "problems_unopened_tests.rs"] mod unopened;`,
   using that file's `setup` helpers and the fake of `tests/lsp.rs`, with a UTF-16 fake and `src/other.rs` on
   disk holding an emoji before the problem:
   - `unopened_files_are_listed_and_open_on_click`: the fake publishes, unversioned, an error in the unopened
     `src/other.rs`. Problems lists `src/other.rs` with its row, `row:col` counted in characters past the
     emoji as for an open file, and the error count includes it. Clicking the row opens `src/other.rs` in
     the active pane with the problem's range selected.
   - `an_open_file_is_listed_once`: with `src/other.rs` opened and the fake then publishing for its open
     document, the panel lists the open buffer's problems only; closing the editor lists the stored ones again.
   - `cleared_and_restarted_servers_drop_them`: an empty publication for `src/other.rs` removes its rows; a
     second publication, then the fake crashing (status `Restarting`), removes them too.

Not here: `workspace/diagnostic` pulls. As far as their sources show (not checked against live servers), no
built-in server (rust-analyzer, pyright, typescript-language-server) advertises `workspaceDiagnostics: true`,
so they would add nothing today. Linux-OK.

## Implementation notes

- The path in `FileDiagnostics` is resolved (`lsp::resolve`, falling back to the path as sent) in the
  manager's sink, on the server's thread, so the main thread does no file system call for it and the
  events keep their order.
- A listed file's path is the workbench root joined with its path under the manager's resolved root,
  as quick open builds it, so a symlinked root lists and opens it as the root names it. A file counts
  as shown when an editor's path, or that buffer's resolved path, is it.
- An open file's `row:col` counts the column in bytes of the row (`EditorModel::problems`); a stored
  file's is counted the same way after decoding, so the emoji in the test's `src/other.rs` puts `bad`
  at `2:21` either way (`an_open_file_is_listed_once` shows the open buffer's row reading the same).
- The text cache entry is dropped when the server's diagnostics for the file change and while an
  editor shows the file, so it's read again once the editor closes (the file may have been saved).
- A stored row opens with `Pane::open_at` and no reveal; an observer on the pane selects the range,
  decoded against the editor's text, once the file has landed, unless the pane opened something else
  first.
