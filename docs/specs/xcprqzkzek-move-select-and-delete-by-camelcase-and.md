---
task_uid: "01m6ezv80yjjtqgxxcprqzkzek"
task: "T411"
rev: 5
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Test subword::tests::moves_stop_at_each_part (cargo nextest run -p fe-editor) passes: fooBarBaz, HTTPServer, utf8Decode, snake_case_name, digit runs, punctuation and row ends, left and right, with the boundaries the spec lists"
    required: true
    manual: false
  - id: "C2"
    text: "Test subword::tests::select_and_delete_variants_follow_the_moves (cargo nextest run -p fe-editor) passes: select_subword_left/right extend, delete_subword_left/right delete exactly the moved-over span, with several carets, a selection (deleted whole), and without splitting a grapheme cluster"
    required: true
    manual: false
  - id: "C3"
    text: "Test subword::tests::deleting_a_subword_in_a_linked_range_mirrors_and_keeps_the_primary (cargo nextest run -p fe-editor) passes"
    required: true
    manual: false
  - id: "C4"
    text: "cargo nextest run -p fe-keymap --test subword_keys passes: on macOS ctrl-alt-left/right, ctrl-alt-shift-left/right, ctrl-alt-backspace and ctrl-alt-delete resolve to the six commands in context editor, the deletes are unbound in vim normal mode, and off macOS none of them is bound while ctrl-alt-left still resolves to pane.move_tab_left"
    required: true
    manual: false
  - id: "C5"
    text: "cargo nextest run -p fe-workbench passes, with the six new command ids in command_catalog_tests.rs's lists and every other test there unchanged"
    required: true
    manual: false
---

# Move, select and delete by camelCase and snake_case parts with editor.*_subword_* commands, ctrl-alt-arrows on macOS (Mac T1318)

## Problem

Outside vim there is no way to move or delete by the parts of camelCase, PascalCase, snake_case or digit-joined names.
VS Code has cursorWordPartLeft/Right (+Select) and deleteWordPartLeft/Right. `editor.move_word_left` and
`delete_word_left` (crates/fe-editor/src/lib.rs `next_word`/`prev_word`, delete_extent.rs) stop only at CharClass
changes. The user approved this on 2026-11-02. The vim subword cards (T1290, T1293, T1294) are blocked and live in
fe-vim's own feature/subword.rs; this card does not depend on them.

## Change

- New crates/fe-editor/src/subword.rs: `EditorModel::next_subword` / `prev_subword` (row-bounded like `next_word`:
  at a row end they step over the break) and `delete_subword_left` / `delete_subword_right` (via `delete_with` and
  `whole_graphemes`, as `delete_word_right`, so every cursor, linked ranges and primary identity work as for word
  deletion). Boundaries inside a run of letters/digits/`_`: lower->Upper (`foo|Bar`), the last capital of a capital
  run before a lowercase (`HTTP|Server`), letter<->digit (`utf|8|Decode`), and each `_` run, which is skipped as
  whitespace is (moving right from `snake|_case` lands after `case`). Non-word runs and whitespace behave as word
  motion. Unicode case via `char::is_uppercase`/`is_lowercase`.
- `Movement::SubwordLeft/SubwordRight` (crates/fe-editor/src/lib.rs, `Movement` and its match, plus viewer.rs's match
  so it still compiles; the read-only viewer treats them as WordLeft/WordRight) and `Motion` variants in commands.rs.
- Commands in crates/fe-editor/src/commands.rs: `editor.move_subword_left/right`, `editor.select_subword_left/right`,
  `editor.delete_subword_left/right`, titled "Move Subword Left" etc.; add them to
  crates/fe-workbench/src/command_catalog_tests.rs's lists.
- Keys in crates/fe-keymap/keymaps/fastedit.toml, macOS only: ctrl-alt-left/right (move), ctrl-alt-shift-left/right
  (select), ctrl-alt-backspace / ctrl-alt-delete (delete), context `editor`, the deletes with alt-delete's
  `(!vim.enabled || vim.mode == insert || vim.mode == replace)` guard. Off macOS they are unbound, because ctrl-alt-left
  and right are pane.move_tab_left/right there and ctrl-alt-backspace is a desktop chord; users can bind the commands.
- Tests in new files: crates/fe-editor/src/subword_tests.rs (declared from subword.rs) and
  crates/fe-keymap/tests/subword_keys.rs (modelled on delete_extent_key.rs).

## Implementation notes

- Both moves stop where one predicate on the text says (`splits` in subword.rs), so a caret inside a part looks past
  itself going left: `HTTPS|erver` moves to `HTTP|Server`. The stops differ by direction only around `_`, as specified.
- Letters with no case split from neither case, since the spec's boundaries name only lower->Upper: `日本語Foo` is one
  part, `日本語|8` is two.
- The scope grew by what the new commands and keys had to touch: `whole_graphemes` became `pub(crate)` in
  delete_extent.rs, and the tests that pin the command and key lists gained entries (fe-editor tests/commands.rs and
  tests/viewer.rs, fe-gpui tests.rs and vim/tests.rs, fe-workbench tests/default_keymaps.rs). `MOVES` in tests/viewer.rs
  checks parity with the editor, which subword moves deliberately lack, so they are listed apart from it.
- The subword deletes widen selections to whole graphemes as well as caret ranges, because selecting by subword can stop
  at a combining mark. They build their own edits for that: `delete_with`, which other commands share, is unchanged.
- The four move and select chords hold in vim normal mode, as the spec guards only the deletes; vim/tests.rs lists them.
- In vim insert mode the deletes split the insert session, as `delete_word_right` does: fe-gpui's `TEXT_INPUT` lists only
  `delete_word_left`, and it stays unchanged here.
