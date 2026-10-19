---
task_uid: "01m55v3e8s9cc41kxt1rxtq21g"
task: "T186"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench unit test tests::project_panel::file_operations::will_rename_edits_land_before_the_rename passes: the open main.rs gets mod helpers; as one undo step, the closed lib.rs on disk gets its edit, src/helpers.rs exists, and willRenameFiles came before didRenameFiles; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench unit test tests::project_panel::file_operations::a_slow_participant_can_be_skipped passes: the field shows Renaming… while held; escape renames at once and the late answer is never applied; a server error renames without edits"
    required: true
    manual: false
  - id: "C3"
    text: "With no server whose willRename filters match, a rename is synchronous as at the base: the existing project panel tests pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# Apply language servers' willRenameFiles edits before a rename in the project tree, so imports and mod paths follow, renaming anyway if a server fails or is skipped (Mac T952)

Filed on argon from Mac T952 (card ~/coordination/argon-cards/zzr/T952.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55ty5n27x118teq7mzz2bxk. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Argon deps: T185 (Mac T951). Title the host commit "... (Mac T952)".

## Problem

LSP completeness track, item 8 (user-approved 2026-10-17: "renaming in the tree updates no imports"). Once the
cards this depends on land, fe-lsp asks servers for the edits a rename needs (`LspManager::will_rename_files`,
decoded `FileEdits`), and the tree sends the did-notifications. A rename in the tree still updates no imports:
`ProjectPanel::rename` (`crates/fe-workbench/src/project_panel_ops.rs:333`) checks `busy_under`, plans the editor
moves (`Workbench::plan_rename`) and renames, all synchronously on the main thread. VS Code, before renaming in
its explorer, asks every `willRename` participant (up to `files.participants.timeout`, 60 s, with a cancellable
"Running 'File Rename' participants..." progress), applies the edits they return (typescript-language-server's
import paths, rust-analyzer's `mod` declarations), then renames; a failing or timed-out participant doesn't stop
the rename.

## Change

1. `crates/fe-workbench/src/project_panel_ops.rs` `rename`: when a manager has a server whose `willRename`
   filters could match (fe-lsp answers quickly with nothing otherwise, and then the rename is synchronous as
   today), the rename becomes asynchronous:
   - the name field stays open showing "Renaming…" and refuses another commit; `escape` cancels the wait
     (dropping the request) and renames at once, as VS Code's cancel does;
   - when the edits land, they are applied through `Workbench::apply_file_edits` (as code actions apply
     theirs: open files edited in their editors as one undo step each, closed files written), then
     `busy_under` is checked again (it may have changed while waiting: refuse as today if busy, the edits
     staying applied, as VS Code leaves them), then `plan_rename`, `fs_ops::rename` and `Renamed` as today;
   - an edit that can't be applied is logged and the rename goes on.
2. `crates/fe-workbench/src/lsp_file_operations.rs`: `Workbench::will_rename(from, to) -> Task<Vec<FileEdits>>`,
   paths resolved as the did-notifications resolve them.
3. Tests, in `crates/fe-workbench/src/tests/lsp_file_operations.rs`:
   - `will_rename_edits_land_before_the_rename`: the fake's `willRename` filter `**/*.rs` answers renaming
     `src/util.rs` to `src/helpers.rs` with an edit of the open `src/main.rs` (`mod util;` → `mod helpers;`)
     and of the closed `src/lib.rs`; after the rename, `main.rs`'s editor has the edit as one undo step,
     `lib.rs` on disk has its edit, `src/helpers.rs` exists, and the fake got `willRenameFiles` before
     `didRenameFiles`.
   - `a_slow_participant_can_be_skipped`: with the answer held, the field shows "Renaming…"; `escape`
     renames at once and the late answer is not applied; a server error renames without edits.

Linux-OK.

## Implementation notes

- Whether the rename waits is decided by polling `LspManager::will_rename_files`' future once:
  with no matching server it's ready at once (`join_all` of nothing), `will_rename` returns
  `Task::ready`, and the panel, polling that task once too, renames synchronously as before. fe-lsp
  is unchanged.
- The wait lives in a detached task, not the field: a field closed by losing focus, by
  `set_show_ignored` or by its row going keeps waiting and renames when the answer lands (refusals
  and failures are then logged). Escape signals the task over a channel the task polls before the
  answer, so an answer that arrives in the same turn as escape is never applied, and the task drops
  the requests ($/cancelRequest).
- `will_rename` and `lsp_files_renamed` resolve both paths by their folder, keeping the names as
  given (carried T185 F1): after a case-only rename on a case-insensitive disk the old path no longer
  canonicalizes to the new one. Linux disks are case-sensitive, so
  `a_renamed_path_keeps_its_spelling` stands the old spelling in with a symlink to the new name.
- Carried T185 F2: create (`edit_done`), duplicate and trash map their paths by the root the
  operation ran under and announce `Created`/`Deleted` whether or not the tree is open;
  `servers_hear_of_operations_done_while_the_tree_reopens` toggles show-ignored behind four
  duplicates (it times out against the old mapping).
- Review v9nvd6 F1: a rename that waited checks its entry's device and inode against those taken
  before asking; moved or replaced meanwhile, neither the edits nor the rename are made, and the field
  says so (`a_file_moved_while_participants_answer_isnt_renamed`, removal and replacement, answered
  and escaped).
- Review v9nvd6 F2: `will_rename` joins each file's text edits from every server into one entry
  (`apply_file_edits` refuses a closed file named twice): sorted, an identical edit once; a later
  server's edits to that file that overlap, or are for another buffer version or encoding, are
  dropped and logged, the rest made (`participants_edits_to_one_file_are_joined`).
