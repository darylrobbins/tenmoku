---
task_uid: "01m6a5gzjyykvt28hv68ch42fm"
task: "T390"
rev: 1
type: "chore"
complexity: "s"
criteria: []
---

# Follow-ups from review of T173

Findings of the review of T173 that don't block the merge (below merge.block_severity):

- F4 medium crates/fe-workbench/src/lsp_highlights.rs:62: words_version is cleared but never assigned when fallback occurrences land. Consequently, shows() always rejects displayed word highlights, so moving within the same word or to another highlighted occurrence clears them for another 250 ms, violating the same-version retain-visible exception.
  fix: Set words_version when installing word fallback. Add coverage that fallback highlights remain visible when moving within an already highlighted range at the same version, while retaining the edit-clearing test.
  instances: crates/fe-workbench/src/lsp_highlights.rs:62, crates/fe-workbench/src/lsp_highlights.rs:287

- F5 medium crates/fe-workbench/src/lsp_highlights.rs:243: The new catch-all Unavailable branch converts actual request failures into word fallback. For example, a configured server with synchronization disabled can reject admission with Unavailable("buffer changed") while the editor still matches wanted; this now displays textual occurrences although the spec requires errors to show nothing.
  fix: Distinguish an unserved buffer from request admission errors, using a serving-server check or narrowly handling the no-server reason, and preserve empty output for other errors. Add coverage for a served buffer whose request admission fails.
  instances: crates/fe-workbench/src/lsp_highlights.rs:243
