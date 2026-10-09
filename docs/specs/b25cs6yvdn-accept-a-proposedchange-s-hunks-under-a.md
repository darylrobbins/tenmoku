---
task_uid: "01m4fg2513q3kh9zb25cs6yvdn"
task: "T257"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-buffer test proposed_accept_as::user_undo_reverts_only_the_hunk: after a user edit elsewhere, accept_as(\u0026mut buffer, id, Origin::User) applies the hunk as a new transaction; buffer.undo(|o| *o == Origin::User) reverts exactly that hunk and keeps the earlier edit, buffer.undo(|o| o.is_ai()) returns Ok(None), the hunk stays Accepted and proposal.origin() is still the Ai origin"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test proposed_accept_as::all_in_one_user_transaction: with one of three hunks edited stale, accept_all_as(\u0026mut buffer, Origin::User) applies the other two in one Origin::User transaction, returns the stale id as accept_all does, and one user-origin undo reverts both"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test proposed_accept_as::inside_an_open_user_transaction: with buffer.start_transaction(Origin::User) open, accept_as and accept_all_as under Origin::User don't panic and return transaction ids other than the open one; after end_transaction the first user-origin undo reverts only the accepted text and the second only the open transaction's edits"
    required: true
    manual: false
  - id: "C4"
    text: "accept and accept_all still edit under the proposal's origin: crates/fe-buffer/tests/proposed_change.rs and crates/fe-buffer/tests/proposed_dry_run.rs pass unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "The ProposedChange bullet in crates/fe-buffer/src/lib.rs and an Accepting paragraph in docs/decisions/command-model.md § Dry run and proposed changes state that the editor accepts a hunk as the user's edit (Origin::User, undone by cmd-z, redone by cmd-shift-z, as in VS Code and Cursor; decided 2026-10-09, replacing T240's AI-transaction accept), that the proposal's origin records who proposed it, and that undo reverts the text but leaves the hunk accepted"
    required: true
    manual: false
---

# Accept a ProposedChange's hunks under a caller's origin, so the editor can put an accepted hunk on the user's undo stack

## Problem

The user decided on 2026-10-09: "accepting a hunk counts as the user's own edit — cmd-z (`EditorModel::undo`)
undoes an accepted hunk, as in VS Code/Cursor". T240 (1799485) chose the opposite: `ProposedChange::accept` and
`accept_all` (`crates/fe-buffer/src/proposed.rs`, `accept` at about line 254, `accept_all` at about 289) always
edit under the proposal's own origin (`self.origin`, an `Origin::Ai { .. }`). `EditorModel::undo`
(`crates/fe-editor/src/lib.rs:927`) only undoes transactions whose origin equals the model's (`Origin::User`), so
an editor that accepts through `accept` puts the hunk where cmd-z can't reach it, and there is no way to choose
another origin.

T240's design is written down in two places that will contradict the new behaviour:
- `crates/fe-buffer/src/lib.rs` `//!` bullet: "the user accepts (each as a transaction under the proposal's origin)";
- `docs/decisions/command-model.md` § "Dry run and proposed changes" says nothing about which origin an accepted
  hunk lands under, which is where the decision belongs.

T252 (review follow-ups of T240) makes `accept` and `accept_all` open a transaction of their own even when the
caller has one open (vim's insert session keeps an `Origin::User` transaction open). That matters more now: an
accept under `Origin::User` inside that session must not join it.

## Change

In `crates/fe-buffer/src/proposed.rs`:

- Add `ProposedChange::accept_as(&mut self, buffer: &mut Buffer, id: HunkId, origin: Origin) -> Result<TransactionId, ProposalError>`
  and `accept_all_as(&mut self, buffer: &mut Buffer, origin: Origin) -> (Option<TransactionId>, Vec<HunkId>)`.
  They do exactly what `accept`/`accept_all` do (checks, landing anchors, statuses, T252's transaction isolation)
  but edit, and open their transaction, under `origin`.
- `accept` and `accept_all` become one-line calls of the `_as` forms with `self.origin.clone()`; their behaviour
  doesn't change. They remain for a host that applies an agent's edits itself and wants them on the agent's undo
  history.
- `ProposedChange::origin()` keeps meaning who proposed the change; its doc comment says so.
- Doc comments of `accept`, `accept_all` and the type say that an editor accepting on the user's behalf uses the
  `_as` forms with the acting origin (`Origin::User`), so the user's undo reverts the hunk.

Docs (this is where the decision is recorded):

- `crates/fe-buffer/src/lib.rs`: the `ProposedChange` bullet says hunks are accepted as a transaction under the
  accepting origin (the user's, from the editor) or the proposal's own.
- `docs/decisions/command-model.md` § "Dry run and proposed changes": add a short "Accepting" paragraph:
  accepting a hunk in the editor is the user's edit: it lands under `Origin::User` on the user's undo stack, so
  cmd-z undoes it and cmd-shift-z redoes it, as in VS Code and Cursor (user decision, 2026-10-09, replacing T240's
  "accepted as the AI's transaction"). The proposal's origin still records who proposed it. Undoing an accepted
  hunk reverts its text; the hunk stays accepted and its diff block doesn't return.

Not in this card: anything in fe-editor or fe-gpui (the editor side is the next card).

## Tests

New file `crates/fe-buffer/tests/proposed_accept_as.rs`, integration tests over the public API in the style of
`crates/fe-buffer/tests/proposed_change.rs` (which this card doesn't change).
