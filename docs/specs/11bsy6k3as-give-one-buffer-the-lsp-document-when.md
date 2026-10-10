---
task_uid: "01m4f4nfw0h9nyqj11bsy6k3as"
task: "T236"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::lsp_aliases::aliases_of_one_file_open_one_document: with src/main.rs and src/../src/main.rs open as two buffers, the server gets exactly one didOpen for the URI, and edits in the second buffer produce no didChange"
    required: true
    manual: false
  - id: "C2"
    text: "tests::lsp_aliases::closing_the_owning_alias_hands_the_document_to_the_other: closing the second buffer sends no didClose; closing the first sends didClose and then didOpen carrying the second buffer's current text, so the document stays open"
    required: true
    manual: false
  - id: "C3"
    text: "The existing tests in crates/fe-workbench/src/tests/lsp.rs pass unchanged"
    required: true
    manual: false
---

# Give one buffer the LSP document when path aliases of a file are open as separate buffers

## Problem

Review of T143 (review 01m4ez2b7wstp7r7m7ak8bej2k, F3, medium), filed in T233:

> Normalization can give distinct buffers the same LSP URI. Pane deduplication compares original paths, so opening src/main.rs and src/../src/main.rs creates separate buffers, while this integration sends two didOpen notifications for the same resolved URI. Their independent edits conflict, and closing either sends didClose while the other remains open.

`Workbench::sync_lsp` in `crates/fe-workbench/src/lsp.rs` calls `LspManager::open` for every shown buffer whose resolved path is under the root, keyed only by `BufferId`; nothing stops two buffers with the same resolved path from both being opened, and `LspManager::close` on either closes the server's one document.

## Change

Enforce one owner per resolved path on the LSP side (the finding's "one coherent document owner per server URI"); the pane's path deduplication is unchanged.

- Add the resolved path to `Document`, and an `owners: HashMap<PathBuf, BufferId>` to the workbench's `Lsp` state, kept in step with `documents` (insert on open, remove wherever a document is removed or closed, cleared with `documents` in `new_lsp_manager`).
- In `sync_lsp`, a buffer whose resolved path is owned by another buffer is not opened on the manager and gets no `Document`; its edits and saves aren't sent.
- When the owner's document is closed (its last view gone, or its language changed), the path is free: if another shown buffer resolves to it, it is opened at the next frame with its own current text. Call `cx.notify()` when an owner is released while such a buffer is shown, so that frame comes.
- Document the rule in the **Buffers** bullet of the module docs.
- Tests in a new `crates/fe-workbench/src/tests/lsp_aliases.rs` (declared in `tests/mod.rs`), using `crate::tests::lsp::Lsp`; open the alias with `Lsp::open("src/../src/main.rs")`.
