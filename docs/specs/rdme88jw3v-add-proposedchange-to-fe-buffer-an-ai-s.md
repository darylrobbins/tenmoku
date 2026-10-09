---
task_uid: "01m4f644234kqd7vrdme88jw3v"
task: "T240"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-buffer test proposed_change::from_text_splits_line_hunks: rewriting two separate functions of a three-function file with from_text gives exactly two hunks with the right old text, new text and ranges, and identical text gives none"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test proposed_change::accepting_a_hunk_while_the_user_types: after user edits elsewhere and at a hunk's edge, accept applies only that hunk as one transaction with the proposal's Ai origin, keeps the user's edge text, undo(|o| o.is_ai()) reverts only it, and the other hunk stays Pending at its moved range"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test proposed_change::edited_hunk_goes_stale: a user edit inside a hunk's range makes it Stale; accept returns ProposalError::Stale and leaves the text unchanged; accept_all applies the other pending hunks in one transaction and returns the stale hunk's id"
    required: true
    manual: false
  - id: "C4"
    text: "fe-buffer test proposed_change::survives_log_compaction: after refresh(\u0026buffer.snapshot()) and buffer.compact_log(proposal.version()), accepting a hunk still lands at the right offsets"
    required: true
    manual: false
---

# Add ProposedChange to fe-buffer: an AI's edits against a snapshot as anchored hunks, each accepted as an AI transaction or rejected, and stale once the user edits under it

## Problem

AI-edit primitives are the product's second priority. The plan's M8 ("AI seams: `ProposedChange` diff blocks,
ghost text, decoration providers, streaming edits") hasn't started, and `rg ProposedChange crates` finds nothing.

`docs/decisions/command-model.md` § "Dry run and proposed changes" (T68) defines a `ProposedChange` as "edits
against a snapshot version, which `Buffer::edit_from` can later apply or reject as conflicting". So a dry-run
command and an agent share one shape. fe-buffer already has the pieces this needs:

- `Origin::Ai { agent, session }`;
- `Anchor`s, with `BufferSnapshot::refresh` and `Buffer::compact_log`;
- `Buffer::edit_from` and its conflict check (`crates/fe-buffer/src/buffer.rs:340-383`);
- origin-scoped undo.

What's missing is the object that holds a proposal and lets the user take it hunk by hunk. Diff blocks in the view
and the dry-run executor build on it later.

## Change

Add the new module `crates/fe-buffer/src/proposed.rs`, exported from `crates/fe-buffer/src/lib.rs`. Add
`imara-diff` (Apache-2.0) as a workspace dependency, appending lines only, and to `crates/fe-buffer/Cargo.toml`. It
must not depend on GPUI.

- Constructors:
  - `ProposedChange::from_edits(base: &BufferSnapshot, edits: impl IntoIterator<Item = (Range<usize>, impl Into<Arc<str>>)>, origin: Origin)`
    makes one hunk per edit, sorted. It panics on overlapping edits, as `Buffer::edit` would reject them.
  - `ProposedChange::from_text(base: &BufferSnapshot, range: Range<usize>, new_text: &str, origin: Origin)` diffs
    `base`'s text in `range` against `new_text` by lines with `imara_diff` (histogram). It makes one hunk per
    changed line group. Identical text gives no hunks.
- Each hunk stores its old range as two `Anchor`s, its old text and its new text. The anchor biases match
  `edit_from`: text the user inserts at a hunk's edges is kept, not swallowed or replaced. Each hunk has a
  `HunkId` and a status: `Pending | Accepted | Rejected | Stale`.
- `hunks(&self, snapshot: &BufferSnapshot) -> Vec<HunkView>`. Each `HunkView` has its id, its current
  `Range<usize>`, its old text, its new text and its status. A `Pending` hunk whose current range text no longer
  equals its old text is reported, and recorded, as `Stale`.
- `accept(&mut self, buffer: &mut Buffer, id) -> Result<TransactionId, ProposalError>`. It checks the hunk's current
  text, then applies it with `Buffer::edit` at the resolved range under the proposal's origin, as one transaction.
  A stale, rejected or already accepted hunk returns `ProposalError::{Stale, NotPending}` and changes nothing.
- `accept_all(&mut self, buffer) -> (Option<TransactionId>, Vec<HunkId> /* skipped as stale */)`. It applies every
  pending, non-stale hunk in one transaction (`start_transaction`/`end_transaction`).
- `reject(id)`, and `is_resolved()` (no pending hunks left).
- `refresh(&mut self, snapshot)` rebases every anchor onto `snapshot` with `BufferSnapshot::refresh`. `version()`
  returns the oldest version its anchors use, so the owner keeps `compact_log`'s floor at or below it.
- Update the `//!` list in `lib.rs` with one bullet.

Not in this card:

- diff blocks in fe-display or fe-gpui;
- the dry-run executor in fe-command or fe-editor;
- streaming hunks as an agent produces them.

## Tests

All tests go in the new file `crates/fe-buffer/tests/proposed_change.rs`, as integration tests over the public API.
