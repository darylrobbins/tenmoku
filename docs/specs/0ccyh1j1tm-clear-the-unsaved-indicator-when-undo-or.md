---
task_uid: "01m4p1wp67v88fyh0ccyh1j1tm"
task: "T5"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "fe-buffer test saved_state::undo_to_the_save_point_is_clean: after a save, edits then undoing all of them leave is_dirty false; redo makes it true; undoing past the save makes it true; a freshly loaded buffer that is edited and fully undone is clean"
    required: true
    manual: false
  - id: "C2"
    text: "fe-buffer test saved_state::retyping_the_saved_text_is_clean: deleting a word and typing it back, replacing a char and then restoring it (same length), and editing both ends of the text and then reverting both all end with is_dirty false"
    required: true
    manual: false
  - id: "C3"
    text: "fe-buffer test saved_state::ai_proposal_edits_track_the_save: accepting a ProposedChange hunk (Origin::Ai) makes the buffer dirty, and undoing it with an Ai-origin filter makes it clean again"
    required: true
    manual: false
  - id: "C4"
    text: "fe-buffer test saved_state::save_moves_the_baseline: after did_save, undoing an earlier edit makes the buffer dirty; set_line_ending to CrLf makes it dirty and switching back to Lf makes it clean"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-buffer, the fe-gpui pane_item_tests and the fe-workbench close_tests and vim_host_tests pass unchanged"
    required: true
    manual: false
---

# Clear the unsaved indicator when undo or retyping makes the buffer match its last save (Mac T371)

Filed on helium from Mac T371 (~/coordination/helium-cards/T371.json; Mac lead message ~/coordination/from-mac-lead-2026-10-11-helium-a.md). Title, type, complexity, scope, criteria and labels as on the Mac.

## Problem

The user (2026-10-11): "the unsaved indicator should reset if you undo all your changes or make the file
contain the same text as the last save".

Dirty state is a version comparison: `Buffer::is_dirty` (`crates/fe-buffer/src/buffer.rs:255`) is
`self.version() != self.saved_version`, and `saved_version` is set only in `Buffer::did_save` (:310).
Versions only grow: undo and redo apply new batches (`apply`/`revert`, :775/:819) that make new versions.
So once you edit, the buffer stays dirty until you save, even after undoing back to the saved text or
retyping it. Every reader goes through this one method: `EditorModel::is_dirty`
(`crates/fe-editor/src/lib.rs:565`), `EditorView`'s `PaneItem::is_dirty`/`window_title`/`close_revision`
(`crates/fe-gpui/src/pane_item.rs:160-205`; kiln-shell draws the tab's dirty dot from `is_dirty`), the
close prompt, and vim `:q`/`:x`/`:qa` (`crates/fe-workbench/src/vim_host.rs`). They all poll it, so fixing
the buffer fixes the dot, the title's `●`, the close prompt and E37.

A version marker for the undo position can't do it: undo is selective by origin (`Origin::User`, `Ai`,
`Lsp`, ...), so there is no single linear undo position, and retyping identical text never returns to
one. Only a comparison of the text against the saved text can.

## Change

All in `crates/fe-buffer/src/buffer.rs`. No reader changes.

1. `Buffer` keeps the saved state: `saved_text: Text` (an O(1) rope clone that shares storage, so it
   holds only the leaves edited since), `saved_line_ending: LineEnding`, plus
   `changed_since_save: Option<Range<usize>>` and `dirty: bool`. `with_text` sets `saved_text` to the
   loaded text, `changed_since_save = None` and `dirty = false`, so a buffer read from disk starts clean.
   `did_save` sets `saved_text = self.snapshot.text.clone()`, `saved_line_ending = self.line_ending`,
   `changed_since_save = None` and `dirty = false`, and still records `saved_version` and emits
   `BufferEvent::Saved` as today.
2. Invariant: outside `changed_since_save` (in current coordinates), the current text and `saved_text`
   are byte-identical; `None` means they're equal. Add one private `note_batch(&mut self, edits: &[Edit])`,
   called once per applied batch next to each `emit_batch` call in `edit` (:403), `undo` (:551) and
   `redo` (:594). `edit_from` and `ProposedChange::accept*` reach `edit`, so AI/proposal edits are
   covered. For a batch spanning `old = first.old.start..last.old.end` and `new = first.new.start..last.new.end`
   with `delta = new.end - old.end` (signed), the range becomes `old.start..new.end` when there was none,
   else `min(h.start, old.start)..(max(h.end, old.end) + delta)`. Starts before the batch's first edit don't
   move, and the end is at or after its last edit, so it shifts by exactly `delta`.
3. Then recompute `dirty`: if `self.len() != self.saved_text.len()`, dirty (O(1), so ordinary typing that
   changes the length never compares text). Otherwise, with lengths equal, the range is the same in both
   texts; compare `self.text().chunks_in_range(r)` with `saved_text.chunks_in_range(r)` byte by byte with a
   small allocation-free chunk-walking helper `chunks_equal` in buffer.rs. Equal: `dirty = false` and
   `changed_since_save = None`. Cost is proportional to the changed region, not the file.
4. `is_dirty` returns `self.dirty || self.line_ending != self.saved_line_ending`: still O(1), no `Cell`,
   `Buffer` stays `Send`. `compact_log` and history trimming don't affect it (no version dependency).
5. Save-as and reload-from-disk don't exist today: a reload creates a new `Buffer` (clean by step 1) and any
   future save-as goes through `did_save`, so both are covered without code. Say so in `did_save`'s doc.

Memory: `saved_text` keeps the pre-save rope leaves alive until the next save or until the text becomes
clean again (step 3 also sets `saved_text = self.snapshot.text.clone()` when it finds them equal, so
undoing to the save point drops the old leaves). Worst case is one extra copy of the file after a
whole-file replace, the same order the undo history already holds (`DEFAULT_HISTORY_LIMIT`).

Tests go in a new integration test file `crates/fe-buffer/tests/saved_state.rs` using the public API only.
The existing `tests::dirty_tracking` (`crates/fe-buffer/src/tests/mod.rs:534`) stays as is and must pass.
