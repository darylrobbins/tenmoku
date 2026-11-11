---
task_uid: "01m743s80t7b1nhfc4z7z09nmp"
task: "T575"
rev: 1
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "The file viewer's text area exposes a read-only text node built with A11Y-7's builder, as section 4 specifies for the viewer"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in crates/fe-gpui/src/file_viewer/tests.rs cover the viewer's run order, a soft-wrapped row and a selection through accesskit_consumer, and pass"
    required: true
    manual: false
  - id: "C3"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-8 File viewer text area: a read-only text node from the editor's a11y builder (T294 design) (Mac T1674)

# A11Y-8 File viewer text area

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-8 File viewer text area** row of section 11 "Proposed card breakdown", and section 4 "Editor text area" as it applies to the file viewer.
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Implementation notes

- The viewer's `file-viewer-text` div (around `FileViewerElement` only) is a `MultilineTextInput` named for the file, active descendant while the viewer is focused. With `window.is_a11y_active()` it pushes synthetic run children and handles `SetTextSelection` (`FileViewer::select_from_a11y`). `FileViewerElement::prepaint` builds `a11y_text::build` (editor cap, `read_only: true`) into the viewer's `A11yTextCell`. The cell/attach helpers are viewer-local copies, since the editor's `A11yTextState.ids` is private and `element.rs` is outside this card's scope.
- The viewer has no soft wrap (design section 4), so the "soft-wrapped row" test checks that a row far wider than the view is one whole run, segment 0.
- Tests: `file_viewer/tests.rs` module `a11y`, in `accesskit_consumer`. GPUI's test platform can't turn a11y on, so the builder is called on the drawn frame.
- Not done: the carried T549 F2 (stored translated strings) spans image_viewer.rs, diff_view.rs and substitute_preview.rs, outside scope.

### Human checks for A11Y-11 (section 12)

- **H12. File viewer.** Open a large log file in the read-only viewer and repeat H4–H5. The text node is read-only, and typing isn't echoed as text.
- (H4. Editor: Inspector on the text area. Role `AXTextArea`; `AXNumberOfCharacters` about three screens of text; `AXInsertionPointLineNumber` changes with the caret; `AXValue` empty.)
- (H5. Editor speech. Arrow down reads the new line; arrow right reads the character; no typing echo in the viewer.)
