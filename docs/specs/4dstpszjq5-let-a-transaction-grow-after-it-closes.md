---
task_uid: "01m4p29zk77jzq3r4dstpszjq5"
task: "T7"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-buffer test transaction_extend::grows_one_group: a transaction extended with edit_in after Origin::User edits in between undoes as one unit with buffer.undo(|o| o.is_ai()) and redoes as one, with the user's edits untouched"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test transaction_extend::leaves_the_open_transaction_alone: with start_transaction(Origin::User) open, edit_in on an AI transaction leaves it open, and later User edits join it, not the AI one"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test transaction_extend::refuses_gone_transactions: edit_in on an undone, unknown or log-compacted transaction returns EditError::NoSuchTransaction and changes no text, emits no event and keeps redos"
    required: true
    manual: false
  - id: "C4"
    text: "fe-buffer test transaction_extend::bookkeeping_matches_edit: edit_in emits Edited with that transaction id and kind Edit, discards that origin's redos, counts history_bytes, never trims the extended transaction, and is_dirty follows T371's rules"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-buffer and cargo clippy -p fe-buffer --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Let a transaction grow after it closes: Buffer::edit_in appends a batch to an earlier transaction while other origins edit and hold their own open transactions (Mac T383)

Filed on helium from Mac T383 (~/coordination/helium-cards/T383.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-b.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The plan's M8 ("AI seams", `~/.claude/plans/glittery-foraging-graham.md` line 112) has the exit criterion "An AI
transaction applied during simulated typing rebases its anchors correctly, and undo stays grouped by origin". The
user (2026-10-11): "Go ahead with the M8 drafters". Streaming AI edits need one undo group that grows chunk by chunk
while the user keeps typing, and `Buffer` can't hold one:

- `Buffer` has a single open-transaction slot (`open: Option<OpenTransaction>`, `crates/fe-buffer/src/buffer.rs:107`).
  `start_transaction` with another origin while one is open trips a `debug_assert` (:453), and vim's insert session
  keeps an `Origin::User` transaction open, so an AI stream can't keep its transaction open through user typing.
- `Buffer::undo`/`redo` call `close_open_transaction` (:525, :562), so a user's cmd-z would also close an AI group
  held in that slot.
- A closed transaction can't be added to: `record` (:604) only joins the open one.

So each streamed chunk would land as its own AI transaction, and "undo AI" would take one chunk at a time.

## Change

All in `crates/fe-buffer/src/buffer.rs`. This card depends on T371 (same file), whose `note_batch` hook runs beside
each `emit_batch`.

- Add `pub fn edit_in<I, S>(&mut self, transaction: TransactionId, edits: I) -> Result<Option<TransactionId>, EditError>`.
  `I` and `S` are bounded as in `edit`. It applies `edits`, in current coordinates and clipped/normalised exactly as
  `edit` does, as one batch appended to the history entry `transaction`, under that entry's origin. That entry can
  be open or closed. It returns `Ok(Some(transaction))`, or `Ok(None)` when nothing changed. Undo and redo then
  revert the entry's batches together, as they already do for a multi-batch entry (`revert`, :819).
- It refuses with the new variant `EditError::NoSuchTransaction(TransactionId)` ("the transaction was undone, or is
  no longer in the history") if the entry isn't in the history (unknown, trimmed, or dropped by `compact_log`) or is
  undone. When it refuses, it changes nothing, emits nothing and doesn't touch the redo history.
- The open transaction, whatever its origin, is left exactly as it was. Later `edit`s of its origin keep joining it,
  not `transaction`.
- Bookkeeping matches `record`'s for a fresh edit. Factor the shared part out of `record` rather than copying it:
  - it discards the origin's redos and adds the batch's bytes to `history_bytes`;
  - it trims that origin's history, but never drops `transaction` itself (treat it as `trim_history` treats
    `aside`);
  - it emits `BufferEvent::Edited { transaction, kind: Edit, .. }` and calls T371's `note_batch`, so `is_dirty`
    stays right.
- Doc comment: this is how a long-running edit (an AI stream) keeps one undo group while other origins edit and
  hold their own open transactions in between.

Not in this card: the stream itself (the next card), anything outside fe-buffer.

## Tests

New file `crates/fe-buffer/tests/transaction_extend.rs`, integration tests over the public API.
