---
task_uid: "01m4rqehy1xcjb36b2s1h6afhj"
task: "T50"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::proposal_accept_tests::accept_button_applies_hunk: a simulated click on a pending block's Accept button applies its new text, emits ProposalEvent::Accepted with the returned transaction, leaves the caret where it was, and the next frame paints no block for it"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::proposal_accept_tests::reject_button_keeps_text: a click on Reject leaves the buffer's text and version unchanged, emits ProposalEvent::Rejected, and the next frame paints no block for it"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::proposal_accept_tests::stale_hunk_offers_reject_only: a stale hunk's block has a Reject button and no Accept button, and accept_hunk on it returns Err(ProposalError::Stale), changes nothing and emits nothing"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::proposal_accept_tests::cmd_z_undoes_an_accepted_hunk: after typing elsewhere then accepting a hunk, dispatching editor.undo reverts exactly the hunk's text and keeps the typing, editor.redo re-applies the hunk, two more editor.undo revert the hunk then the typing, the hunk stays Accepted with no block painted throughout, and the buffer has no Ai-origin transaction; with vim on, accepting during an insert session and pressing Esc then u undoes only the accepted hunk"
    required: true
    manual: false
  - id: "C5"
    text: "fe-gpui test view::proposal_accept_tests::split_views_share_resolution: with two views of one buffer showing one proposal, accepting a hunk in one removes its block from the other's next frame, and the host API reject_hunk behaves as the button does"
    required: true
    manual: false
---

# Accept or reject a proposal's hunks from Accept and Reject buttons in its diff blocks, or from the host, as the user's undoable edit, with an event telling the host which (Mac T258)

Filed on helium from Mac T258 (~/coordination/helium-cards/T258.json; Mac lead message ~/coordination/from-mac-lead-2026-10-12-helium-x.md). Title, type, complexity, scope, criteria and labels as on the Mac.

Helium notes (Mac note x, stream 1, the proposal/AI chain, run strictly one after another for crates/fe-gpui/src/view.rs): counts the card pins (keymap rows in default-macos.toml, keymap_tests.rs and similar) were taken on Mac main; on helium use the counts on your base and change them by exactly the deltas the card states. Screenshot goldens are Metal-only and do not run on helium: a golden criterion is met here by the Linux suite passing, and the Mac regenerates or checks goldens after the sync. Argon's Mac T427 (multi-cursor keys, alt-click) also changes view.rs and vim.rs but is not on helium: do not port it or code around it.

## Problem

T255 shows a `ProposedChange`'s hunks as diff blocks in `EditorView` (`crates/fe-gpui/src/view/proposal.rs`,
`crates/fe-gpui/src/element/proposal.rs`), read-only. The approved plan (2026-10-08) wants blocks "the user can
accept or reject per hunk", and the host app (AI-first dev tool) needs to hear what the user chose, to tell the
agent.

The user decided on 2026-10-09 that "accepting a hunk counts as the user's own edit — cmd-z
(`EditorModel::undo`) undoes an accepted hunk, as in VS Code/Cursor". `EditorModel::undo`
(`crates/fe-editor/src/lib.rs:927`) undoes only transactions under the model's origin (`Origin::User`), so the
view must accept with `ProposedChange::accept_as(…, Origin::User)` (the previous card), not
`ProposedChange::accept`, which lands under the proposal's `Origin::Ai` and which cmd-z skips.

## Change

- Host API on `EditorView`, in `crates/fe-gpui/src/view/proposal.rs`:
  - `accept_hunk(&mut self, id: HunkId, cx) -> Result<TransactionId, ProposalError>`: finds the shown proposal
    holding `id` and applies it with `ProposedChange::accept_as(&mut self.model_mut().buffer_mut(), id, Origin::User)`
    (so the model catches up and the hunk is on the user's undo stack), then notifies. An id in no shown proposal
    is `ProposalError::NotPending`.
  - `reject_hunk(&mut self, id: HunkId, cx) -> Result<(), ProposalError>` via `ProposedChange::reject`; the buffer
    isn't touched.
  - `pub enum ProposalEvent { Accepted { hunk: HunkId, transaction: TransactionId }, Rejected { hunk: HunkId } }`,
    with `impl EventEmitter<ProposalEvent> for EditorView`, emitted after each successful accept or reject, by the
    buttons or the host API alike. Export it from `crates/fe-gpui/src/lib.rs` (one `pub use` line).
- Buttons in each block's header row (`element/proposal.rs` paints them; their boxes are kept on the view like
  `painted_chevrons`, but in all builds): `Accept` and `Reject` on a pending hunk, only `Reject` on a stale one.
  A left click on a button, in `EditorView::on_mouse_down` (`view.rs`), calls `accept_hunk`/`reject_hunk` and
  doesn't move the caret or start a drag. The pointer is a hand over a button.
- Undo: an accepted hunk is one `Origin::User` transaction of its own, even inside vim's insert session (T252's
  isolation). `editor.undo` (cmd-z) reverts it and `editor.redo` (cmd-shift-z) re-applies it, like any edit of
  the user's. Undo and redo change only the text: the hunk stays `Accepted` and its block doesn't come back.
- After an accept or reject, every view showing that proposal repaints without its block on its next frame.

Not in this card: keyboard commands or keymap rows for accept/reject, next/previous hunk, accept all and reject all
(the next card), an undo button.

## Tests

New file `crates/fe-gpui/src/view/proposal_accept_tests.rs` (declared `#[cfg(test)]` from `view.rs`), in the style
of `crates/fe-gpui/src/view/fold_hover_tests.rs`.

## Implementation notes

- The header row reads "Proposed change" (or a stale hunk's reason), then its buttons: Accept and Reject on a
  pending hunk, Reject alone on a stale one. A read-only view offers Reject alone too; the host API
  `accept_hunk` still works through it. Buttons sit after the header text so a click near a block's left edge
  still lands on the text rows (`proposal_tests::clicks_land_on_text_rows`).
- Button boxes live on `EditorView::proposal_buttons` (all builds), as the part of each box in view. Hitboxes
  over the same parts give the pointing hand.
- After an accept or reject the view calls `cx.refresh_windows()`, so views of the buffer in other windows drop
  the block on their next frame too.
- Carried findings: T49 F1, header rows (text and buttons) now count toward the horizontal scroll reach
  (`proposal_accept_tests::narrow_pane_scrolls_to_stale_reason`). T49 F2, the new texts are indexed once
  (`element::proposal::Texts`: a rope per text, its line count, and long rows' `RowIndex`es measured with the
  frame's font and tab size). A frame reads only the rows in view and a slice of a long one
  (`frames_read_a_slice_of_a_long_line`, `frames_read_the_rows_of_a_tall_block_in_view`).
