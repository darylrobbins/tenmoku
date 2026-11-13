---
task_uid: "01m7azq5qtykjxdb5kx9v8vsxh"
task: "T372"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln_ui::Button exposes test-visible debug_selector ids on its label and its chevron, and tests::git_sync_ui passes (nextest -p fe-workbench) with a long-branch-name test that asserts, at a wide and a narrow window, the observed bounds: the name label is truncated (narrower than its full shaped width) while the complete count text is present, and name, counts and chevron are contained in the widget and ordered by their real bounds"
    required: true
    manual: false
  - id: "C2"
    text: "cargo nextest run -p fe-workbench --test l10n and cargo nextest run -p kiln-ui pass, python3 scripts/check_kiln_boundary.py passes, and the Button renders identically (no visible change)"
    required: true
    manual: false
---

# Title bar branch widget: observe the label's ellipsis and the chevron's real bounds (kiln_ui::Button test hook; follow-up from review of T370 and T367, Mac T1766)

# Title bar branch widget: observe the label's ellipsis and the chevron's real bounds (kiln_ui::Button test hook; follow-up from review of T370 and T367, Mac T1766)

Codex's review of T367 (F2) and again of T370 asks that the long-branch-name test in crates/fe-workbench/src/tests/git_sync_ui.rs inspect the RENDERED name truncation (an ellipsis) and the real bounds of the name, the counts and the chevron, not bounds computed from the button edge minus padding. T370 did everything its scope allows: at a wide and a narrow window it compares the counts bounds with the shaped count-text width and the name's room with its shaped full width (so truncation is established), and checks containment and order from computed bounds.

What is still not observable: the Button draws its label in a private truncating div and its chevron as its own svg (kiln/crates/kiln-ui/src/controls.rs, around lines 576-647); neither carries a debug_selector and the Button API has no hook, so crates/fe-workbench cannot reach them.

## Change
In kiln_ui::Button add test-visible debug_selector ids on the label element and on the chevron icon (derived from the button's id, e.g. "<id>-label" and "<id>-chevron"; follow the lint rules in memory: kiln-ui lints every literal, so keep the suffixes constructor-side constants or whatever the lint allows, and do not add user-facing strings). Then in git_sync_ui.rs assert the REAL bounds: at both window sizes the truncated name's label is narrower than the full shaped name while the count text is complete, and name label, counts and chevron are contained in the widget and ordered by their observed bounds. Replace T370's computed-bounds assertions with these where they overlap.

Scope notes: kiln must not depend on fastedit (scripts/check_kiln_boundary.py; the SPDX header rule applies to any new file); the change in kiln-ui must not alter how the Button looks or lays out.
