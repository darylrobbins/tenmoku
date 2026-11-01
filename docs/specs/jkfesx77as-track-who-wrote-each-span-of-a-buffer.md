---
task_uid: "01m6c61y7q25y057jkfesx77as"
task: "T397"
rev: 3
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-buffer tests typed_text_is_the_users, an_ai_edit_is_the_agents and an_accepted_hunk_is_the_proposals_agent_though_applied_as_user pass (crates/fe-buffer/tests/provenance.rs)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer tests undo_of_an_ai_edit_restores_the_users_spans, redo_reapplies_the_ai_author, row_authors_prefers_ai_then_user, reload_clears_authors and ten_thousand_typed_chars_stay_one_span pass"
    required: true
    manual: false
  - id: "C3"
    text: "The typing case of cargo bench -p fe-buffer --bench buffer, run before and after (alternating binaries, fastest sample), is within 5%; the numbers are in the decision file"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-buffer -p fe-editor -p fe-gpui passes with no existing test edited"
    required: true
    manual: false
---

# Track who wrote each span of a buffer (user, agent, tool) through edits, undo and redo, with accepted AI hunks credited to their agent (Mac T1278)

## Problem

User direction 2026-11-01 (vision: "AI-aware not AI-dependent"): show who wrote which lines, the user or an
agent, built on the edit origins undo already records. Every transaction carries an origin
(`fe_buffer::Origin` in `crates/fe-buffer/src/buffer.rs:25`: `User`, `Ai { agent, session }`, `Lsp`, `Format`,
`Host`), but nothing keeps which text came from which. And the origin alone misattributes AI text: accepting
a proposal's hunk applies it under `Origin::User` so it lands on the user's undo stack
(`ProposedChange::accept_as` / `accept_all_as`, `crates/fe-buffer/src/proposed.rs:314`, called with
`Origin::User` from `crates/fe-editor/src/changeset.rs:568,668,989`, `crates/fe-gpui/src/view/proposal.rs:94`,
`crates/fe-gpui/src/view/proposal_commands.rs:250`). This card is the GPUI-free model; the gutter is the next.

## Change

1. New `crates/fe-buffer/src/provenance.rs`, exported from `crates/fe-buffer/src/lib.rs`: `Provenance`, the
   buffer's text as sorted, non-overlapping byte spans each with an `Author` (`User`, `Ai { agent }`,
   `Tool(&'static str)` for Lsp/Format, `Host(name)`); text with no span (as read from disk) has no author.
   Adjacent spans with one author merge, so typing keeps one span.
2. `crates/fe-buffer/src/buffer.rs`: the `Buffer` owns one `Provenance` and updates it as each batch applies (in
   or beside `emit_batch`): spans after an edit shift, replaced text's spans go, inserted text gets the batch's
   author. The author is the batch's origin, except inside `Buffer::with_author(author: Origin, f)`, whose
   batches are authored by `author` (the history entry remembers it, so a redo re-authors the same way). Undo and redo restore the spans the undone or redone batch
   replaced, so undoing an AI edit gives the user's text back its author; the replaced spans are kept beside
   each history entry and dropped with it, so the undo cap bounds them. `Buffer::provenance(&self) -> &Provenance`.
   Loading or reloading text clears it.
3. `crates/fe-buffer/src/proposed.rs`: `accept_as` and `accept_all_as` apply their edit inside
   `buffer.with_author(self.origin.clone(), …)`, so accepted text is the proposal's agent's whatever origin it
   is applied under (the undo stack still follows that origin). No caller changes.
4. `Provenance::row_authors(&self, text: &Text, rows: Range<u32>) -> Vec<(Range<u32>, Option<Author>)>`: runs of
   rows by author: a row with any AI-authored byte is that AI's (the last such span's agent), else any
   user-authored byte makes it the user's, else a tool's or host's, else none.
5. Cost: an edit costs O(log spans + spans it touches), not a pass over every span after it (keep the spans in
   the crate's `sum-tree`, as other per-offset data in fe-buffer is, or offsets relative to the previous span).
   The typing case of `cargo bench -p fe-buffer --bench buffer` is run before and after, alternating binaries,
   compared by fastest sample; the numbers go in the decision file. No wall-clock assertion in tests.

Tests in new `crates/fe-buffer/tests/provenance.rs`: `typed_text_is_the_users`, `an_ai_edit_is_the_agents`,
`an_accepted_hunk_is_the_proposals_agent_though_applied_as_user`, `undo_of_an_ai_edit_restores_the_users_spans`,
`redo_reapplies_the_ai_author`, `row_authors_prefers_ai_then_user`, `reload_clears_authors`,
`ten_thousand_typed_chars_stay_one_span`.

Not here: persistence across sessions (the spans are not in hot-exit backups or on disk). Doing that means
storing spans with `BackupStore` entries and dropping them when the file changes on disk; a follow-up card if
wanted.
