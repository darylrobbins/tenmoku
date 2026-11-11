---
task_uid: "01m743s68kzk9sjpx4kfrnxz6n"
task: "T573"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "TextInput and TextField/TextLine each expose a node with an id, a role (TextInput, SearchInput or PasswordInput), their value as one text run, the selection, and a SetTextSelection action handler, as section 6's template specifies"
    required: true
    manual: false
  - id: "C2"
    text: "Tests in kiln/crates/kiln-ui/src/tests/text_input_a11y.rs and kiln/crates/kiln-ui/src/tests/text_field.rs load the node in accesskit_consumer (dev-dependency =0.38.0) and assert the value, the selection after typing and after SetTextSelection, non-BMP UTF-16 lengths and that a password input exposes no value; all pass"
    required: true
    manual: false
  - id: "C3"
    text: "The finish notes list this card's section 12 human-check lines (H-numbers and text), for the A11Y-11 VoiceOver pass"
    required: true
    manual: false
---

# A11Y-5 Text-input template: TextInput and TextField expose role, value, selection and SetTextSelection (T294 design) (Mac T1671)

# A11Y-5 Text-input template

Spec: `docs/decisions/qenw9xxdzk-accessibility-design.md`, the **A11Y-5 Text-input template** row of section 11 "Proposed card breakdown", and section 6 "Chrome semantics" (the text-input template) and section 9 shape 1.
The user approved the design's platform path (section 3, option (a): gpui-pre 0.3.9's
AccessKit, no vendoring) and its first slice (section 10) as written on 2026-11-09.

Build what that row and those sections specify, with the tests section 9 "Testing strategy"
assigns (shape 1 for text elements, shape 2 for chrome). If the design's text differs from
this card's criteria, the design wins.

Platform: **Linux-OK**.

## Implementation notes

- `A11yLine` (`kiln-ui/src/text_input.rs`, crate-private, shared with `text_field.rs`) is the pure builder: role, the text the run reads, the selection's byte ends and `read_only`. `write` makes the one `TextRun` (value, `character_lengths`, left-to-right) and sets the `TextSelection` on the input's node; `attach` adds it through `a11y_synthetic_children` and registers the `SetTextSelection` handler with `on_a11y_action`. Built only while `window.is_a11y_active()`.
- A character, to AccessKit, is a grapheme, so a screen reader's caret lands where the arrow keys would; a grapheme over 255 bytes is split at a char boundary, and an offset inside one snaps back to its start.
- Both widgets now set their role and name through A11Y-2's `node()`, so the shape-2 recorder sees them (A11Y-6's picker tests can assert `SearchInput`).
- Roles: `TextInput::search()` / `TextField::search()` give `SearchInput` (callers such as the find bar and pickers opt in under A11Y-6); a masked `TextField` is `PasswordInput` whatever `search` says. `TextInput` has no masking, so it is never `PasswordInput`.
- **Password value.** Following section 4's template ("built from `shown_text` … so password fields never expose their text"), a masked field's run reads one bullet a grapheme: the consumer's value is `•••••`, never the secret, and the test checks no node's value or label holds any of it. Length and selection stay usable, as NSSecureTextField reports bullets.
- `TextInput`'s selection was all-or-nothing; review F1 (round 1) asked for real ranges, so it now holds an anchor and the caret: `SetTextSelection` selects any range, forwards or backwards, typing and deletes replace it, Left/Right collapse it to its start/end, the platform's `selected_text_range` reports it (with `reversed`), and it is highlighted. `select_all` is that range over all the text, and `selected()` still means all of it; `selection()` gives the bytes. An open composition is in the run (as the platform sees it) and is dropped by a screen reader's selection, as a move drops it. With typing off (a picker's vim normal mode) the node is `read_only`.
- Not in this card: run geometry (`bounds`, character positions and widths), so `accessibilityFrameForRange:` answers zero for these fields; the placeholder as `placeholder` (the name already falls back to it).

### Human checks for A11Y-11 (section 12)

- **H8. Pickers.** Open the command palette and type: the query is echoed. Down arrow reads the selected command. Escape returns to the editor, and VoiceOver says so. The same for quick open. (This card's part: the query is a `SearchInput` whose typing is echoed; the role itself arrives once A11Y-6 calls `search()`.)
- **H11a′. Focus and focus ring, keyboard only.** Of the surfaces that take focus, the text fields show the focus ring. With Full Keyboard Access on, Space still types in text fields.
- **Proposed addition to H1′, not yet in section 12's text.** Accessibility Inspector on a text field (find bar, a picker query, the project panel filter, a settings field): `AXTextField` (or `AXSearchField`; `AXSecureTextField` for a masked field) with its name as title, `AXValue` the text (bullets when masked), `AXSelectedTextRange` following the caret, and `setAccessibilitySelectedTextRange:` moving it.
