---
task_uid: "01m5n96tbfq1cd07ktx633zw52"
task: "T235"
rev: 5
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "Workbench::set_changeset(Changeset) (pub(crate), the entry point card 13 calls) makes the AI Changes panel show it: a list opened in the bottom dock beside Problems (follow problems_panel.rs for dock registration) with a header 'N files, M hunks pending', and a row per file: icon, path relative to the root, +/- line counts from the hunks, pending/accepted/rejected counts, and Accept File and Reject File buttons (selectors ai-changes-accept-\u003cpath\u003e, ai-changes-reject-\u003cpath\u003e); header buttons Accept All, Reject All and Revert Turn (disabled until something is accepted). Tests lists_files_with_counts, accept_file_updates_row_and_buffer, reject_file, accept_all_and_reject_all, revert_turn_restores_text. Documented limit (T243): the panel is currently a sidebar panel; bottom-dock placement with no activity-rail entry is follow-up T243 and is not required here"
    required: true
    manual: false
  - id: "C2"
    text: "Clicking a row, or Enter on it, opens the file in the active pane (opening it if closed), attaches the file's ProposedChange to its editor with set_proposals and reveals the first pending hunk; with every file's editor already showing its proposal, per-hunk Accept / Reject remain the editor's own buttons and the list counts update live from the editor's ProposalEvent. Tests click_opens_file_at_first_pending_hunk, hunk_accept_in_editor_updates_counts. Documented limit (T244): an empty new file's unsaved state (no synthetic newline baseline) is follow-up T244 and is not required here. Documented limits (T246): new-file rows (pending or accepted) do not open on click or Enter (F19), and SVG files open through the ordinary route so their text proposal is not attached in the active pane (F20); both are follow-up T246 and are not required here"
    required: true
    manual: false
  - id: "C3"
    text: "Commands (registry; defaults in fastedit.toml): ai.changes_accept_all, ai.changes_reject_all, ai.changes_revert, ai.changes_next_file, ai.changes_previous_file, ai.show_changes (opens/focuses the panel); defaults only for ai.changes_next_file/previous_file (alt-] / alt-[ chosen if unbound; check with rg) in context ai_changes; the buttons dispatch the same commands. Tests commands_match_buttons, binding_resolves_per_platform (keymap)"
    required: true
    manual: false
  - id: "C4"
    text: "Revert Turn with a file edited since shows a confirm prompt naming the files (the existing confirm dialog used by close-with-unsaved) and calls revert_forced only on yes; a resolved changeset (all decided, none accepted) leaves the panel showing 'No pending changes'. Tests revert_prompts_when_files_edited, resolved_changeset_message. Required fixes: editor Accept All with retained and current proposals must not record earlier-turn edits into this turn's checkpoint, so Revert Turn never removes them (test accept_all_does_not_record_earlier_turn_edits_in_the_checkpoint); the checkpoint and per-hunk decisions survive save, close and reopen, and rejected hunks stay rejected (test checkpoint_and_decisions_survive_save_close_reopen, covering accepted and partially rejected files). Documented limit (T245): editor Accept All skipping accepted no-op hunks (no AcceptedNoChange event) is follow-up T245 and is not required here. Documented limit (T246): the Revert Turn confirmation names paths only and is not invalidated by edits made while the prompt is pending (F18); follow-up T246, not required here"
    required: true
    manual: false
  - id: "C5"
    text: "BufferSet adapter in the new ai_changes_buffers.rs maps BufferId to the open editors' buffers in the workbench; headless GPUI tests run on Linux; no golden"
    required: true
    manual: false
---

# Add an AI Changes review list: the files a turn proposed with hunk counts, Accept / Reject per file and for all, a click that opens the file at its first pending hunk, and Revert Turn from the checkpoint (Mac T1105)

## Problem
Card 11's model needs a place to review it. VS Code's Copilot Edits shows a 'N files changed' list with Keep/Undo per file and for all; Source Control shows the same shape for git changes (fastedit's Git panel, `git_panel.rs`, is the nearest in-tree list to copy).

## Change
- New `ai_changes_panel.rs` (the panel entity and rows), `ai_changes_buffers.rs` (the `BufferSet` impl over the workbench's open editors plus creating buffers for NewFile entries), `Workbench` stores the active `Changeset` (one at a time; a new `set_changeset` replaces it after resolving the old: if the old has pending hunks, the old proposals stay on their editors, only the panel's list is replaced).
- Dock registration, panel icon and activity-rail behaviour follow `problems_panel.rs` and `dock.rs`; do not add a rail entry (bottom dock tab only).
- Commands via `#[kiln::command]` in `workbench_commands.rs`; keymap defaults in `fastedit.toml`; the two command tables get rows.
- Does not parse chat replies (card 13).

## Tests
New `ai_changes_panel_tests.rs` builds a `Changeset` directly over two or three open editors with `ProposedChange::from_text`; `crates/fe-keymap/tests/ai_changes_bindings.rs`. Linux-OK. Textual conflicts expected in the shared files, same as other command-adding cards.

## Implementation notes

- The panel is in the sidebar dock beside Problems (the only bottom dock is the terminal's, which draws no panels of its own), made lazily on the first `set_changeset` or `ai.show_changes`, and joins the rail once shown, as Problems does. `dock.rs` and `workbench.rs` changed to register it.
- `Workbench::set_changeset(changeset, window, cx)` takes the window to show the dock.
- `fe-editor`'s `Changeset` gained `proposal(path)` and `new_file_text(path)` (read-only), and `arm`, `adopt_accept` and `rebind`: the workbench arms each file before an editor can accept, files an editor's own hunk accept under the checkpoint so Revert Turn covers it, and follows a file whose tab was closed and reopened to its new buffer (when it holds the text the file had before any accept; its hunks are pending again).
- The turn's proposals are attached to every open editor of its files when the changeset is set, and again for editors opened later; a click opens the file in the active pane (sharing the buffer of another pane's editor) at its first pending hunk.
- A file the turn created opens as an unsaved buffer; reverting it closes its tab without asking.
- The Revert confirmation is tied to its changeset and the files it named: a replaced changeset ignores the answer, and files edited since the question ask again.
- Next/previous file only move the selection; Enter or a click opens the file.
- An editor accept is filed under the checkpoint only for hunks of the changeset's own proposals; when the text holds changes beyond the turn's hunks (an earlier turn's accepted hunks, text typed between accepts) Revert Turn undoes the turn's hunks one by one and leaves those be. A hunk accepted with nothing to change is reported to subscribers as `ProposalEvent::AcceptedNoChange`.
- A file saved and closed, then reopened, is followed to its new buffer with its checkpoint and its accepted and rejected hunks (`Changeset::rebind`), unless it was edited after the save (a toast says so).
- Known limits, left for follow-ups T243-T245: AI Changes stays in the right sidebar rather than a bottom dock tab; an empty new file is made from a synthetic newline baseline; editor Accept All skips accepted no-op hunks.
