---
task_uid: "01m4p2a15dt4x2g28vc7x7bhca"
task: "T13"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test tests::ghost_text::typing_consumes_the_prefix: with \"foo()\" at the caret, typing f then o leaves remaining \"o()\", backspace shows \"oo()\" again, and typing x ends it (None)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test tests::ghost_text::survives_other_edits: Origin::Ai edits before the anchor and after the caret leave remaining unchanged, and accept inserts at the shifted caret"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test tests::ghost_text::accept_is_one_user_undo_step: accepting a 3-line suggestion after 2 typed chars inserts the rest as one Origin::User transaction, model.undo() removes exactly it and restores the caret, and inside an open Origin::User transaction it joins that one"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test tests::ghost_text::accept_word_and_line: accept_word takes the next word with the punctuation or spaces before it, accept_line takes everything through the next newline, and the rest stays live"
    required: true
    manual: false
  - id: "C5"
    text: "fe-editor test tests::ghost_text::ends_without_editing: a caret left of the anchor, a second caret, a selection, another buffer or an anchor below oldest_version gives None, and accept returns None leaving text and version unchanged"
    required: true
    manual: false
---

# Add GhostText to fe-editor: an anchored inline suggestion that typing its prefix consumes, accepted whole, by word or by line as the user's undoable edit (Mac T388)

Filed on helium from Mac T388 (~/coordination/helium-cards/T388.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-b.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

M8 of the plan (`~/.claude/plans/glittery-foraging-graham.md`) lists "ghost text" among the AI seams, and the
user asked on 2026-10-11 to go ahead with the M8 cards: inline AI suggestions shown as dimmed text at the caret,
not part of the buffer, anchored so they survive concurrent edits; typing a char that matches the suggestion
consumes it; Tab accepts it as one undo step; Esc or moving away dismisses it. `rg -i ghost crates` finds
nothing. fe-editor already has the shape this needs in `crates/fe-editor/src/completion.rs`
(`CompletionSession`: a GPUI-free session the view holds, `live()` re-derived from the model each time, and
`accept` as one `Origin::User` transaction that joins an open one, as vim's insert session holds).

Undo origin: the user decided on 2026-10-09 (T257, `docs/decisions/command-model.md` § "Accepting") that
accepting an AI's edit in the editor is the user's own edit, on the user's undo stack, because
`EditorModel::undo` (`crates/fe-editor/src/lib.rs:990`) undoes only `Origin::User` transactions. Ghost text
follows that: it is accepted under `Origin::User`, and records the suggesting `Origin::Ai` only as who proposed it.

## Change

New module `crates/fe-editor/src/ghost_text.rs`, declared `pub mod ghost_text;` in `crates/fe-editor/src/lib.rs`
(that line only). GPUI-free.

- `GhostText::new(snapshot: &BufferSnapshot, offset: usize, text: impl Into<Arc<str>>, origin: Origin)`.
  It stores the buffer id, `snapshot.anchor_before(offset)` (typing at the offset goes after it), the text
  (any number of lines) and the origin. `text()`, `origin()` (who suggested it), `version()` (the anchor's
  version) and `refresh(&mut self, snapshot)` (rebases the anchor with `BufferSnapshot::refresh`, as
  `ProposedChange::refresh` does, so the owner can keep `compact_log`'s floor at or below `version()`).
- `remaining(&self, model: &EditorModel) -> Option<&str>`: the part still to show, or `None` when the
  suggestion is over. It is live only when: the model's buffer is the suggestion's; its anchor's version is not
  below `oldest_version()` (never panic); there is exactly one selection and it is empty; the caret `c` is at or
  after the anchor `a`; the buffer text `a..c` equals a prefix of `text` that ends on a char boundary and is
  shorter than `text`. Then it returns `&text[c - a..]`. So typing a matching char consumes it, deleting back
  toward the anchor shows more again, and a mismatched char, a caret moved away, a second caret or a selection
  ends it. `is_live(&self, model) -> bool`.
- `accept(&self, model: &mut EditorModel) -> Option<TransactionId>` inserts all of `remaining` at the caret
  through the model's own edit path (as `EditorModel::insert` does), so it is one `Origin::User` transaction,
  joining a transaction already open (vim's insert session) as typing does; the caret ends after it, and
  `model.undo()` removes exactly the inserted text and puts the caret back. `None` and no edit when not live.
- `accept_word(&self, model)` inserts the leading run of non-word chars plus the following run of word chars
  (`CharClass`), stopping before a `\n` unless `remaining` starts with one; `accept_line(&self, model)` inserts
  up to and including the next `\n`, or all of `remaining`. Both are one transaction each, and the suggestion
  stays live with what's left.

Dismissal is the owner dropping the value; there is no dismissed state.

Not in this card: painting, keys, commands or events (fe-gpui cards that follow); merging a suggestion's suffix
with text already after the caret (such as a closing bracket).

## Tests

New file `crates/fe-editor/src/tests/ghost_text.rs`, declared from `crates/fe-editor/src/tests/mod.rs` (one
`mod` line), using its `parse` helper.

## Implementation notes

- Accepting runs `EditorModel::insert` under `run_as(Origin::User, …)`, so it is the user's edit even when a
  command running as an AI accepts it; the transaction is also reported to that outer command.
- `refresh` is a no-op for a snapshot of another buffer, or one that can't map the anchor (below its floor or
  older than the anchor), rather than panicking as `ProposedChange::refresh` does.
- A model sees a `compact_log` floor only once it catches up with a later edit: its snapshot keeps its own log
  until then, so the suggestion stays live against it until that edit.
