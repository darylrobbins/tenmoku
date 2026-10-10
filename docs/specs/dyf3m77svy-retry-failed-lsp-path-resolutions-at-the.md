---
task_uid: "01m4f4nh1zyh9r4pdyf3m77svy"
task: "T237"
rev: 1
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "tests::lsp_resolve_retry::a_new_file_opens_once_its_folder_exists_and_it_is_saved: a new .rs file opened in a missing folder sends nothing; after the folder is created and the buffer is saved, its server gets didOpen"
    required: true
    manual: false
  - id: "C2"
    text: "tests::lsp_resolve_retry::a_failure_cached_before_a_root_change_is_resolved_again: after the folder is created and set_root replaces the root (trusted), without a save, the buffer gets didOpen"
    required: true
    manual: false
  - id: "C3"
    text: "tests::lsp_resolve_retry::a_root_that_failed_to_resolve_starts_servers_after_a_save: after set_root to a missing folder, then creating it and trusting it, saving a shown .rs file under it launches its server and sends didOpen"
    required: true
    manual: false
  - id: "C4"
    text: "A resolution that completes after its manager was replaced changes no state; the existing tests in crates/fe-workbench/src/tests/lsp.rs pass unchanged"
    required: true
    manual: false
---

# Retry failed LSP path resolutions at the next save and at a root change

## Problem

Review of T143 (review 01m4ez2b7wstp7r7m7ak8bej2k, F4, medium), filed in T233:

> Resolution failures become permanent. Opening a new Rust file before its parent directory exists caches None; creating the directory and saving never retries because contains_key remains true. Root replacement also preserves this cache. Similarly, an initial root-resolution failure leaves the manager absent without recovery when the root becomes available.

In `crates/fe-workbench/src/lsp.rs`: `sync_lsp` resolves a buffer only while `!lsp.resolved.contains_key(&buffer)`, and stores `None` on failure for as long as the buffer is shown. `new_lsp_manager` clears `documents`, `servers` and `messages` but not `resolved` or `resolving`, and its root-resolution `Err` arm only logs. Resolution completions spawned in `sync_lsp` aren't checked against `lsp.generation`.

## Change

Retry on save, not on a timer or a filesystem watcher: a save is when a new file's folder has to exist, and it costs one `canonicalize` per save.

- **Failed files.** When a buffer's resolution lands as `None`, keep a `fe_buffer::Subscription` for it (as `Document::moved` does). In `sync_lsp`, a failed buffer whose subscription drained a `BufferEvent::Saved` has its `resolved` entry removed, so the same frame's resolve step resolves it again. Drop the subscription with the entry.
- **Root replacement.** `new_lsp_manager` clears `resolved`, `resolving` and the failed buffers' subscriptions. Each resolution task records the generation it started under; its completion is dropped (and doesn't touch `resolving`) if `lsp.generation` has moved on.
- **Failed root.** When the root's resolution fails, remember it (`root_failed`). While it's set and `lsp.manager` is `None`, `sync_lsp` subscribes to each shown buffer and, on a drained `Saved`, retries the root's resolution once (one attempt in flight at a time, guarded by the generation as `new_lsp_manager` is). Success starts the manager as today (`start_lsp_manager`, then trust).
- Update the **Paths** bullet of the module docs to say failures are retried at the buffer's next save and at a root change.
- Tests in a new `crates/fe-workbench/src/tests/lsp_resolve_retry.rs` (declared in `tests/mod.rs`), using `crate::tests::lsp::Lsp`, `Workbench::set_root` and `Lsp::set_trust`; save with `cmd-s` as `edits_and_saves_reach_the_server` does.
