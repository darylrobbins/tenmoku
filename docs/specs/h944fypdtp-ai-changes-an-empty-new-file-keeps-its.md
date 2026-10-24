---
task_uid: "01m5q7z4n1aqb0j8h944fypdtp"
task: "T244"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "An empty new file proposed by a changeset has no synthetic newline: undo does not insert one, the file stays unsaved so closing prompts, and saving writes exactly the accepted text. Tests empty_new_file_undo_inserts_no_newline, empty_new_file_close_is_protected, empty_new_file_saves_accepted_text"
    required: true
    manual: false
---

# AI Changes: an empty new file keeps its unsaved state without a synthetic newline baseline (follow-up of T235 F15)

## Problem
T235's empty-file handling in ai_changes_buffers.rs invents a saved newline baseline and deletes it through a user transaction. Undo then inserts an unrequested newline and marks the nonexistent file clean, so it can be closed without unsaved-file protection (review finding F15, medium, T235).

## Change
Represent the unsaved state of an empty new file independently of synthetic text. Verify undo, close protection and saving.

## Tests
Headless tests for undo, close protection and saving of an empty new file proposed by a changeset.
