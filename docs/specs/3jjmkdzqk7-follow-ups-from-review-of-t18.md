---
task_uid: "01m4pfxeyad243963jjmkdzqk7"
task: "T23"
rev: 2
type: "chore"
complexity: "m"
criteria: []
---

# Follow-ups from review of T18

Findings of the review of T18 that don't block the merge (below merge.block_severity):

- F1 medium crates/fe-terminal-view/tests/view.rs:187: The shaping regression uses GPUI's default NoopTextSystem, which gives every BMP character the same advance, never forms ligatures, and advances combining marks as ordinary characters. PaintedText also records only the shaped line's input text and caller-provided origin. Consequently, the purported narrow glyphs are not narrow, and changing ordinary-text decoration widths back to the natural shaped width of `ab` would still pass. This leaves part of the requested rendering regression untested.
  fix: Use a real font-backed text system or a controlled shaping backend with distinct narrow advances, ligatures and combining-mark behavior. Verify the shaping assumptions and inspect shaped glyph positions or painted output, including ordinary decoration widths that differ from natural text widths.
  instances: crates/fe-terminal-view/tests/view.rs:187

- F2 medium crates/fe-terminal-view/src/element.rs:377: Test reporting runs unconditionally in production on every terminal prepaint. It allocates a new String for every nonblank cell, copies all decoration/background bounds, and retains the duplicate report in TerminalView. A populated terminal therefore adds thousands of allocations and corresponding frees per redraw solely for test inspection, introducing avoidable work on the rendering hot path.
  fix: Make report collection opt-in for tests, disabled by default, or move these regressions into unit tests with test-only instrumentation. Ensure normal terminal rendering does not construct or retain PaintReport.
  instances: crates/fe-terminal-view/src/element.rs:377, crates/fe-terminal-view/src/element.rs:146, crates/fe-terminal-view/src/element.rs:156, crates/fe-terminal-view/src/element.rs:169
