---
task_uid: "01m4p397rkrsczkxz88xjambns"
task: "T7"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-ui test tests::text_input_ime::composition_is_not_text: after marking に then 日本 on a TextInput holding ab, text() is ab, no TextInputEvent was emitted, marked_text_range is 2..4 and text_for_range(0..4) is ab日本"
    required: true
    manual: false
  - id: "C2"
    text: "kiln-ui test tests::text_input_ime::commit_emits_once: replace_text_in_range(None, 日本語) makes text() ab日本語 and emits Changed once; unmark_text commits; an empty mark ends the composition with no event; set_text and clear drop it"
    required: true
    manual: false
  - id: "C3"
    text: "kiln-ui test tests::text_input_ime::bounds_follow_the_text: bounds_for_range of the composition starts right of the committed text's shaped width, not at the element's left edge"
    required: true
    manual: false
  - id: "C4"
    text: "kiln-ui's existing text_input tests and fe-workbench's find_bar, search_panel, quick_open and vim_line tests pass unchanged"
    required: true
    manual: false
---

# Take IME composition in kiln-ui's TextInput, so pickers, find, search and the vim command line accept Japanese (Mac T430)

Filed on argon from Mac T430 (card ~/coordination/argon-cards/T430.json, Mac lead message ~/coordination/from-mac-lead-2026-10-11-argon-b.md). Mac task uid 01m4p30js517aczfgb579kdw0g.

## Problem

kiln-ui's `TextInput` refuses composition: its doc says "no caret movement, selection or IME composition (M6)"
(`kiln/crates/kiln-ui/src/text_input.rs:22`), `replace_and_mark_text_in_range` (:216) does nothing and
`marked_text_range` is `None`. So a Japanese IME can't type into anything built on it: Cmd-P and the other pickers
(`crates/fe-workbench/src/picker.rs`, `kiln/crates/kiln-shell/src/picker.rs`), the find bar (`find_bar.rs`), the
search panel (`search_panel.rs`) and the vim command line (`vim_line.rs`). Its `bounds_for_range` (:226) returns the
whole element's bounds, so a candidate window would sit at the field's left edge. M6's exit criterion is "Japanese
IME works" (`~/.claude/plans/glittery-foraging-graham.md` line 110). User 2026-10-11: "Draft any work in earlier
phases we haven't drafted yet."

kiln-ui's `TextField` (`text_field.rs`) does compose, but every `TextInput` user relies on its append-only model and
events; this card adds composition to `TextInput` rather than moving them.

## Change

All in `text_input.rs`; update its doc comments.

- A composition after the committed text: `marked: Option<(String, Range<usize>)>` (text, selected bytes within
  it). `TextInput` only appends, so it is always at the end.
- `replace_and_mark_text_in_range`: sets the composition's text and selection; empty text ends it with no event.
  Ranges are read in committed+composition UTF-16 coordinates.
- `replace_text_in_range` with `None` or the marked range commits: appends the text's first line, emits
  `Changed` once, ends the composition. Any other range: as today, and the composition is dropped.
- `unmark_text` commits the composition's text.
- `marked_text_range` is the composition's UTF-16 range after the text; `selected_text_range` is inside it while
  composing; `text_for_range` reads committed+composition.
- `text()` excludes the composition, and no `Changed` is emitted while composing, so pickers don't refilter on
  each romaji key. `set_text` and `clear` drop it.
- Render the composition after the text, underlined in the text colour, with the caret after it.
  `bounds_for_range` for the composition starts at the element's left padding plus the committed text's shaped
  width (`window.text_system().shape_line` in the element's font and size), so the candidate window sits under it.

## Tests

New file `kiln/crates/kiln-ui/src/tests/text_input_ime.rs`, with `mod text_input_ime;` in
`kiln/crates/kiln-ui/src/tests/mod.rs`.
