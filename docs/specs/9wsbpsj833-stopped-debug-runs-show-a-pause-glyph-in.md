---
task_uid: "01m7fve66yfn8pdm9wsbpsj833"
task: "T663"
rev: 4
type: "feature"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-ui's bundled set has a Pause icon at the set's stroke width"
    required: true
    manual: false
  - id: "C2"
    text: "a stopped debug run's row in the Runs panel draws the Pause icon tinted with the accent colour (test in src/runs_panel_debug_paused_tests.rs, declared from runs_panel.rs; ids runs_panel::debug_paused_tests::\u003ctest\u003e)"
    required: true
    manual: false
---

# Stopped debug runs show a Pause glyph in the accent colour in the Runs panel (kiln-ui Pause icon) (T1848 F3) (Mac T1879)

Review follow-up of Mac T1848 (helium T659) F3, medium, carried as a known limit in its C4.

The debugger design (D0, docs/decisions/*debugger*) draws a stopped debug run with a Pause glyph in the accent colour. kiln-ui's bundled icon set has no Pause icon, so runs_panel.rs draws stopped debug rows with another glyph. Add a Pause icon to kiln/crates/kiln-ui/src/file_icons.rs (with its svg asset, 1.75 stroke like the others, and its NOTICE/licence row if it comes from an icon set), and draw stopped debug rows in crates/fe-workbench/src/runs_panel.rs with it in the accent colour.

Tests: a runs panel test that a stopped debug run's row uses the Pause icon with the accent tint, in a new file crates/fe-workbench/src/runs_panel_debug_paused_tests.rs, declared from runs_panel.rs as `#[cfg(test)] #[path = "runs_panel_debug_paused_tests.rs"] mod debug_paused_tests;` (not from src/tests/mod.rs, which other debugger cards hold). If a screenshot scene shows a stopped debug run, its macOS goldens are regenerated on the Mac when the batch lands.

Platform: Linux-OK.
