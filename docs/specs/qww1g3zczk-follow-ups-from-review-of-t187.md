---
task_uid: "01m4dzdcyperj5sxqww1g3zczk"
task: "T194"
rev: 1
type: "chore"
complexity: "l"
criteria: []
---

# Follow-ups from review of T187

Findings of the review of T187 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/pane.rs:207: Pending search-result opens on any row after the first still lose their return destination. pending_place returns None for these reveals, caret_place passes it through, and Back/Forward still pop history without recording the loading file. After a navigation jump, open an unread file through a search result on row > 0 and immediately press Back; Forward cannot return to that search destination. The symmetric Forward case also remains broken. The new tests cover only plain opens and byte-offset reveals.
  fix: Preserve row/column pending destinations until they can be resolved, or defer Back/Forward while a pending file's caret cannot be determined, without consuming history. Add regressions for pending search-result opens on later rows in both directions, asserting restored paths and offsets.
  instances: crates/fe-workbench/src/pane.rs:207, crates/fe-workbench/src/nav.rs:545, crates/fe-workbench/src/nav.rs:624, crates/fe-workbench/src/nav.rs:625
