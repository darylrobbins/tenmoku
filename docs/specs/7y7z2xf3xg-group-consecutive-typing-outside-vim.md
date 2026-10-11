---
task_uid: "01m4p6c449srvcw57y7z2xf3xg"
task: "T16"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test typing::tests::one_step: editor.insert of h, e, l, l, o from the user within 1 s on the injected clock undoes as one step, leaving the caret where typing began, and redoes as one"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test typing::tests::breaks: a caret move, a 1 s pause, and a newline each start a new step; typing foo space bar undoes bar then foo-space; a paste is its own step"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test typing::tests::deletes_group: three backspaces undo as one step, and typing after them starts a new one"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test typing::tests::other_origins: an Origin::Ai edit elsewhere between keystrokes keeps the group (undo removes only the typed text and keeps the AI edit); a user edit through a split model of the buffer, an undo and a save each end it"
    required: true
    manual: false
  - id: "C5"
    text: "Typing with three carets groups as one step per run; with a transaction open (start_transaction(Origin::User)) typing joins it and no group is kept; cargo test -p fe-editor -p fe-vim -p fe-buffer and the fe-gpui vim::insert_tests pass unchanged"
    required: true
    manual: false
---

# Group consecutive typing outside vim into one undo step, broken by a caret move, a 1 s pause, a new word or a line break (Mac T428)

Filed on helium from Mac T428 (~/coordination/helium-cards/T428.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-c.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

Outside vim, every keystroke is its own undo step: `EditorModel::insert` (`crates/fe-editor/src/lib.rs:856`) goes
through `apply_with_carets` (:1112), which calls `Buffer::edit` once per keystroke, a transaction each. Only vim's
insert session (T162) and completion group edits, through `Buffer::start_transaction`. Typing "hello" and pressing
cmd-z removes one letter. The plan's M6 ("Editing polish: multi-cursor, IME, undo, ...",
`~/.claude/plans/glittery-foraging-graham.md` line 110) and its reuse map list "undo coalescing"; the 2026-10-11 gap
analysis found no card. User 2026-10-11: "Draft any work in earlier phases we haven't drafted yet."

Holding a transaction open across keystrokes isn't safe: `Buffer` has one open-transaction slot, another origin's
`start_transaction` trips a `debug_assert`, and undo closes it. T383 adds `Buffer::edit_in`, which appends a batch
to an earlier, closed transaction while other origins edit; this card builds on it.

## Change

New module `crates/fe-editor/src/typing.rs` (`mod typing;` in lib.rs) holding the model's typing group, with hooks
in lib.rs's `insert`, `tab`, `backspace`, `delete_forward`, `delete_word_left`, `undo`, `redo` and
`apply_with_carets`. In `crates/fe-buffer/src/buffer.rs` add only
`pub fn open_transaction(&self) -> Option<TransactionId>`.

A typing group is: its transaction id, its kind (`Insert` or `Delete`), the carets it left (anchors), when its last
edit landed, and whether its last typed char was whitespace. An edit joins the group, through
`Buffer::edit_in(group, edits)`, when all of these hold; otherwise it starts a new group with a plain `edit`:

1. It is an `insert` whose text has no line break, or a `tab` (both kind `Insert`), or a `backspace`,
   `delete_forward` or `delete_word_left` on carets only (kind `Delete`), and its kind is the group's.
2. Every selection is a caret, and the carets are exactly where the group left them (so any caret move, click,
   or selection change ends it).
3. No edit of this model's origin reached the buffer since the group's last edit, from anywhere: another model
   of the buffer (a split), a command, a format. Edits of other origins (LSP, AI) don't end it.
4. Less than `TYPING_PAUSE` = 1 s since the group's last edit, on the model's clock:
   `EditorModel::set_clock(Rc<dyn Fn() -> Instant>)`, default `Instant::now`, so tests are deterministic.
5. `buffer.open_transaction()` is `None`. While one is open (vim's insert session, completion), edits join it as
   they do today, and no group is kept.
6. For `Insert`: the text doesn't start a new word, i.e. not (its first char is non-whitespace and the group's last
   typed char was whitespace). "foo bar" is two steps: "foo " then "bar".

Everything else that edits through `apply_with_carets` (newline, paste, cut, completion, the multi-cursor and
other commands) ends the group and is its own step; `newline` always is. `undo`, `redo` and a save
(`BufferEvent::Saved`) end the group. If `edit_in` refuses (`EditError::NoSuchTransaction`), start a new group.

An extended edit pushes the group's transaction id into `self.transactions`, as `apply_with_carets` does today, and
places carets as it does. Undoing a group leaves the carets where the run began (History's caret picks treat it as
one transaction).

## Tests

New file `crates/fe-editor/src/typing_tests.rs`, declared from typing.rs
(`#[cfg(test)] #[path = "typing_tests.rs"] mod tests;`), driving the model and `commands::registry()` with
`HeadlessHost` and an injected clock.

## Implementation notes

- The group holds its own `Subscription` to the buffer, taken when it starts and drained after each of its own
  `edit_in`s; any `Edited` event of the group's origin (another model, `buffer_mut`, an undo or redo anywhere) or a
  `Saved` ends it. `sync_display` polls it, so events don't pile up and a paused group is dropped.
- Condition 2 ("any caret move ends it") is enforced in two ways: `set_selections`, which every move, click and
  selection change goes through, ends the group even if the carets land where they were (left then right), and
  the carets are compared with those the group left, as resolved now.
- Outside this card's scope list, two files changed: `ghost_text.rs`'s accept goes through
  `insert_typed(_, None)`, so accepting a suggestion stays its own step rather than joining the typing before it;
  and `tests/ghost_text.rs::accept_is_one_user_undo_step` now expects the two chars typed before accepting to undo
  as one step (it asserted the old one-char-per-step undo).
- A typed insert replacing a selection starts a group (typing over a selection then more chars is one step);
  backspace and delete over a selection, and cut, are their own steps.
