---
task_uid: "01m55v3er61bp0ep72w97tjxba"
task: "T187"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test workspace_edit::tests::ops::resource_operations_decode_in_order passes: the spec's six-step edit decodes to six entries in order with their options, the created and renamed files' edits Closed; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test workspace_edit::tests::ops::edits_after_a_delete_refuse_the_edit passes; refuses_bad_edits loses only its create and rename cases (the one existing test edited); capabilities_are_declared still pins resourceOperations []"
    required: true
    manual: false
  - id: "C3"
    text: "An applyEdit with an operation is answered not applied by the workbench with the operation's reason; edits without operations decode exactly as at the base"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no existing test edited beyond what the spec names; cargo clippy -p fe-lsp -p fe-workbench --all-targets -- -D warnings is clean; no Cargo change, and git diff against the base touches no file outside the scope except this lane's docs/specs/\u003ckey\u003e-*.md"
    required: true
    manual: false
---

# fe-lsp: decode a workspace edit's create, rename and delete operations in order, still refused by the workbench and not yet advertised (Mac T953)

Filed on argon from Mac T953 (card ~/coordination/argon-cards/zzr/T953.json rev 1; note zzr, the LSP completeness track part 2). Mac task uid 01m55ty680enqrq1n3dhymq6nr. Base: argon trunk 005ee25 (= mac/main b11e4ef synced). Painted-state tests, no goldens. fe-lsp server.rs/lib.rs/requests.rs and fe-workbench lsp.rs are in most LSP cards' scopes on every host, so keep changes there small. Title the host commit "... (Mac T953)".

## Problem

LSP completeness track, item 8 (user-approved 2026-10-17: "resource operations in workspace edits (server.rs:2022
refuses them)"). A workspace edit may create, rename or delete files between its text edits
(`documentChanges` with `CreateFile`, `RenameFile`, `DeleteFile`, in order): rust-analyzer's rename of a module
moves `foo.rs` to `bar.rs`, its "move module to file" assist creates one, and typescript-language-server's "Move
to a new file" refactor creates one. fe-lsp advertises `resourceOperations: []` with `failureHandling: abort`
(`crates/fe-lsp/src/server.rs:2020-2024`, pinned at `crates/fe-lsp/src/workspace_edit_tests.rs:265-277`) and
`decode` refuses any operation outright (`crates/fe-lsp/src/workspace_edit.rs:224`, `refused_operation`
`:309`). Its output is a flat `Vec<FileEdits>`, merged per file and sorted, so an order like "create `b.rs`,
then write into it" can't be expressed.

This card decodes operations, in order, without advertising them: the workbench refuses the new targets by
name until the next card applies them, so no server sends one because of this card.

## Change

1. `crates/fe-lsp/src/workspace_edit.rs`:
   - `EditTarget` (`#[non_exhaustive]`) gains `Create { overwrite: bool, ignore_if_exists: bool }`,
     `Rename { to: PathBuf, overwrite: bool, ignore_if_exists: bool }` and `Delete { recursive: bool,
     ignore_if_not_exists: bool }`, each a `FileEdits` entry for its path (`Rename`'s path is the old one).
   - `decode` keeps `documentChanges`' order when it has operations: text edits to a file are merged only
     between operations, so the output lists, in order, each run of text edits and each operation. Without
     operations the output is exactly as today (merged per file, sorted).
   - Text edits to a file created earlier in the same edit decode as `Closed` (against empty text, by
     `decode_closed`); to a file renamed earlier, under its new path, as a closed file. A text edit to a path
     deleted earlier in the edit, or an operation on a non-`file:` URI, refuses the whole edit with a reason.
   - Module docs: the operations, and that they are decoded but not yet advertised.
2. `crates/fe-workbench/src/workspace_edit.rs` `check_file_edits` (`:137-200`): the three new targets are refused
   with their own reasons ("fastedit can't create files from an edit yet", and so on) instead of the catch-all;
   nothing else changes. So a server that sends an operation unadvertised is still refused, now by the
   workbench, with the reason in the answer as before.
3. Tests: new `crates/fe-lsp/src/workspace_edit_ops_tests.rs`, declared at the end of
   `crates/fe-lsp/src/workspace_edit_tests.rs` as `#[path = "workspace_edit_ops_tests.rs"] mod ops;`:
   - `resource_operations_decode_in_order`: an edit of [text edit to open `a.rs`, create `b.rs`, text edit to
     `b.rs`, rename `c.rs` → `d.rs`, text edit to `d.rs`, delete `e.rs` (recursive)] decodes to six entries in
     that order with the given options, `b.rs`'s and `d.rs`'s edits `Closed`.
   - `edits_after_a_delete_refuse_the_edit`: deleting `e.rs` then editing it refuses the edit, naming it.
   - `crates/fe-lsp/src/workspace_edit_tests.rs` `refuses_bad_edits`: its create and rename cases (about
     `:463-473`, "creates", "renames") are no longer refused by fe-lsp; drop them there (the new file decodes
     them, and the workbench refuses them). The one existing test this card edits; `capabilities_are_declared`
     and its pinned `resourceOperations: []` are unchanged.

Linux-OK.
