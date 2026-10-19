---
task_uid: "01m55v3e3jtgrxqhdd32c0rvq2"
task: "T185"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-workbench unit test tests::project_panel::file_operations::tree_operations_notify_servers passes: creating, renaming and trashing src/new.rs send didCreateFiles, didRenameFiles and didDeleteFiles; notes.txt and a refused rename send nothing; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Tell language servers about files created, duplicated, renamed and trashed in the project tree (didCreateFiles, didRenameFiles, didDeleteFiles), as VS Code's explorer does (Mac T951)

Filed on argon from Mac T951 (card ~/coordination/argon-cards/zzr/T951.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55ty518xfs7vfe3vjx1220t. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T184 (Mac T950). Title the host commit "... (Mac T951)".

## Problem

LSP completeness track, item 8 (user-approved 2026-10-17). Once the card this depends on lands, fe-lsp sends
`workspace/didCreateFiles`, `didRenameFiles` and `didDeleteFiles` to the servers whose filters match
(`LspManager::did_create_files`, `did_rename_files`, `did_delete_files`). The project tree's operations don't
call them. In `crates/fe-workbench/src/project_panel_ops.rs`: `commit_edit` (`:267`) creates a file or folder in
the background and then calls `edit_done` (`:387`); `rename` (`:333`) renames on the main thread and emits
`PanelEvent::Renamed` (`project_panel.rs:66-80`, handled by `Workbench::panel_renamed`,
`crates/fe-workbench/src/workbench.rs:1341`); `delete_selected` (`:468`) trashes in the background; and
`duplicate_selected` (`:514`) creates a copy. There are no created or deleted panel events. VS Code sends the
did-notifications after its explorer's create, rename, delete (to trash) and copy succeed.

## Change

1. `crates/fe-workbench/src/project_panel.rs` `PanelEvent`: new `Created(Vec<(PathBuf, bool /* folder */)>)` and
   `Deleted(Vec<(PathBuf, bool)>)`, with docs.
2. `crates/fe-workbench/src/project_panel_ops.rs`: after a create (new file, new folder) or duplicate
   succeeds, emit `Created` with the new path; after a trash succeeds, `Deleted` with each trashed path. A
   failed operation emits nothing. `Renamed` is unchanged (it carries `from` and `to`).
3. New `crates/fe-workbench/src/lsp_file_operations.rs`, declared from `crates/fe-workbench/src/lsp.rs` as
   `#[path = "lsp_file_operations.rs"] mod file_operations;`: with a manager, `Created`, `Deleted` and
   `Renamed` call the matching fe-lsp method, paths resolved as `lsp::resolve` resolves them (a deleted path
   through its parent), kinds as the event gives them.
4. `crates/fe-workbench/src/workbench.rs` `follow_root`: the two new arms, and one line in the `Renamed` arm.
5. Tests: new `crates/fe-workbench/src/tests/lsp_file_operations.rs`, declared at the end of
   `crates/fe-workbench/src/tests/project_panel.rs` as `#[path = "lsp_file_operations.rs"] mod
   file_operations;`, with the fake of `tests/lsp.rs` advertising `didCreate`, `didRename` and `didDelete`
   filters `**/*.rs`, driving the tree as that file's tests do:
   - `tree_operations_notify_servers`: creating `src/new.rs` sends `didCreateFiles` with it; renaming it to
     `src/renamed.rs` sends `didRenameFiles` with the old and new URIs; trashing it sends `didDeleteFiles`;
     creating `notes.txt` sends nothing; a rename refused because the file is being saved sends nothing.

Linux-OK.
