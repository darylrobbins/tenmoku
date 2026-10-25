---
task_uid: "01m5qd88qcxzy14a0xcwyqsa8n"
task: "T246"
rev: 2
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "Revert Turn's confirmation captures the named buffers' identities and versions and asks again if either changes before the answer is applied. Test revert_confirm_reasks_after_edit_while_pending"
    required: true
    manual: false
  - id: "C2"
    text: "Clicking or pressing Enter on a pending or accepted new-file row opens and reviews it. Tests new_file_row_opens_when_pending, new_file_row_opens_when_accepted. Documented limits (T247): a closed accepted new file's row does not reload its saved contents (F5), and a pending new-file preview is read-only only in its first view, not in a split or peer view of its buffer (F4); both are follow-up T247 and are not required here"
    required: true
    manual: false
  - id: "C3"
    text: "Opening a row for an SVG file shows its text proposal in the active pane (replacing an SVG viewer), in another pane, and after reopening. Tests svg_row_opens_text_proposal, svg_row_reopens_in_another_pane. Documented limit (T247): reopening an SVG row without an existing viewer can still activate the SVG tab after the user navigated elsewhere, and a failed or cancelled text read leaves its observer waiting (F3); follow-up T247, not required here"
    required: true
    manual: false
---

# AI Changes panel: Revert Turn confirm tracks buffer versions, new-file rows open, SVG rows open as text proposals (follow-up of T235 F18, F19, F20)

## Problem
Three medium review findings on T235's AI Changes panel (crates/fe-workbench/src/ai_changes_panel.rs):
- F18 (ai_changes_panel.rs:792): Revert Turn's confirmation records only paths. Editing or reopening a named buffer while the prompt is pending does not invalidate the answer, so confirming can discard edits made after the question.
- F19 (ai_changes_panel.rs:848): New-file rows are inert because proposal(path) returns None for every NewFile entry; clicking or Enter neither reviews pending new files nor activates accepted ones.
- F20 (ai_changes_panel.rs:853): Row opening uses ordinary open_at routing, which opens SVG files as image viewers and cannot attach or reveal their text proposals in the active pane.

## Change
- F18: capture buffer identities and versions with the prompt, ask again if either changes.
- F19: handle pending and accepted new-file entries explicitly on click and Enter.
- F20: open via a text-editor route that shares the proposal's buffer, replacing an SVG viewer in the pane.

## Tests
Pending-prompt edit regression; row opening for pending and accepted new files; SVG row opening in the active pane, another pane, and after reopening.
