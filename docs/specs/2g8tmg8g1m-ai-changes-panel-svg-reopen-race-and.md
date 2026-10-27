---
task_uid: "01m5rbkmmacct2j02g8tmg8g1m"
task: "T247"
rev: 1
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "Reopening an SVG row without an existing viewer never activates the SVG tab after the user navigated elsewhere, and a failed or cancelled text read retires its observer. Tests svg_reopen_does_not_steal_focus_after_navigation, failed_text_read_retires_observer"
    required: true
    manual: false
  - id: "C2"
    text: "A pending new-file preview stays read-only in a split and in a peer view of its buffer, or an edited preview asks before accept or reject discards it. Tests preview_read_only_in_split_view, edited_preview_protected_on_accept_and_reject"
    required: true
    manual: false
  - id: "C3"
    text: "Clicking the row of a closed accepted new file reopens its current saved contents. Test accepted_new_file_row_reopens_saved_contents"
    required: true
    manual: false
---

# AI Changes panel: SVG reopen race and failed reads, read-only previews in split and peer views, reopen closed accepted new files (follow-up of T246 F3, F4, F5)

## Problem
Three medium review findings left on T246 after three rounds (crates/fe-workbench/src/ai_changes_panel.rs):
- F3 (:913): reopening an SVG row without an existing viewer lets Pane::text_landed activate and focus the SVG tab after the user navigates elsewhere; failed or cancelled text reads leave the observer waiting.
- F4 (:996): a pending new-file preview is read-only only in its first EditorView; a split or a peer view of the same buffer is editable, and accept or reject then discards those edits without asking.
- F5 (:982): closed accepted new-file rows are inert; there is no branch that reloads the accepted file's current contents through the text-file route and keeps the changeset association.

## Change
- F3: track the actual text-read request and its terminal state, check navigation before activating the tab, retire the observer on failure or cancellation.
- F4: keep read-only protection across every view sharing the preview buffer, or protect edited previews before discarding them.
- F5: reload a closed accepted file from its current contents on row click.

## Tests
SVG reopen without a viewer then navigate away; failed and cancelled reads; split and peer-view edits before accept and before reject; accepted new file saved with edits, closed, row clicked, reopened with those contents.
