---
task_uid: "01m5t4r022e3y8ph1d8gdgqgx8"
task: "T346"
rev: 1
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T13

Findings of the review of T13 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-editor/src/ghost_text.rs:44: Suggestions retain CRLF/lone-CR endings, while Buffer::edit normalizes inserted text to LF. Prefix consumption therefore diverges from the buffer, and partial acceptance loses the remaining suggestion. For example, offering "a\r\nb" before "Z" and calling accept_line inserts "a\n" but places the caret after Z because EditorModel::insert computes its position using the unnormalized input length. Whole acceptance has the same caret error.
  fix: Use the buffer's normalized LF representation consistently for prefix matching, word/line boundaries, and insertion lengths; retain the original separately if text() must expose it unchanged. Add CRLF and lone-CR tests covering typing, partial acceptance, whole acceptance, and caret placement before existing suffix text.
  instances: crates/fe-editor/src/ghost_text.rs:44, crates/fe-editor/src/ghost_text.rs:93, crates/fe-editor/src/ghost_text.rs:146
