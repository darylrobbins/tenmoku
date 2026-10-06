---
task_uid: "01m48y3pah09mn9p5f83rx5at0"
task: "T111"
rev: 3
type: "chore"
complexity: "l"
criteria:
  - id: "C1"
    text: "F1: clicking the Project header tab while the search input is focused moves focus to the active pane, and cmd-b and cmd-shift-f then work without another click"
    required: true
    manual: false
  - id: "C2"
    text: "F2: a mouse-down, rerun, mouse-up sequence where a different match replaces the pressed row opens nothing or the pressed match, never the replacement"
    required: true
    manual: false
  - id: "C3"
    text: "F3: with invalid UTF-8 before a match (bytes ab\\xffneedle), the preview highlight and the editor selection both cover exactly needle"
    required: true
    manual: false
  - id: "C4"
    text: "F4: overlapping or duplicate highlight ranges are merged after widening; a byte regex over a multibyte character followed by unmatched text highlights only the matched character"
    required: true
    manual: false
  - id: "C5"
    text: "New tests live in crates/fe-workbench/src/tests/search_panel_followups.rs and fail at base"
    required: true
    manual: false
---

# Follow-ups from review of T77

Findings of the review of T77 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-workbench/src/dock.rs:140: Clicking the Project header tab while the search input is focused hides that input without transferring focus. Unlike the ShowProject command, this path never calls leave_search. The focused handle then has no rendered dispatch path, so workbench shortcuts such as cmd-b and cmd-shift-f stop working until another focusable element is clicked.
  fix: Route header switches through the same focus-aware transition as keyboard commands, using the active pane as the fallback. Add a test that clicks Project from a focused search input and then uses the dock shortcuts.
  instances: crates/fe-workbench/src/dock.rs:140

- F2 medium crates/fe-workbench/src/search_panel.rs:318: Result element IDs contain only the row index and are reused across generations. If a new search replaces results between mouse-down and mouse-up, GPUI can retain the pressed state for that index and invoke the replacement row's handler. Its captured generation is already the new generation, so the guard in open does not prevent opening a different match from the one pressed.
  fix: Include the search generation in each result element's identity. Add a mouse-down/rerun/mouse-up regression test where a different match replaces the pressed row before release.
  instances: crates/fe-workbench/src/search_panel.rs:318

- F3 medium crates/fe-workbench/src/search_panel.rs:279: Source-byte ranges are treated as offsets in decoded text. T65 supports invalid UTF-8 and documents that lossy decoding shifts later text; the editor also replaces invalid sequences. For bytes ab\xffneedle, the search range is 3..9 but needle occupies 5..11 in the preview and buffer. The current code highlights and selects the replacement character plus only part of needle; character-boundary clamping cannot repair this coordinate mismatch.
  fix: Carry or compute a source-to-decoded offset mapping for preview highlights and editor selection, retaining row-based CRLF handling. Add a raw-byte fixture with invalid UTF-8 before the match and assert both highlight and selection coordinates.
  instances: crates/fe-workbench/src/search_panel.rs:279, crates/fe-workbench/src/search_panel.rs:378, crates/fe-workbench/src/pane.rs:340

- F4 medium crates/fe-workbench/src/search_panel.rs:383: Widening each match independently to character boundaries can produce overlapping highlights, which StyledText converts into consecutive text runs. For regex (?-u:\xC3|\xA9) against éX, the byte matches 0..1 and 1..2 both become 0..2. The resulting duplicate runs extend the accent onto X even though it did not match.
  fix: Merge overlapping or duplicate highlight ranges after clipping and widening. Add a byte-regex fixture over a multibyte character followed by unmatched text.
  instances: crates/fe-workbench/src/search_panel.rs:383
