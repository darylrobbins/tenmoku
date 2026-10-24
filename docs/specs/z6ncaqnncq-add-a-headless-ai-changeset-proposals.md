---
task_uid: "01m5n96t2jyp33p8z6ncaqnncq"
task: "T233"
rev: 4
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "Changeset::new(turn_id, origin) takes files added with add_file(path, buffer_id, Rc\u003cRefCell\u003cProposedChange\u003e\u003e) and lists them in insertion order with per-file counts (hunks, pending, accepted, rejected); summary() totals them. Buffers are reached through a BufferSet trait (fn buffer_mut(\u0026mut self, BufferId) -\u003e Option\u003c\u0026mut Buffer\u003e) that the workbench implements later and tests implement over a HashMap. Tests files_listed_in_order_with_counts, summary_totals Documented limit (follow-up card): the hunk counts of a file come from add_file_seen(path, buffer_id, proposal, \u0026BufferSet), which reads them from the buffer snapshot; plain add_file cannot, because ProposedChange exposes no hunk count without a BufferSnapshot, so it records the file with counts_known = false and files() reports counts_known = false (never a made-up zero, and never missing = true for an available buffer). The module docs state this."
    required: true
    manual: false
  - id: "C2"
    text: "accept_hunk, reject_hunk, accept_file, reject_file, accept_all and reject_all apply through ProposedChange::accept/accept_all/reject (accepting stays a user-origin transaction as the proposal API defines) and return the TransactionIds applied; a file whose buffer is missing from the BufferSet, or whose buffer version moved so a hunk conflicts, reports Err(Conflict{path}) for that file and carries on with the others in accept_all. Tests accept_one_hunk, accept_file, accept_all_skips_conflicting_file_and_reports_it, reject_all_leaves_text_untouched, resolved_when_every_hunk_decided"
    required: true
    manual: false
  - id: "C3"
    text: "Checkpoint: the first accept in a changeset records, per touched file, the buffer version and the text before the accept; revert(\u0026mut BufferSet) restores that pre-accept text as one new transaction when nothing else was edited since (buffer version equals the one after the last accept), flagged Reverted{reapplied: true}. fe-buffer has no undo-by-transaction-id (Buffer::undo takes an origin filter, which cannot tell this changeset's accepts from other user transactions), so text restore is the revert mechanism, as the card's Change section directs; the module docs say so. A file the user edited since is never silently overwritten: it returns NeedsConfirm{path} and revert_forced() does it. After revert the file's hunks are pending again (proposal refreshed), including any hunk that was stale when the checkpoint was taken. Tests revert_restores_pre_accept_text, revert_after_user_edit_needs_confirm, revert_forced_restores_base, revert_makes_hunks_pending_again, revert_with_nothing_accepted_is_noop, revert_keeps_a_stale_sibling_pending Documented limit (follow-up card): a stale hunk whose new text already equals its proposed text, or a stale deletion already done, is not rebuilt as pending after revert, because ProposedChange::from_edits drops no-op edits and cannot reset a hunk to pending; every other stale sibling stays pending. The module docs state this."
    required: true
    manual: false
  - id: "C4"
    text: "A file added whose buffer does not exist yet (new file) is a NewFile{text} entry accepted by creating the buffer through BufferSet::create(path, text) and reverted by removing it; deleted-file entries are out of scope. Tests new_file_entry_accept_and_revert"
    required: true
    manual: false
  - id: "C5"
    text: "Headless (fe-buffer + fe-editor only), no new dependency; Linux-OK"
    required: true
    manual: false
---

# Add a headless AI changeset: proposals for several files from one turn, with per-file and per-hunk accept and reject, accept all, reject all, a summary, and a checkpoint that reverts everything the turn accepted (Mac T1104)

## Problem
Proposals are per buffer; a chat reply that edits several files (chat_apply.rs applies one fenced block at a time) has no joint review, accept-all or undo-the-whole-turn. VS Code's Copilot edits and Cursor's composer show a changed-files list with keep/undo per file and for all, and can revert to a checkpoint. This card is the model; the review list (card 12) and chat wiring (card 13) follow.

## Change
New `crates/fe-editor/src/changeset.rs`, declared and re-exported from `lib.rs`. It owns no buffers and no GPUI: it holds `Rc<RefCell<ProposedChange>>` (same type `ScriptedAi` and the editor view hold, see `scripted_ai.rs::proposals`) and borrows buffers per call through `BufferSet`. Use `ProposedChange::{accept, accept_all, reject, hunks, refresh, is_resolved}` and `Buffer` transaction undo (`fe-buffer/src/buffer.rs`: TransactionId, undo of a given transaction); read those APIs before designing the revert and keep to what they offer, adding nothing to fe-buffer. If an undo-by-id primitive is missing, use the version check and text restore only, and say so in the module docs.

## Tests
New `crates/fe-editor/tests/changeset.rs` over real `Buffer`s (construction as in `crates/fe-buffer/tests/proposed_change.rs`). Headless, Linux-OK.

## Implementation notes

- `fe-buffer` has no undo-by-transaction-id (`Buffer::undo` takes an origin filter), so revert undoes `Origin::User` transactions newest first, only when the version check shows nothing else edited, verifies each undone id, and otherwise restores the pre-accept text (see the module docs).
- `ProposedChange` cannot return a hunk to pending, so revert replaces the shared proposal with a fresh one built from the hunks at the first accept; hunk ids change.
- Revert is text restore only (`reapplied: true`): origin-filtered undo cannot tell the changeset's accepts from other user transactions.
- `files`, `summary`, `reject_file` and `reject_all` take the `BufferSet`: hunk counts and ids need a snapshot.
- `BufferSet::create`/`remove` default to unsupported, so a new-file entry conflicts on a set that doesn't implement them.
