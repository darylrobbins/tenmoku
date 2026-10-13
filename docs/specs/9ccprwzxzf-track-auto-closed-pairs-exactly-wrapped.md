---
task_uid: "01m4sjp2csvc5wb49ccprwzxzf"
task: "T85"
rev: 2
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-editor test pairs::tests::wrapped_pair_types_over_and_backspaces: after [ wraps a selected foo and the selection collapses right, typing ] moves over the closer ([foo]| with no second ]); deleting the wrapped text then backspace removes the empty [] (F2)"
    required: true
    manual: false
  - id: "C2"
    text: "fe-editor test pairs::tests::edited_half_is_not_typed_over: a closer replaced by the same char through model.insert, or a half changed and restored before a sync, is typed rather than moved over, and backspace between such a pair deletes one char (F3)"
    required: true
    manual: false
  - id: "C3"
    text: "fe-editor test pairs::tests::leaving_the_pair_forgets_it: set_cursor to another row and back with no sync, or one of two carets on a row leaving its pair and returning, makes typing ) insert a closer (F4)"
    required: true
    manual: false
  - id: "C4"
    text: "fe-editor test pairs::tests::more_than_64_carets_keep_their_pairs: with 65 carets, ( then ) gives () at every caret, and ( then backspace leaves nothing at every caret; MAX_TRACKED is gone (F5)"
    required: true
    manual: false
  - id: "C5"
    text: "pairs::tests and fe-gpui commands::pairs_host_tests pass unchanged; cargo test -p fe-editor -p fe-gpui -p fe-workbench and cargo clippy on those crates --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Track auto-closed pairs exactly: wrapped pairs, edited halves, a caret leaving the pair, and more than 64 carets (T69 review F2-F5) (Mac T477 follow-up)

Follow-up from T69 (Mac T477, auto-close pairs, landed as 5298ee4), filed on helium. Review 01m4sf07t4srn20cd6g5rtmd8m
(`rgt review show T69`), findings F2 to F5. All four are in how `crates/fe-editor/src/pairs.rs` tracks the
closers it inserted.

## Problem

- **F2 (pairs.rs :161).** The `Typing::Wrap` branch of `EditorModel::insert_typed` inserts a closer but never pushes an
  `AutoClosed`. After wrapping a selected `foo` with `[` and collapsing to the right, typing `]` gives `[foo]]`.
  Deleting the selected text then leaves an empty `[]` that backspace can't remove as a pair.
- **F3 (pairs.rs :55).** `AutoClosed::resolve` checks only that the anchored ranges still hold the right chars,
  so it can't tell a closer that was never touched from one that was edited. Example: create `()`, select the `)`,
  replace it with `)` through `model.insert`, and put the caret before it. Typing `)` still moves over it. An edit
  that changes a half and then restores it before the next sync also brings the pair back. The spec allows
  type-over only for closers that are still untouched.
- **F4 (lib.rs :349, pairs.rs :255).** `forget_auto_closed` runs only from `sync_display`, and it keeps a pair while
  any caret is on its row. So `set_cursor` to another row and back before a sync keeps the pair. With several
  carets, another caret left on the original row also keeps the departed caret's pair.
- **F5 (pairs.rs :12, :188).** `MAX_TRACKED = 64` drops pairs that are still live. With 65 carets, typing `(` pairs
  all of them, but typing `)` then inserts a second closer at the first caret. Backspace leaves a dangling closer
  there too.

## Change

All of this is in `crates/fe-editor/src/pairs.rs`, plus selection hooks in `crates/fe-editor/src/lib.rs`.

- **Wrap tracks its pair.** The Wrap branch records an `AutoClosed` for the opener and the closer it inserted, the
  same way Open does. Type-over and empty-pair backspace then treat it like any other pair.
- **Any edit to a half retires the pair, for good.** Each `AutoClosed` records the buffer version it was made or
  last refreshed at. When pairs are checked (`forget_auto_closed`, and before `typing_at` or `empty_pair_at` use
  them), every edit since that version (`BufferSnapshot::edits_since`) is inspected. A pair is dropped if any of
  those edits' old ranges overlaps or replaces its opener or closer, even when the resulting char matches again.
  Text inserted between the halves, or at their outer edges, doesn't count.
- **A pair lives while a caret is inside it.** A pair is kept only while some caret's head is between its opener's
  end and its closer's start, inclusive. That is checked on every selection change, not only at sync:
  `set_selections` and every other place lib.rs assigns `self.selections` call `forget_auto_closed` (or a cheaper
  check of the same rule). Leaving the pair, to another row or not, forgets it, even if the caret comes back
  before a frame. Another caret elsewhere on the row doesn't keep it alive.
- **No cap.** Remove `MAX_TRACKED` and the drain in `insert_typed`. The rule above bounds the list to pairs that
  contain a caret.

Existing tests in `pairs_tests.rs` must pass unchanged.

## Tests

Add the tests to the existing `crates/fe-editor/src/pairs_tests.rs` (already declared from pairs.rs at base as
`mod tests`), leaving its existing tests unchanged, using `crate::tests::{parse, render}`. Not a new file: Regatta's
regression check applies only test files to base, so a test module declared in pairs.rs can't run there.

- `wrapped_pair_types_over_and_backspaces` (F2)
- `edited_half_is_not_typed_over` (F3: replace the closer with the same char, and change then restore a half
  before a sync; in both cases `)` is typed, not moved over, and backspace deletes one char)
- `leaving_the_pair_forgets_it` (F4: set_cursor to another row and back with no sync, and with two carets on one
  row, moving one out and back; in both cases its `)` is typed, not moved over)
- `more_than_64_carets_keep_their_pairs` (F5: 65 carets, `(` then `)` gives `()` at each; again with backspace
  after `(`, which leaves nothing at each)

## Implementation notes

- `AutoClosed` now holds plain offsets and the version they're in, not anchors. `advanced` walks
  `edits_since(version)` batch by batch: an edit whose old range overlaps a half (`old.start < half.end &&
  old.end > half.start`, which also catches an insertion strictly inside a multi-byte half) drops the pair;
  otherwise each half shifts by the edits ending at or before its start, so an insertion at the opener's start or
  the closer's start moves that half and one at a half's end doesn't. A pair whose version is below the log's
  `oldest_version` is dropped too. The one-row check stays.
- `forget_auto_closed` keeps a pair only while some selection head is in `opener.end..=closer.start` (binary
  search over the sorted heads), and leaves the list sorted by closer start, so `typing_at` and `empty_pair_at`
  find their pair by binary search instead of scanning: typing at thousands of carets stays O(n log n). It runs
  from `sync_display` as before, plus after every assignment to `self.selections` in lib.rs (`set_selections`,
  undo/redo's carets, `apply_selecting`), and at the end of `insert_typed` to bring new pairs up to the snapshot.
  `rows.rs` assigns selections after a row move too, but that edit replaces the rows' text, so it already retires
  any pair on them.
- F3's model.insert case passes through the caret rule alone (the insert leaves the caret after the closer), so
  the test also replaces the closer through a split model and changes and restores a half through `buffer_mut`
  while the caret stays inside; those exercise the edit rule on its own. All four new tests fail at base.
