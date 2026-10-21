---
task_uid: "01m55v3f2a2s8frgc0vq51n8px"
task: "T188"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench unit test workspace_edit::tests::ops::renames_creates_and_deletes_apply_in_order passes: a.rs's editor follows to b.rs with the edit, c.rs exists with its text, d.rs is trashed, and the edit is answered applied; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test workspace_edit::tests::ops::a_failing_operation_aborts_the_rest passes"
    required: true
    manual: false
  - id: "C3"
    text: "fe-lsp unit test workspace_edit::tests::capabilities_are_declared passes with resourceOperations [create, rename, delete] and failureHandling abort (the one existing test this card edits)"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Apply workspace edits that create, rename and delete files, in order with their text edits, and advertise those resource operations, as VS Code does (Mac T954)

Filed on argon from Mac T954 (card ~/coordination/argon-cards/zzr/T954.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55ty6px62fmc2zxsnvve3m3. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T187 (Mac T953), T185 (Mac T951). Title the host commit "... (Mac T954)".

## Problem

LSP completeness track, item 8 (user-approved 2026-10-17). Once the card this depends on lands, fe-lsp decodes a
workspace edit's create, rename and delete operations, in order, as `EditTarget::Create`, `Rename` and `Delete`,
and the workbench refuses them by name (`check_file_edits`, `crates/fe-workbench/src/workspace_edit.rs:137`).
fe-lsp still advertises `resourceOperations: []`, so rust-analyzer can't rename a module's file and refuses
such renames, and "move to file" refactors are missing. VS Code advertises `["create", "rename", "delete"]` and
applies them in order with the text edits.

## Change

1. `crates/fe-workbench/src/workspace_edit.rs` `apply_file_edits` / `check_file_edits`: apply entries in order:
   - **Create**: through the tree's file operations (`fs_ops`), an empty file; an existing file is an error
     unless `overwrite` (truncated) or `ignore_if_exists` (skipped).
   - **Rename**: the file or folder moved as the tree's rename moves it, open editors following
     (`Workbench::plan_rename` before, then the move, then `follow_moves` / the `Renamed` handling the tree's
     rename gets), refused while `busy_under` the path as the tree refuses; an existing target is an error
     unless `overwrite` or `ignore_if_exists`.
   - **Delete**: to the Trash, as the tree deletes; open editors of it close as when the tree trashes it; a
     missing path is an error unless `ignore_if_not_exists`; a folder needs `recursive`.
   - Text edits as today, a created file's against its (empty) text.
   - **Abort**: the first failing entry stops the rest; what was applied stays applied (LSP's `abort`), and the
     reason is reported as today's refusals are. The checks done today before applying anything still run
     first for text edits.
   - The tree's file operations now also send their did-notifications (the did-notify card), so a created,
     renamed or deleted file reaches the servers.
2. `crates/fe-lsp/src/server.rs` `client_capabilities()`: `workspaceEdit.resourceOperations = ["create",
   "rename", "delete"]`, `failureHandling` staying `abort`.
3. Tests:
   - `crates/fe-lsp/src/workspace_edit_tests.rs` `capabilities_are_declared`: the pinned `resourceOperations`
     (`:270-277`) becomes the three; the one existing test this card edits.
   - New `crates/fe-workbench/src/workspace_edit_ops_tests.rs`, declared at the end of
     `crates/fe-workbench/src/workspace_edit_tests.rs` as `#[path = "workspace_edit_ops_tests.rs"] mod ops;`, with
     the fake of `tests/lsp.rs` pushing `workspace/applyEdit`s:
     - `renames_creates_and_deletes_apply_in_order`: an edit renaming the open `src/a.rs` to `src/b.rs` then
       editing `src/b.rs`, creating `src/c.rs` with text, and deleting `src/d.rs`: the editor of `a.rs` now
       shows `src/b.rs` with the edit, `src/c.rs` exists with its text, `src/d.rs` is gone (trashed), and the
       answer is applied.
     - `a_failing_operation_aborts_the_rest`: creating an existing file (no options) then editing another
       answers not applied with the reason, and the later edit isn't applied.

Linux-OK (trash through the same `fs_ops` the tree's tests use on Linux).

## Implementation notes

- **Scope.** `apply_file_edits` runs inside the workbench's own update and has no `Window` (the
  applyEdit loop updates the entity, not the window). `Workbench::busy_under` and `plan_rename`
  take a window and read its root workbench, which panics while that workbench is being updated,
  and `moves`, `moved_to` and `DetachedSaves` are private to their module. So there are window-less
  twins: `busy_under_here`, `plan_rename_here`, `follow_moves_everywhere` and `and_others`, over
  this workbench as `self` and `other_workbenches`. `busy_under`'s body moved into `busy_among` so
  both share it. `other_workbenches` finds each other window's root through `update_window`,
  because a window that's being updated panics on any read. That's this one's own window when a
  picker in it applies the edit, and it's passed over (`update_window` fails for it). The others
  are then read and updated as entities. Rev 3 added `workbench.rs` for these. T123 (Mac T706)
  has since split `workbench.rs` and moved them to `crates/fe-workbench/src/workbench_files.rs`
  (its `workbench_files` submodule), so after the rebase onto 42fda8f the twins live there, next
  to the functions they mirror, and `workbench.rs` is unchanged.
- **Order and checks.** Entries apply in order. Each run of text edits between operations is
  checked as a whole before any of it applies, which is today's check when there are no
  operations. The first failing entry stops the rest, and earlier ones stay applied (`abort`).
  Every check of what editors show looks at every window's workbench (shown, being read, held by
  a Save As), the closed-file check included.
- **Later runs of one buffer.** fe-lsp decodes every text edit to an open file against the
  server's text at its version. When an earlier run of the same workspace edit changed the buffer,
  a later run's edits for that version are moved past the earlier edits (`rebase`), as long as the
  buffer is still where this edit left it. If they overlap the earlier edits, or meet an insert
  inside them, they're refused. The earlier edits are recorded as the `ProposedChange` made them,
  from its hunks: clipped, CRLF normalized to LF, and no-op replacements dropped.
- **Every window.** An open target's editor is looked up in every window's workbench, and its
  other editors in every window are notified.
- **Edits after an operation (fe-lsp).** A text edit to a path an earlier operation made is decoded
  against the text the server had for it (`Reader::origin`):
  - after a rename, the source file's text, as it was then, through folder renames too;
  - after a create or rename that `ignoreIfExists` may skip (without `overwrite`), the path's own
    text from before the operation;
  - after a plain or overwriting create, empty text.

  If the server has that file open, the edit is `EditTarget::Open` for its buffer and version, so
  the host's usual version check refuses a stale one. Otherwise it's `Closed`.

  Whether a create or rename with `ignoreIfExists` (without `overwrite`) is made depends on the
  disk, so fe-lsp decodes the edits that depend on it both ways, as `EditTarget::Depends { on,
  made, skipped }`. `on` is the operation itself, and the host takes the latest equal operation
  before the entry, which stays correct when willRename answers are joined. The skipped branch
  reads the path's own text, and the rename's source (and the files under it) are still there.
  Each branch is a target, or why the edits can't apply that way, and identical branches
  collapse. The workbench records each operation as made or skipped, and resolves each `Depends`
  before checking its run.

  Branches meet again, so what paths hold is a graph, not a tree (review round 3, F11). What a
  path holds after a number of effects is worked out once, memoized, and interned as a node, so
  equal nodes are one. Its parts come before it, and it's computed with an explicit stack, not
  recursion. Each file decodes only the nodes it can reach, in order, once each. Leaves are shared
  by what decodes them (closed, a document, a refusal), and a `Depends` by its operation and
  parts, with `Arc` branches. So 2,000 alternating skippable renames decode in a fraction of a
  second, where the tree doubled with each. The host's `resolve_depends` walks one branch per
  operation, as a loop. Cost: each path state scans the effects back to the latest that covers it.
- **Follow-ups (round 3, at the lead's request):** T210 (F10) gives each operation occurrence a
  stable identity. `Depends` names its operation by value, so two identical skippable operations
  in one edit are confused. T211 (F9) truncates through a symlink-safe write handle in fe-project,
  so an overwrite keeps owner, ACLs and xattrs. Today only the permission bits carry over. So an editor that
  follows a rename gets the edits through the Open path. A closed edit to a file an editor shows
  is still refused, because it was computed against disk text the server doesn't have. A version on
  an edit to a fresh path is still refused.
- **Create** goes through `fs_ops::create_file` under the canonical root, making the folders
  above the file. With `overwrite`, which wins over `ignoreIfExists` as LSP says, the existing
  file goes to the Trash (`fs_ops::trash`) and is created again. Both steps work through the tree's
  symlink-safe directory handles, so nothing is truncated through a path, and the old content
  stays recoverable. A regular file's permission bits (`0o777`) are set on the new one with
  `fchmod`, through `fe_project::open_file_under`'s handle, before any text is written to it.
  Owner, extended attributes and setuid bits aren't carried over; the old file in the Trash keeps
  them. fe-project has no symlink-safe write open, so true truncation through a handle isn't
  available. A file any window shows, is reading or holds for a Save As is refused.
- **Rename** goes through `fs_ops::rename`, which renames within a folder only, so a move to
  another folder is refused by name. A destination any window shows (or a file under it) is
  refused whether or not it's on disk. The only exception is the entry itself under another case.
  `overwrite` trashes an existing target first. Editors follow in every window.
- **Delete** trashes through `fs_ops::trash`. Editors of it stay open with their buffers, as the
  tree's delete leaves them (the spec's "close" contradicts "as when the tree trashes it"; the
  tree's behaviour wins).
- Every operation is refused outside the project root, on the root itself, and while
  `busy_under` its path (and a rename's destination). Servers hear `didCreateFiles`,
  `didRenameFiles` and `didDeleteFiles` through the did-notify card's `lsp_files_*`.
- The ops tests trash files from a project under the system temp folder, then remove them from
  the Trash by their `.trashinfo`, in the home trash or the one at the top of the mount. Racing a
  folder swapped for a symlink is `fs_ops`' own guarantee, tested in fe-project; this card adds no
  path-based file access of its own.
