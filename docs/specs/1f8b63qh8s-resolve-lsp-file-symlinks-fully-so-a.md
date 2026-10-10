---
task_uid: "01m4f4ndsb9rn5v91f8b63qh8s"
task: "T235"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::lsp_symlinks::a_file_symlink_out_of_the_root_is_not_sent: opening a file symlink under the root that points outside it sends no didOpen for either path, and the outside file's text reaches no server"
    required: true
    manual: false
  - id: "C2"
    text: "tests::lsp_symlinks::a_file_symlink_into_the_root_opens_its_target: opening, by an absolute path, a symlink outside the root that points at an existing file under it sends didOpen for the target's URI"
    required: true
    manual: false
  - id: "C3"
    text: "A file that doesn't exist yet, in an existing folder, still resolves by its folder: the existing tests in crates/fe-workbench/src/tests/lsp.rs pass unchanged"
    required: true
    manual: false
---

# Resolve LSP file symlinks fully, so a file is under the root only if its target is

## Problem

Review of T143 (review 01m4ez2b7wstp7r7m7ak8bej2k, F2, medium), filed in T233:

> The resolver leaves final-component file symlinks unresolved. A project/main.rs symlink pointing outside the root passes containment and sends the outside file's contents to the server; an outside symlink pointing into the root is incorrectly excluded. Resolving only the folder does not establish the documented containment guarantee.

`resolve` in `crates/fe-workbench/src/lsp.rs` canonicalizes only `path.parent()` and re-joins the file name, so a symlinked *file* is never followed. `Workbench::sync_lsp` then tests `path.starts_with(root)` on that half-resolved path. The module docs promise "a file is under the root only if it really is".

## Change

- `resolve(path)`: if `fs::symlink_metadata(path)` succeeds (the file exists, or is a symlink, dangling or not), return `fs::canonicalize(path).ok()`, so a dangling symlink resolves to `None`. Only when `symlink_metadata` fails with `NotFound` keep today's fallback: the canonical parent joined with the file name (a new file not yet saved). Any other error is `None`.
- Update `resolve`'s doc comment and the **Paths** bullet of the module docs to say files are resolved fully, and only a file that doesn't exist yet is resolved by its folder.
- Tests in a new `crates/fe-workbench/src/tests/lsp_symlinks.rs` (declared in `tests/mod.rs`), using `crate::tests::lsp::Lsp` and `std::os::unix::fs::symlink`, with the outside file in a second `tempfile::tempdir()`. To show something was *not* sent, open `src/main.rs` afterwards, wait for its `didOpen`, then check `Lsp::sent` and the fake's records.
