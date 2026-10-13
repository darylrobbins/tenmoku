---
task_uid: "01m4s7vkqqrehxa5bcqxqp7xck"
task: "T580"
rev: 3
type: "bugfix"
complexity: "m"
criteria:
  - id: "C1"
    text: "A direct target's char stays readable: nothing opaque is painted over its glyph, or it is repainted on top at \u003e= 4.5:1 contrast, in the default dark and light themes; direct targets are still marked distinctly (jump_direct_tests passes)"
    required: true
    manual: false
  - id: "C2"
    text: "Every label glyph painted (preview and live) has \u003e= 4.5:1 contrast with its box in the default dark and light themes"
    required: true
    manual: false
  - id: "C3"
    text: "The letters drawn after {c1} and the session's labels after {c2} are worked out over the same rows: each target's letter after {c1} equals its live label after {c2}, including with a partly visible bottom row and with a fold in view"
    required: true
    manual: false
  - id: "C4"
    text: "Through the real GPUI key path (simulate_keystrokes into a drawn EditorView with vim on), g s {c1} {c2} then a label moves the caret to that label's target, both when labels wait and after an autojump, including when the label is the one shown after {c1}"
    required: true
    manual: false
  - id: "C5"
    text: "Every existing jump test passes: fe-vim jump_spec and jump_direct, and fe-gpui jump_tests, jump_style_tests and jump_direct_tests"
    required: true
    manual: false
---

# Fix vim jump labels: the direct-target marker hides its char, and the letters shown after {c1} differ from the labels after {c2}

User report (running app, vim): "vim leap isn't actually working. The first few targets have a pink border with no visible letter, and even when I enter a valid letter target, the cursor doesn't move there."

Diagnosis (lead's subagent, confirmed with scratch fe-gpui tests and real-Metal screenshots):

1. Labels not visible. After {c1}, the nearest match of each {c2} group is a Direct target: jump_overlay gives it empty text (crates/fe-vim/src/jump.rs:695). paint_jump then draws a 2px jump_label (pink) border inside that char's own ~7.8px cell, over the backdrop-dimmed glyph (crates/fe-gpui/src/element.rs:1837-1856), so the char is unreadable. Theme colours themselves have enough contrast.

2. Typing a shown letter doesn't move the caret:
 (a) After {c1} the overlay paints letters, but the next key is always read as {c2} (jump.rs:498): typing a shown letter there jumps to {c1}{letter} or fails. Leap semantics stay: {c1} {c2} then label; the preview letters after {c1} must be what you type after {c2}.
 (b) The preview letters after {c1} are worked out over the frame's laid-out rows (element.rs:1738-1742), which include a partly visible bottom row (and, from the code, folds); the session after {c2} labels only the host viewport (jump.rs:308-309), whole display rows counted as buffer rows (view.rs:478). Near 18 targets one side autojumps with the SAFE label set and the other uses the full set, so the letters differ. Repro: window 10.5 rows tall, after {c1} target at row 12 shows "j"; after {c2} it is labelled "n"; typing "j" leaves the caret at row 9.

Existing gap: fe-gpui jump_tests.rs status_shows_query picks a label but checks only the status text, never the caret.

Put the new tests in the existing crates/fe-gpui/src/element/jump_tests.rs (it is overlaid at base for the regression check, so it must compile against the base API: drive keys and inspect paints/caret only; reuse jump_style_tests' editor/draw/paints helpers by copying what is needed). Do not declare a new test module from element.rs for the regression tests: Regatta's base overlay would not compile it.

## Implementation notes

- Direct target: its laid-out rect is its char's block grown by the 2px
  border on every side, so the border rings the char from outside.
  `JumpPaint::Direct` keeps its shape (bounds, `jump_label`, 2px), so
  jump_direct_tests is unchanged. The border is painted as edge strips less
  every other direct target's char block and every label's box
  (`direct_ring`; review F1), so targets side by side or a row apart don't
  cover each other's chars or labels: their borders merge.
- Rows: `fe_vim::jump::view_rows(model, viewport, top_display_row)` maps a
  `Viewport` to the text rows its display rows show, counted from the
  display row the view really starts at (partway down a soft-wrapped top
  row; review F2). A fold or a wrapped row counts as display rows, not text
  rows. The session (`vim.jump`) takes the viewport and the new
  `EditorHost::top_display_row` (default `None`; the GPUI host's comes from
  `EditorView::page_rows`, the frame's `first_row` or a vim scroll's) from
  the host; `jump_layout` passes the frame's own viewport and `first_row`,
  what the view reports once the frame is stored. A partly shown last row
  is outside both. Rows a fold hides take no label in either
  (`label::preview` takes a `shown` row filter; the session drops hidden
  targets).
- `HeadlessVim::jump_overlay` uses `view_rows` too (review F3), with no
  display start: its host doesn't wrap.
- C2 already held at base (the theme colours pass); the test guards it.
