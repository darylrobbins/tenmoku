---
task_uid: "01m6z298jj61a7zn6zk225w8xc"
task: "T537"
rev: 2
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "the string fixture with format specifiers renders under Fastedit dark, light and ember: the error draws in the editor foreground, braces and escapes draw in their resolved colours, and the surrounding text keeps the string colour, asserted on drawn colours rather than model decorations (review F2)"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench -p fe-gpui and cargo clippy -p fe-workbench -p fe-gpui --all-targets -- -D warnings pass"
    required: true
    manual: false
---

# Follow-ups from review of T1123 (Mac T1607)

Findings of the review of T1123 that don't block the merge (below merge.block_severity):

- F2 medium crates/fe-workbench/src/tests/lsp_semantic_format_specifier.rs:72: The fixture and token ranges are corrected, but the requested captured-name rendering verification remains missing. This test still reads model decorations. The new foreground_spans test selects an arbitrary uncoloured highlight (currently embedded), uses only default_dark, and never overlays a string or checks drawn colours. Consequently, neither test verifies that error actually overrides the surrounding string under all three Fastedit themes.
  fix: Add rendering assertions for the string fixture under Fastedit dark, light, and ember: verify error draws in the editor foreground, braces and escapes draw in their resolved colours, and surrounding text retains the string colour.
  instances: crates/fe-workbench/src/tests/lsp_semantic_format_specifier.rs:72, crates/fe-gpui/src/element/foreground_spans_tests.rs:12
