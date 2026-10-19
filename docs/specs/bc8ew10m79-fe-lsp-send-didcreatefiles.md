---
task_uid: "01m55v3dmk1ds9mmbc8ew10m79"
task: "T184"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test tests::sync::file_operations::did_notifications_follow_server_filters passes: didRenameFiles carries a.rs -\u003e b.rs only, didCreateFiles the folder only, and no didDeleteFiles without a filter; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit tests tests::sync::file_operations::will_rename_returns_decoded_edits_or_times_out and tests::sync::file_operations::file_operation_capabilities_are_advertised pass"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: send didCreateFiles, didRenameFiles and didDeleteFiles to servers whose file-operation filters match, and ask willRenameFiles for a rename's edits (Mac T950)

Filed on argon from Mac T950 (card ~/coordination/argon-cards/zzr/T950.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55ty40pfcrw0n7znf49pf07. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T181 (Mac T941). Held in the inbox: waits until Mac T918, Mac T850 is on argon's trunk through a mac/main sync. Title the host commit "... (Mac T950)".

## Problem

LSP completeness track, item 8 (user-approved 2026-10-17; `~/.local/share/fastedit-lead/lsp-gap-audit.md`: "File
operations ✗: renaming in the tree updates no imports"). LSP 3.16 file operations let a server react to files the
client creates, renames and deletes: `workspace/willRenameFiles` (a request whose answer is a workspace edit
applied before the rename, so typescript-language-server and rust-analyzer fix imports and `mod` paths) and the
`workspace/didCreateFiles`, `didRenameFiles` and `didDeleteFiles` notifications. Each server says which files it
cares about in `workspace.fileOperations.<op>.filters` (scheme, glob, file or folder, ignoreCase). VS Code sends
them for its explorer's operations. fe-lsp advertises no `workspace.fileOperations` and has no way to send them.

Once the card this depends on lands, fe-lsp has `LspGlob` and `LspManager::notify_sessions`.

## Change

1. New `crates/fe-lsp/src/file_operations.rs`:
   - Filters: a server's `FileOperationRegistrationOptions.filters`, each a scheme (only `file` matches),
     a glob built with `LspGlob` (case-folded when `ignoreCase`) and a `matches` of `file` or `folder` (unset:
     both); a path matches if any filter does, the kind read from the filesystem by the caller.
   - `LspManager::did_create_files(paths: Vec<(PathBuf, bool /* folder */)>)`,
     `did_delete_files(..)` and `did_rename_files(moves: Vec<(PathBuf, PathBuf, bool)>)`: through
     `notify_sessions`, each server whose `fileOperations.didCreate`/`didDelete`/`didRename` filters match at
     least one gets one notification with the matching ones (a rename matches by its old path), as `file:`
     URIs; others get nothing.
   - `LspManager::will_rename_files(moves) -> impl Future<Output = Vec<FileEdits>>`: asks each server whose
     `willRename` filters match, concurrently, and gives their workspace edits decoded as
     `workspace/applyEdit` edits are (`crate::workspace_edit`, so a resource operation in one is refused as
     there), in server order; a server's error, null answer or timeout gives nothing for it (VS Code renames
     anyway when a participant fails).
2. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `workspace.fileOperations = {dynamicRegistration:
   false, didCreate: true, willRename: true, didRename: true, didDelete: true}` (no willCreate, no willDelete:
   their edits would need the same flow for operations fastedit can't hold).
3. `crates/fe-lsp/src/requests.rs` `timeout`: `workspace/willRenameFiles` → `timeouts.references` (the long
   one, as VS Code's participants wait up to `files.participants.timeout`, 60 s).
4. `crates/fe-lsp/src/lib.rs`: `mod file_operations`.
5. Tests: new `crates/fe-lsp/src/tests/sync/file_operations.rs`, declared by `mod file_operations;` in
   `crates/fe-lsp/src/tests/sync.rs` (its only change):
   - `did_notifications_follow_server_filters`: a fake with `didRename` filter `**/*.rs` (files) and
     `didCreate` filter `**` (folders): renaming `a.rs` → `b.rs` and `x.txt` → `y.txt` sends one
     `didRenameFiles` with the first only; creating folder `m` and file `n.rs` sends `didCreateFiles` with `m`
     only; `did_delete_files` sends nothing (no filter).
   - `will_rename_returns_decoded_edits_or_times_out`: a `willRename` answer editing an open file and a closed
     one gives `FileEdits` as an `applyEdit` of the same edit would; a delayed answer past the timeout gives
     nothing and is cancelled.
   - `file_operation_capabilities_are_advertised`.

Linux-OK.
