---
task_uid: "01m743s6vsy05qvwfcssay83h4"
task: "T574"
rev: 3
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "The editor's text area is one node built by a pure a11y_text::build function and pushed into the A11ySubtreeBuilder by a thin adapter, with the node placement, text runs, cap, selection projection and geometry section 4's decisions specify"
    required: true
    manual: false
  - id: "C2"
    text: "Shape 1 tests in crates/fe-gpui/src/element/a11y_text_tests.rs load the result in accesskit_consumer (=0.38.0) and cover every case section 9 lists: folds, soft wrap, CRLF, non-BMP characters, the cap, the empty file, vim's collapsed selection, the selection projection cases (Select All over the cap, endpoints above and below the window, a caret scrolled off, an endpoint in a fold or past a truncated row, none panicking), geometry and run order; all pass"
    required: true
    manual: false
  - id: "C3"
    text: "The a11y_text bench in crates/fe-gpui/benches/a11y_text.rs runs, and its numbers on a large file against the budget section 4 sets are in the finish notes"
    required: true
    manual: false
  - id: "C4"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-7 Editor text area: one AccessKit text node with runs, selection and geometry from a pure builder, tested against accesskit_consumer (T294 design) (Mac T1673)

# A11Y-7 Editor text area

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-7 Editor text area** row of section 11 "Proposed card breakdown", and section 4 "Editor text area" (all of its decisions) and section 9 shape 1.
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK; behaviour is checked on the Mac by A11Y-11**.

## Implementation notes

- **Placement.** The editor's focus div (`view.rs`, `div().id("editor")`) is a `Group` named "Editor" (`gpui-a11y-editor`, which is why `crates/fe-gpui/l10n/en-US/fe-gpui.ftl` joined the scope). Inside it, `EditorView::text_area` wraps `EditorElement` alone in `div().id("editor-text").size_full()`: a `MultilineTextInput` named for the file (the tab title), with `aria_active_descendant()` while the editor is focused (completion open or not), and, while `window.is_a11y_active()`, `a11y_synthetic_children` and an `on_a11y_action(SetTextSelection)` handler (`element::attach_a11y_text`). Both names go through A11Y-2's `fe_ui::node`, so the recorder sees them.
- **Builder.** `crates/fe-gpui/src/element/a11y_text.rs`, declared from `element.rs`: `a11y_text::build(Input { display, frame, long_rows, widths, tab_size, anchor, head, origin, scale, cap, read_only }) -> A11yText`. It names nothing of the editor, so the file viewer (A11Y-8) can call it as it is; the bench compiles the file on its own, which is why its tests are declared from `element.rs`. `EditorElement::prepaint` builds it at the end of prepaint, only while a11y is active, into the view's `A11yTextCell` (`Rc<RefCell<…>>`); otherwise the cost is the flag and one cell write a frame. `A11yText::write` sets the selection and `read_only` on the text node and gives the run nodes in display order; the adapter takes their ids from `synthetic_node_id((buffer_row, segment))` and keeps id → run for `SetTextSelection`, which maps to buffer offsets and sets the primary selection (a caret when collapsed) through vim's `split_session`, scrolled into view.
- **Runs, window, cap, selection, geometry** as section 4 decides: one unlinked run a display row; a line's last segment ends in its break (`\n`, or `\r\n` as one two-byte character for a viewer source; the editor's `Text` normalises CRLF files to `\n`); fold headers without the marker; visible rows always (cut at a grapheme boundary of the whole row when the byte cap runs out), then margin rows alternately below and above, up to a viewport's worth each side, clipped to the file, stopping at 300 rows or 64 KiB; the five projection rules; bounds, positions and widths on every run (visible rows from the shaped line, margin rows from `CharWidth` sums with tab stops, at `frame.row_top`), the break a zero-wide position at the text's end, empty runs zero-wide at the row's start, all scaled to device pixels as GPUI's own node bounds are.
- **A character is a grapheme cluster**, as in A11Y-5 and as the caret moves; one over 255 bytes is split at char boundaries.
- **Speed.** Positions come in one pass a row: `MeasuredLine::xs_for_indexes` (a default method in fe-display; the editor's GPUI line walks its glyphs once, where `LineLayout::x_for_index` scans from the start each time), `ExpandedRow::to_display_ascending` (one pass over the tabs and spans), `RowIndex::xs_for_offsets` (one walk of a long row's index, not a walk from a 4 KiB checkpoint per char) and `RowLayout::xs_for_offsets`, which joins them as `x_for_offset` does, hints outside a long row's slice included (`crates/fe-display/tests/batched_positions.rs` checks it against `x_for_offset`). The window's text is read in one slice when it's at most four times the byte cap, rather than a rope seek a row.
- **Bounded reads (review F1).** A margin row that doesn't fit the bytes left is rejected from its range before anything of it is read or measured; a visible row past the cap is read only to the cut and a few bytes after it (the grapheme cursor's context). Tests count the bytes a source gives and the chars measured for a 4 MiB row beside the view and in it.
- **Vim and read-only (review F2, F3).** A screen reader's selection first drops a pending vim command and leaves visual mode (`cancel_vim`, as a click or a jump does), so a visual sync doesn't restore the old selection and an operator doesn't act at the new caret. The node is read-only when the editor is (`is_read_only`, a diff's read-only side) or outside vim's typing modes.
- **Tests.** `element/a11y_text_tests.rs` (33, with the recorder test, the adapter's read-only and vim tests, plus `element::line_position_tests`): the pure builder over real `DisplayMap`s and `layout_frame` frames, loaded in `accesskit_consumer =0.38.0` and asked `document_range`, `line_range_from_index`, `text_selection`/`_focus`/`_anchor`, `to_line_index`, `to_global_utf16_index`, `text_position_from_global_utf16_index`, `bounding_boxes` and `text_position_at_point`. GPUI's test platform can't turn a11y on (section 9), so the adapter is checked by the recorder test (the group and the text area, named), by calling its builder on a drawn editor's frame (read-only, vim), and on the Mac by A11Y-11.
- **Not in this card:** `value` stays unset (section 4; H4 decides); a screen reader's selection after a fold header's break lands on the first hidden row, as a click there would; the viewer's use of the builder is A11Y-8's.

### Bench (`cargo bench -p fe-gpui --bench a11y_text`)

12,739 lines (478 KiB) of fe-gpui's Rust source; per keystroke the builder, then an `accesskit_consumer::Tree` update with what it writes. Linux x86-64 lane machine, release, other lanes building (load 15–28 for this run), so upper bounds. Budget (section 4): 0.5 ms p99 for both, on the Mac.

| Case | Runs | Builder p50 / p99 | Consumer update p50 / p99 | Both p50 / p99 |
|---|---:|---:|---:|---:|
| Keystroke, 100-row viewport (at the 300-row cap) | 300 | 0.178 / 0.258 ms | 0.105 / 0.157 ms | **0.283 / 0.421 ms** |
| Scroll a row, 100-row viewport | 300 | | | 0.346 / 0.457 ms |
| Keystroke, 60-row viewport | 180 | 0.098 / 0.116 ms | 0.061 / 0.069 ms | 0.160 / 0.183 ms |
| Scroll a row, 60-row viewport | 180 | | | 0.200 / 0.217 ms |
| Stress: a 4 MiB line of `é`, wrap off (one run cut at 64 KiB) | 60 | | | 1.169 / 1.322 ms |
| Stress: rows of 1,000 tabs and letters (64 KiB) | 65 | | | 0.559 / 0.677 ms |

The budget case (the Rust file) is within 0.5 ms p99. The stress rows hold 5× its text (the full 64 KiB cap, against 12 KB); their cost is linear in the text emitted and bounded by the cap, not by the row's length.

### Human checks for A11Y-11 (section 12)

- **H4. Editor: Inspector on the text area.** Role `AXTextArea`. `AXNumberOfCharacters` is about three screens of text. `AXInsertionPointLineNumber` changes with the caret. `AXValue` is empty. Note whether VoiceOver still reads the text with VO-A and VO-L; if not, A11Y-7 must set `value`.
- **H5. Editor speech.** Type a word: the characters and the word are echoed. Arrow down: the new line is read. Arrow right: the character is read. Down onto the wrapped line, then down again: each visual row is read. Repeat in vim normal mode with `j`/`l`: lines and characters are read, and command keys aren't echoed as typing. If arrows are silent, that is section 3 trigger 1.
- **H6. Window edges.** Page Down to the end of a long file and back. Reading follows the viewport. VO-A from the top of the screen stops about one screen below the last visible line. Note whether that's acceptable; this is Q1 and trigger 2.
