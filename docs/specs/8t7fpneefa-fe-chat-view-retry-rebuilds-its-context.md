---
task_uid: "01m5fzyqx71zpt2m8t7fpneefa"
task: "T266"
rev: 3
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The integration test retry_drops_unauthorized_context (top level, crates/fe-chat-view/tests/retry_context.rs) passes: after a failed turn with an attached file, a root change, a trust revocation and a buffer.read removal each make Retry send no file block from the old authorization, while an unchanged authorization retries with the same context; cargo nextest run -p fe-chat-view -p fe-workbench and clippy --all-targets -D warnings pass"
    required: true
    manual: false
---

# fe-chat-view: Retry rebuilds its context from the current authorization instead of copying the failed turn's attachments (T249 review F15 follow-up)

## Problem

T249 (Mac T969, chat context attachments) landed with one review medium (F15) open: **Retry copies historical context verbatim after authorization changes.** An attachment request that fails with a network error can be retried after changing roots or removing `buffer.read`, resending the invalidated file bytes despite cleared chips and restoration flags (`crates/fe-chat-view/src/view.rs` ~259).

## Change

Rebuild a retry from the currently authorized context (or invalidate the stored historical context whenever authorization changes: root change, trust revocation, `buffer.read` removal), so a retry never resends bytes that are no longer authorized. Tests (in the existing chat-view and chat-panel test modules): a failed turn with an attached file, then (a) the root changes, (b) trust is revoked, (c) `buffer.read` is removed, each followed by Retry; the retried request carries no file block from the old authorization.

Depends on T249; the card shares `crates/fe-chat-view/src/view.rs` with the chat chain (T250 ff.), so it runs first.
