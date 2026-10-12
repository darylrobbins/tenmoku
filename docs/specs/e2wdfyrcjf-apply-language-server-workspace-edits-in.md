---
task_uid: "01m4rbj9nyav8qrwe2wdfyrcjf"
task: "T83"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test workspace_edit::tests::open_and_closed_files: edits to an open buffer and to a closed UTF-8 and a closed windows-1252 CRLF file apply: the buffer as one user transaction that cmd-z undoes, the closed files rewritten in their own encoding and line endings"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test workspace_edit::tests::all_or_nothing: a buffer edited past its version, or a closed file that has since been opened, refuses the whole edit and changes no buffer or file"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test workspace_edit::tests::server_apply_edit: a fake server's workspace/applyEdit is applied and answered applied; a refused one is answered failed with the reason"
    required: true
    manual: false
  - id: "C4"
    text: "the command registry lists lsp.rename, lsp.code_actions, lsp.format_document and lsp.format_selection in the editor context, and cargo test -p fe-workbench and cargo clippy -p fe-workbench --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Apply language-server workspace edits in the workbench, all or nothing, answering workspace/applyEdit, and stub the rename, code-action and format commands (Mac T472)

## Problem

The user (2026-10-12) asked for LSP rename, code actions and formatting in the app. Rename and code actions change
many files at once, some of them not open, and servers push edits through `workspace/applyEdit` while running a
code action's command. T465 (fe-lsp) decodes those into `fe_lsp::workspace_edit::FileEdits` and raises
`LspEvent::ApplyEdit`, but the workbench has no way to apply them. And each feature's commands would be registered
in `crates/fe-workbench/src/lsp.rs` (`registry()`, :1382, `LspOp`) and its pickers added to `Palette`
(`crates/fe-workbench/src/workbench.rs` :96), so the three UI cards would collide on the same lines.

## Change

1. **Applying edits**: new module `crates/fe-workbench/src/workspace_edit.rs` with
   `Workbench::apply_file_edits(&mut self, edits: Vec<FileEdits>, cx) -> Result<AppliedEdits, String>`, all or
   nothing as far as it can be:
   - Every target is checked first. An `Open` target's buffer must still be at its version; a `Closed` target's
     file must not have been opened since and must read cleanly (read the way the editor loads files, with its
     encoding detection, then `decode_closed`). Any failure refuses the whole edit, changing nothing, and names the
     file.
   - Then each open buffer gets its edits as one transaction of the user (the user asked for the rename or the
     action, as with accepting a completion), so cmd-z in that editor undoes it; selections stay anchored. Then each
     closed file is written back in its own encoding, BOM and line endings, atomically (temp file and rename, as
     saving does). A write failure stops there and the error names the files already written.
   - `AppliedEdits { files: usize, edits: usize }` for the UI's message.
   - `LspEvent::ApplyEdit` (handled where lsp.rs handles the other events) runs `apply_file_edits` on the main
     thread and answers its `ApplyReply`: applied, or failed with the error.
2. **Stubs**, so the feature cards touch only their own modules: lsp.rs's `registry()` gains `lsp.rename`
   ("Rename Symbol…"), `lsp.code_actions` ("Code Actions…"), `lsp.format_document` ("Format Document") and
   `lsp.format_selection` ("Format Selection"), in the editor context, each dispatching through a new `LspOp`
   variant to a `Workbench` method in its own new module: `lsp_rename.rs` (`rename_symbol`),
   `lsp_code_actions.rs` (`show_code_actions`) and `lsp_format.rs` (`format_active(selection: bool)`). Here each
   method only logs at debug level that it isn't implemented yet. `Palette` gains `Rename` and `CodeActions`
   variants (ids `rename`, `code_actions`), unused until their cards.

## Tests

New file `crates/fe-workbench/src/workspace_edit_tests.rs`, declared from workspace_edit.rs with `#[path]`, using
the workbench test helpers and a temp project.

## Implementation notes

- `FileEdits` and `EditTarget` are `#[non_exhaustive]`, so the workbench's tests can't build them: they get them from
  the workbench's manager (`Workbench::lsp_decode_edit`, test only, over `LspManager::decode_workspace_edit`) or as
  the fake's `workspace/applyEdit`, as a rename's answer or a server's push would arrive.
- A closed file is checked by reading it (`loader::read`), decoding and applying its edits to that buffer and
  encoding the result to a sink, so a file with bytes its encoding can't read, or an edit its encoding can't write
  (an emoji in windows-1252), refuses the whole edit before anything changes. It's then written with
  `EditorModel::save`, atomically and refusing a file changed since the check.
- An open target whose buffer no editor shows any more is refused ("no longer open").
- `run_lsp_op` dispatches each op to its own method; restart and log moved to `run_server_op`.
- Review round 1: a closed target is resolved (`lsp::resolve`, as the files the workbench opens on servers are)
  and compared with every editor's file and every file a pane is reading, resolved, so a symlink to an open file
  or a file still being read into a tab is refused, as is one file named twice by two paths. The unedited buffer
  is encoded first, so a file that didn't decode cleanly is refused even when the edit would remove its U+FFFD.
  An `ApplyEdit` whose reply is already answered (timed out, or its session ended) is dropped, and checked again
  between the check and the changes (`apply_file_edits_while`). `Pane::reading` (pane.rs, outside the declared
  scope) lists the reads in flight for this.
- Review round 2, extending the scope to fe-lsp (`workspace_edit.rs`, `server.rs`, `lib.rs` and its tests) and to
  the LSP tests' harness (`tests/lsp.rs`, `with_scripts` made `pub(crate)`):
  - `ApplyReply::claim` takes the answer over before anything changes: the timeout then waits for the claim's
    answer instead of answering `applied: false` itself, so the server never hears "not applied" for an edit
    that was. The workbench claims each `ApplyEdit` first, and drops one it can't claim. `apply_file_edits_while`
    is gone.
  - The server actor keeps the replies it hands out (weakly, so the host dropping them still answers them) and
    answers them failed when the session ends, crashed or stopped, before a graceful shutdown waits on the server.
  - The host's read of the launch file (`Workbench::launch_reading`) counts as opened until it lands.
  - Open buffers are edited through `ProposedChange::accept_all`, which sets an open transaction aside, so a
    user's open one (a Vim insert) is neither joined nor closed; inserts at one place are joined in order first,
    as it takes no two at one place.
