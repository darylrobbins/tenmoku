---
task_uid: "01m4p29zt7rf9r85m4cc5qt7gq"
task: "T8"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-buffer test ai_stream::typing_during_a_stream (the M8 exit test): User edits before, after and at both edges of the region, some inside an open User transaction, interleaved with 5 pushes give the exact expected text; range() covers exactly the chunks; an anchor the user set after the region resolves right; repeated undo(User) removes only the user's text; one undo(is_ai) restores the old target text; redo brings the stream back"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer proptest ai_stream::interleaved_matches_model (at most 64 cases): random User edits outside the region or at its edges, interleaved with random chunks, match a String model, and one undo(is_ai) leaves exactly the user-only model text"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test ai_stream::conflicts_and_undone: a user edit inside the target before the first chunk gives StreamError::Conflict with nothing applied; a push after undo-AI gives StreamError::Undone; after a user edit inside streamed text, undo(is_ai) returns UndoError::Conflict, changes nothing, and the next push still lands at the region's end"
    required: true
    manual: false
  - id: "C4"
    text: "fe-buffer test ai_stream::finish_keeps_and_crlf: finish mid-stream keeps what landed as one AI transaction, and a \\r\\n split across two chunks becomes one \\n"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-buffer and clippy -D warnings pass, and fe-buffer still has no GPUI dependency"
    required: true
    manual: false
---

# Stream an AI's edit into the buffer chunk by chunk as one AI-origin undo group while the user types, with anchors rebased and cancel keeping what landed (Mac T384)

Filed on helium from Mac T384 (~/coordination/helium-cards/T384.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-b.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The plan's M8 ("AI seams: ... streaming edits", `~/.claude/plans/glittery-foraging-graham.md` line 112) has the exit
criterion "An AI transaction applied during simulated typing rebases its anchors correctly, and undo stays grouped by
origin". The user (2026-10-11): "Go ahead with the M8 drafters". `ProposedChange` (T240, `crates/fe-buffer/src/proposed.rs`)
covers a finished AI edit offered as hunks, and T240's spec leaves out "streaming hunks as an agent produces them".
Nothing applies an AI's output as it arrives, chunk by chunk, while the user keeps typing. The previous card adds
`Buffer::edit_in`, which lets one transaction grow while other origins edit in between.

## Change

GPUI-free, in fe-buffer. Add the new module `crates/fe-buffer/src/stream.rs`. Export `EditStream` and `StreamError`
from `crates/fe-buffer/src/lib.rs` and add one `//!` bullet there.

`EditStream` writes an AI's output into a target range as it arrives, as one undo group under the AI's origin:

- `EditStream::begin(buffer: &Buffer, range: Range<usize>, origin: Origin) -> EditStream` records the target range
  (clipped) as two anchors and keeps its old text. It doesn't edit. The start anchor is `anchor_after` and the end is
  `anchor_before`, so text the user inserts at the range's edges stays outside it, as with `edit_from`.
- `push(&mut self, buffer: &mut Buffer, chunk: &str) -> Result<(), StreamError>`:
  - **The first non-empty push** checks that the text under the resolved target range still equals the old text.
    If it doesn't, it returns `StreamError::Conflict` and the stream is dead. Otherwise it replaces the range with
    the chunk through `buffer.with_transaction_aside(|b| b.edit(..))`, so the chunk gets a fresh transaction even
    inside an open one of the same origin, and keeps that `TransactionId`.
  - **Every later push** inserts at the region's end through `buffer.edit_in(id, ..)`. Map
    `EditError::NoSuchTransaction` (the group was undone, trimmed or compacted away) to `StreamError::Undone`; the
    stream is then dead.
  - After each push, the region is `start..end`: start `anchor_after`, end `anchor_before` at the end of the text
    just written. So the user's text typed at either edge stays outside the region, and later chunks go before text
    the user typed at the region's end. If the user's insertion at an empty region puts the end anchor before the
    start, chunks land at the start.
  - Text the user changes inside the region stays. Later chunks still go to the region's end.
  - A chunk ending in `\r` holds that `\r` back until the next push or `finish`, so a `\r\n` split across chunks
    becomes one `\n`.
  - Pushing to a dead or finished stream returns `StreamError::Finished` (dead) or the error that killed it.
- `range(&self, snapshot: &BufferSnapshot) -> Range<usize>`: the target range before the first chunk, then the
  region written so far.
- `transaction(&self) -> Option<TransactionId>`, `origin(&self) -> &Origin`.
- `finish(self, buffer: &mut Buffer) -> Option<TransactionId>` flushes a held `\r` and ends the stream.
- `refresh(&mut self, snapshot)` and `version()`, as on `ProposedChange`, so the owner keeps `compact_log`'s floor at
  or below it.

Cancelling mid-stream keeps what landed (decided here). A host stops a stream with `finish`, or by dropping it. The
text written so far stays, and it is one transaction under the AI's origin. So the user's cmd-z
(`EditorModel::undo`, filter `origin == Origin::User`, `crates/fe-editor/src/lib.rs:990`) never touches it, and one
`buffer.undo(|o| o.is_ai())` ("undo AI") reverts the whole stream and restores the old target text. `redo` brings it
back. Write this in the module doc and in a short "Streaming" paragraph of the `//!` bullet.

Not in this card:

- growing a `ProposedChange` hunk by hunk as the agent streams (diff-block streaming);
- line-by-line replacement of the old text as new lines arrive;
- anything in fe-editor or fe-gpui.

## Tests

New file `crates/fe-buffer/tests/ai_stream.rs`, over the public API. Run the proptest with at most 64 cases so the file
stays fast. The exit test is a model check:

- user edits are `Origin::User` inserts and deletes at random offsets outside the region or at its edges, some
  inside an open `start_transaction(Origin::User)` as vim's insert session does;
- these are interleaved with pushes of random chunks;
- the expected text comes from a plain `String` model.

## Implementation notes

- `StreamError` has `Conflict` and `Undone`. The errors stick: once a push
  fails, every later push returns the error that killed the stream. Since
  `finish` takes the stream by value, a finished stream can't be pushed to, so
  no `Finished` variant is needed.
- After each push, both anchors are made again at the current version: the
  start `anchor_after` the region's start, the end `anchor_before` the end of
  the text just written. This also keeps a start anchor at an empty region from
  moving past a chunk inserted there.
- If the first chunk equals the old target text, `Buffer::edit` changes
  nothing and returns no transaction. The region then covers that text, and the
  next chunk that changes the text starts the transaction, set aside as the
  first one is.
- One ordering, which fe-buffer refuses safely: after undo-AI and then
  redo-AI, the user's undo of a transaction that inserted at the region's edges
  returns `UndoError::Conflict`. The redo reverts the batch whose
  replacement collapsed those positions, so the buffer stops trusting them
  (`AppliedBatch::collapsed_by`). The stream doesn't cause this, and the fix
  isn't part of this card. `typing_during_a_stream` therefore runs the scenario
  twice: in one run, undo-AI and redo-AI with the user's text kept; in the
  other, every user undo and then undo-AI and redo-AI.
