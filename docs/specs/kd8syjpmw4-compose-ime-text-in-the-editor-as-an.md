---
task_uid: "01m4p397vh9gvaqmkd8syjpmw4"
task: "T8"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test view::ime_tests::composing_leaves_the_buffer: in ab| marking k then か leaves the model text ab, the buffer version and undo history unchanged and no BufferEvent emitted; marked_text_range is 2..3, text_for_range(0..3) is abか, and selected_text_range is where the IME asked"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test view::ime_tests::commit_types_once: replace_text_in_range(None, 加) inserts at every caret through one recorded editor.insert and one undo step; unmark_text commits the composed text; an empty mark ends it with no edit; composing over a selection deletes it first as one undo step"
    required: true
    manual: false
  - id: "C3"
    text: "fe-gpui test view::ime_tests::painted_as_inlay: while composing, the frame's inlay holds the composition at the caret with an underline quad under its rect, the caret quad inside it at the IME's selected offset, the rest of the row shifted, and bounds_for_range of the composition inside that rect; and when the overflow fallback shifts the frame, the horizontal margin is capped to the available text width so the composition caret quad stays inside the text area, shown by a geometry test with a text area narrower than the four-em margin (F8). The long-row mid-composition slice case (F9) is a follow-up, not part of this criterion"
    required: true
    manual: false
  - id: "C4"
    text: "fe-gpui test view::ime_tests::dropped: a click, blur or a move command during a composition drops it with no edit; in vim normal mode composition calls change nothing; vim::insert_tests and input_method_utf16_offsets_with_emoji pass unchanged"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-gpui passes with only the mid-composition model-text assertion in crates/fe-gpui/src/tests.rs changed"
    required: true
    manual: false
---

# Compose IME text in the editor as an underlined inlay at the caret, editing the buffer only when it commits (Mac T429)

Filed on argon from Mac T429 (card ~/coordination/argon-cards/T429.json, Mac lead message ~/coordination/from-mac-lead-2026-10-11-argon-b.md). Mac task uid 01m4p30j7xjqvxgbyxdfep3r7j.

## Problem

The plan's M6 exit criterion is "Japanese IME works" (`~/.claude/plans/glittery-foraging-graham.md` line 110); the
2026-10-11 gap analysis found no card and no check. User 2026-10-11: "Draft any work in earlier phases we haven't
drafted yet."

`EditorView::replace_and_mark_text_in_range` (`crates/fe-gpui/src/view.rs:1056`) selects the marked range and calls
`model.insert` for every composition step, so each step is a real buffer edit: an undo entry (outside vim), a
`BufferEvent::Edited` (an LSP `didChange` for each romaji key), the dirty dot, a reparse, and the text shows in split
views mid-composition. And it's never painted as a composition: `crates/fe-gpui/src/element.rs` doesn't read
`marked`, so there is no underline and the user can't tell composing text from committed text.

## Change

Composition text stays out of the buffer until it commits, drawn as T389's inlay (`fe_display::layout::Inlay`,
`layout_frame_with_inlay`, `Frame::inlay`).

- New `crates/fe-gpui/src/view/ime.rs`: `Composition { at: Anchor, text: String, selected: Range<usize> }` (bytes
  within `text`) replaces `EditorView::marked`. It also owns the UTF-16 mapping of the composed document: the
  buffer text with `text` spliced in at `at`. Every `EntityInputHandler` method in view.rs (:997-1110) reads
  through it:
  - `replace_and_mark_text_in_range(range, text, selected)`: with `range` `None` or the marked range, sets the
    composition's text (starting one at the primary caret if there is none). A composition that starts over a
    non-empty selection first deletes it through `editor.insert` with "" (one undo step). A range covering buffer
    text (an IME reconverting it) deletes that text first the same way, then composes there. Empty `text` ends the
    composition with no edit. `selected` (UTF-16, relative to the composition) sets its caret.
  - `replace_text_in_range(None or the marked range, text)` commits: the composition is dropped, then
    `type_text(text)` runs (`editor.insert` at every caret, recorded as typing), so vim's insert session, macro
    recording (T231) and undo grouping see one typed run. A range elsewhere is replaced as today.
  - `unmark_text` commits the composition's text the same way.
  - `marked_text_range`, `selected_text_range` (the composition caret while composing), `text_for_range` and
    `character_index_for_point` answer in composed coordinates; `bounds_for_range` inside the composition answers
    from `Frame::inlay`.
- Paint (`crates/fe-gpui/src/element.rs`, the `layout::layout_frame` call at :514): with a composition, lay out
  with `layout_frame_with_inlay` and `Inlay { offset: at, text, style: the text style }`, draw an underline in the
  text colour under `Frame::inlay`, and draw the primary caret at the composition caret inside it. Other carets
  paint as usual.
- A click (`on_mouse_down`, which clears `marked` today at :360), blur, and any command that changes selections
  or text drop the composition with no edit.
- With vim on, `accepts_text` (`crates/fe-gpui/src/vim.rs:381`) still gates it: only insert mode composes.
- Ghost text (T390, not landed): the frame has one inlay. While composing, the composition owns it and ghost text
  isn't drawn; whichever of this card and T390 lands second wires that.

## Tests

New file `crates/fe-gpui/src/view/ime_tests.rs`, declared `#[cfg(test)]` from view.rs. In
`crates/fe-gpui/src/tests.rs`, change only the input-method test's assertion of the model text mid-composition
(:583, now "ab").

## Respec (rev 2, 2026-10-11, argon lead after the Mac lead's note argon-c)

Review round 4 fixed F3 and F7 and raised two new medium findings. This revision settles them:

- **F8 is in this task (C3).** When the overflow fallback shifts the frame so the composition caret is visible,
  cap the horizontal margin to the available text width (accounting for the caret's width), so the caret stays
  inside the text area even when the text area is narrower than the margin (four ems). Add a geometry test with a
  viewport whose text area is narrower than the margin, asserting the composition caret quad lies inside the text
  area.
- **F9 is a follow-up, out of scope here.** A wide composition in the middle of a row longer than
  MAX_SHAPED_ROW_BYTES can fall outside relayout's buffer-based shaped slice, so its inlay, underline and caret
  disappear. Don't fix it in this task; the review should file it as a follow-up.

## Implementation notes

- `EntityInputHandler` moved from view.rs into `view/ime.rs`, with every method reading through `Composed` (the
  buffer text with the composition spliced in). Without a composition it is the buffer text, so the old UTF-16
  helpers in view.rs went.
- Before a `RunCommand` runs, `end_composition_for` (the view's `on_action` listener) drops the composition if
  the command:
  - is an fe-editor command that validates and either mutates the buffer or moves carets (`editor.move*`,
    `editor.select*`, `editor.page*`, folds), including a move that can't go further, which edits nothing;
  - is one of vim's;
  - is `completion.accept`.
  Copy, save, toggling soft wrap, the hover and completion menu commands, and invocations the registry refuses all
  keep it. Clicks and `select_and_reveal` drop it directly. `changed` doesn't drop it, because commands that
  neither move nor edit (toggling soft wrap) call it too.
- With vim on, only insert mode composes. A composition records the vim switch's generation, and `composition()`
  reads as nothing outside insert mode or after the switch changed. Render and every handler that edits (after
  syncing vim) drop such a stale one, so a later return to insert mode doesn't bring it back.
- The composition caret is drawn at the end of the input method's selection, where `selected_text_range` puts the
  active edge (forward) and where autoscroll looks.
- A commit of "" while composing just ends the composition and repaints, with no `editor.insert`.
- Blur drops the composition in the existing `hover_blur` listener. GPUI fires focus listeners only for an active
  window, so `ime_tests::editor` activates its window.
- `FrameLayout::inlay_bounds` and `inlay_index_at` answer `bounds_for_range` and `character_index_for_point`
  inside the composition, from `Frame::inlay` and the row's shaped line. `FrameLayout` records which composition
  it drew. Until a frame draws the composition as it is now, both fall back to the buffer caret at `at`.
- fe-display's autoscroll keeps the buffer caret in view, and that caret is before the inlay. While composing,
  prepaint keeps the composition caret inside the right scroll margin on every frame. If the caret is past the
  margin, it first lays the frame out again, scrolled to the caret. Where that can't reach (wrapped rows, which
  the layout doesn't scroll, or a long row, whose scroll limit leaves the inlay out), it shifts the frame the
  rest of the way: `scroll.x` plus the precomputed rects (selections, cursors, inlay, fold markers). Everything
  else, painting rows and hit-testing included, follows `scroll`. Once the composition ends, the next layout
  scrolls as usual.
- The margin is four ems, capped to the text area's width but kept at least the caret's width. So in a text area
  narrower than the margin, the composition caret stops at the area's left edge rather than behind the gutter
  (`ime_tests::the_caret_stays_in_a_text_area_narrower_than_the_margin`).
- An invocation is checked as `run_command` would route it, before the composition is dropped. The context is
  fixed up the same way. With vim on, vim's commands are validated in vim's registry against the vim-stamped
  context, so `vim.move` in insert mode is refused and keeps the composition. With vim off they're unknown and
  keep it too. `completion.accept` is validated in the completion menu's context.
- The "composing over a selection" and reconversion checks are inside `ime_tests::commit_types_once`. The deletion
  and the commit are separate undo steps.
- `tests::injected_languages_compiled_meanwhile_draw_again` fails in a full `cargo test -p fe-gpui` run on this
  machine and passes run alone. It fails the same way on the base commit (b9a6340) without this change.
