---
task_uid: "01m4h7mpp30hhtf4xecgtnzabj"
task: "T302"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T146

Findings of the review of T146 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-gpui/src/hover.rs:420: Reply admission checks only the buffer version. While a request is pending, pointer movement within the original word updates current.hovered.under without cancelling it. If the reply's range excludes that new position, its content is still displayed and remains visible until another pointer event, showing hover for an older position.
  fix: Before displaying a reply, validate its returned range against the latest hovered position and discard or refresh replies that no longer cover it. Add a delayed-provider test that moves within a word before a narrower-range reply arrives.
  instances: crates/fe-gpui/src/hover.rs:420

- F2 medium crates/fe-gpui/src/hover.rs:280: Existing hover coverage is checked only after finding a new word or diagnostic. Whitespace without diagnostics produces None first, so moving through whitespace inside a returned range dismisses the popover. For example, a hover covering `alpha + beta` disappears on either intervening space despite the pointer remaining inside its range.
  fix: Check the current popover's range against the hit-tested offset before filtering positions for new-request eligibility. Extend the range test to move through whitespace inside the returned range and assert that no new request occurs.
  instances: crates/fe-gpui/src/hover.rs:280

- F3 medium crates/fe-gpui/src/hover.rs:294: Caret-triggered popovers are explicitly excluded from the coverage reuse path. After cmd-k cmd-i shows hover, moving the pointer onto the same word or within the returned range immediately hides it and starts another delayed request. This causes flicker and duplicate requests even though the pointer has not moved off the hover range.
  fix: Preserve caret-triggered hover when pointer movement remains within its word or returned range, while retaining caret-movement dismissal. Add tests for both pending and completed command-triggered hover followed by same-range pointer movement.
  instances: crates/fe-gpui/src/hover.rs:294

- F4 medium crates/fe-lsp/src/tests/hover.rs:183: Both delayed-request checks capture the hover count after calling manager.hover. That call eagerly queues the request, so the fake thread can record it before the count is sampled. The subsequent wait then expects an additional request that never comes and fails after ten seconds, making the required test scheduling-dependent.
  fix: Capture the baseline count before issuing each request, or synchronize using the specific request ID.
  instances: crates/fe-lsp/src/tests/hover.rs:183, crates/fe-lsp/src/tests/hover.rs:207
