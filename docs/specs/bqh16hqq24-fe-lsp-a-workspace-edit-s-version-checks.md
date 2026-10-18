---
task_uid: "01m55zd0tjwge757bqh16hqq24"
task: "T189"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-lsp unit test workspace_edit::tests::ops::a_version_on_a_fresh_path_is_refused passes: a create then an edit with a non-null version is refused with a reason naming the path, the same after a rename's new path, and a null version after a create still decodes as Closed against empty text; it fails at the base"
    required: true
    manual: false
  - id: "C2"
    text: "fe-lsp unit test workspace_edit::tests::ops::directory_operations_cover_the_paths_under_them passes: an edit under a directory deleted earlier in the edit is refused; after renaming dir a to b, an edit under a/ is refused and an edit under b/ is Closed even with a buffer open at that path; a create under a deleted directory makes that path fresh again; /a/bc is not under /a/b; it fails at the base"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-lsp -p fe-workbench passes with no existing test edited; cargo clippy -p fe-lsp --all-targets -- -D warnings is clean; no Cargo changes"
    required: true
    manual: false
---

# fe-lsp: a workspace edit's version checks still apply to paths an earlier create or rename made fresh, and delete and rename of a directory cover the paths under it (T187 review F1, F2)

Argon follow-up to T187 (Mac T953) review 0rsqvy, findings F1 and F2 (medium). Both are latent today, because the workbench refuses every resource operation. They become live when T188 (Mac T954) applies and advertises create, rename and delete, so T188 waits on this card.

## Problem

`crates/fe-lsp/src/workspace_edit.rs` decodes a workspace edit's resource operations in order (T187).

1. **F1: fresh paths skip version checks** (around lines 391 and 413). A `TextDocumentEdit` on a path that an earlier create or rename in the same edit made fresh is decoded as `EditTarget::Closed` against empty text. Its `version` is never checked, so a create or rename followed by an edit naming any non-null version (unknown or stale) succeeds. That breaks the decoder's documented refusal contract.
2. **F2: operations track exact paths only** (around lines 236 and 247). After a recursive delete of a directory, an edit to a file under it is accepted, though that file was deleted earlier in the edit. A rename of a directory neither refuses later edits under its old path nor marks files under its new path as `Closed`. So an edit can decode against an unrelated open buffer at the destination.

## Change

- A path made fresh by an earlier create or rename keeps its `Closed` target, against empty text for a create. A non-null `version` on its `TextDocumentEdit` is refused with a reason that names the path, because the server cannot know a version for a file the edit itself makes. A null version is accepted as today.
- Track operation effects by path components, in operation order:
  - A delete of a directory (recursive or not) removes every path under it, so a later edit under it is refused as an edit of a deleted file.
  - A rename of a directory moves every path under it: a later edit under the old path is refused as deleted, and a path under the new name is fresh (`Closed`), even if a buffer is open there.
  - A later create under a deleted directory makes that path fresh again, as a file create does today.
- Paths are compared by components after the decoder's existing URI-to-path step, not by string prefix (`/a/b` is not under `/a/bc`).
- Update the doc comments on the decoder.

New tests go in the existing `crates/fe-lsp/src/workspace_edit_ops_tests.rs` (module `workspace_edit::tests::ops`). Existing tests stay unchanged.
