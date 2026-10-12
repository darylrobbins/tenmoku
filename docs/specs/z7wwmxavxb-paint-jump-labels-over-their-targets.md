---
task_uid: "01m4nzg3xa0fky54z7wwmxavxb"
task: "T368"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-gpui test element::jump_tests::labels_at_targets: with vim on, a 40-row buffer in a 10-row window and gs plus two chars matching more targets than the safe labels, the laid-out frame has one label rect at each visible target's caret bounds, none outside the window, and a backdrop; after \u003cEsc\u003e it has neither"
    required: true
    manual: false
  - id: "C2"
    text: "fe-gpui test element::jump_tests::status_shows_query: the vim-mode status item reads JUMP ab while the jump waits for a label and NORMAL after it ends"
    required: true
    manual: false
  - id: "C3"
    text: "Colours come from existing EditorTheme fields; nothing under crates/fe-config, crates/fe-vim or kiln/ changes; the frame paints nothing new with no jump pending (the earlier element tests pass unchanged)"
    required: true
    manual: false
---

# Paint jump labels over their targets with a dimmed backdrop in the editor, and show the jump query in the status bar

# Jump labels painted in the editor: label boxes over the targets, a dimmed backdrop, and the query in the status bar

Phase 1b of the leap/flash plan (`leap-flash-design.md` §3A "Overlay for the host", §6 1b). The GUI half
of the jump: fe-vim's session card provides `VimState::jump_overlay` and `jump_query`. T230 makes
`ViewHost` answer `EditorHost::viewport()` from the laid-out frame, so the jump finds its targets in the
rows the user sees.

## Problem

With the fe-vim cards landed, `gs{c1}{c2}` labels targets that nothing draws. The user can't see a label,
the preview after `{c1}`, or what they've typed. The design calls for labels overlaid on the target chars
(leap's and flash's default: no layout change, no inlays) and the query in the status bar.

## Change

### Painting (`crates/fe-gpui/src/element.rs`)

- In prepaint, while the editor's `VimState` has a jump pending, call `jump_overlay(text, rows)` with the
  frame's visible buffer rows. For each label, lay out a rect at `layout.caret_bounds(offset)`, one char
  wide, as `match_rects` does for search matches. Cap it as `MAX_MATCHES` caps those.
- In paint, after the text and before the carets:
  - **Backdrop:** a translucent quad of the theme's background over the text area, dimming it while a
    jump is pending. It is always on, with no setting (the user's decision, 2026-10-11).
  - **`Live` labels:** a filled box in the theme's cursor colour with the label glyph in the background
    colour, painted over the char.
  - **`Preview` labels:** the same at reduced opacity.
  - **`Concealed`:** the box with no glyph, dimmed.
- Colours are derived from the existing `EditorTheme` fields, as `search_match_color` is ("until the
  theme has a slot for it"). Theme slots `editor.jump_label`/`jump_backdrop` are a follow-up with the
  T245–T247 theme work. Nothing in fe-config changes.
- Nothing paints with no jump pending, and the frame's other layout doesn't change.

### The status bar (`crates/fe-gpui/src/pane_item.rs`)

While a jump is pending, the vim-mode status item reads `JUMP` followed by the typed chars from
`jump_query()` (`JUMP`, `JUMP a`, `JUMP ab`). It goes back to the mode's label when the jump ends.

## Tests (new file)

`crates/fe-gpui/src/element/jump_tests.rs` (declared from `element.rs`):
- **Labels:** a GPUI test lays out an editor with vim on, a 40-row buffer and a 10-row window, then types
  `gs` and two chars that match more than the safe labels can take. One label rect sits at each target's
  caret bounds inside the window, and none sits outside it. After `<Esc>`, no label rects and no
  backdrop are laid out.
- **Status item:** the item reads `JUMP ab` while the jump waits and `NORMAL` after it.

The running-app check (labels visible, backdrop, preview after `{c1}`, a pick in normal and after `d`) is
a separate action task, filed once this card lands.

## Out of scope

- Theme slots, a backdrop setting, the fe-config setting for `s`/`S`.
- Cross-pane labels.
- Flash-style labels after the match.
- Inline labels (no inlays).

## Implementation notes

- `FrameLayout::jump` holds the pending jump's backdrop (the text bounds) and its labels, laid out in
  prepaint by `jump_layout` from `VimState::jump_overlay` over the frame's buffer rows. Each label is a
  `block_rect` (factored out of `block_carets`): from the target's caret bounds to the next grapheme
  boundary, as the block cursor is sized, so a wide char is covered whole.
- Alphas: backdrop 0.6 of the background, preview labels 0.5, concealed boxes 0.3 of the cursor colour.
- The status item's `JUMP` text comes from `status::jump_label`; `pane_item::status_items` is
  `pub(crate)` so the element tests can read it.
