---
task_uid: "01m7c2j161tx4rnww587qhebwk"
task: "T1829"
rev: 1
type: "bugfix"
complexity: "s"
criteria:
  - id: "C1"
    text: "tests::rail_badge_placement: with 3 changed files the Changes badge fill lies wholly inside the Changes button and does not overlap the Chat button or its icon box"
    required: true
    manual: false
  - id: "C2"
    text: "source_control::SIDEBAR.icon is Icon::FileDiff; rail_b3 and kiln-shell dock_rail_actions badge tests updated to the corner placement; the gap is struck in docs/design-language.md"
    required: true
    manual: false
  - id: "C3"
    text: "Screenshot goldens that move are regenerated on the Mac and pass"
    required: true
    manual: false
---

# Rail: Changes uses the file-diff glyph, and its count badge sits at its own icon's bottom-right corner, clear of the Chat button below

User report: the Changes count badge on the activity rail looks like it belongs to the robot (Chat) button below it, and the design system's Changes icon was never switched in.

Cause (verified in code): the badge is keyed correctly to `DockPanel::SourceControl` (crates/fe-workbench/src/dock.rs ~728-735). Its placement in kiln/crates/kiln-ui/src/activity_rail.rs (~295-304) puts the fill's top at the icon's bottom edge (top = (32 + 16)/2 - 2 = 22), so the 14 px pill and its 2 px ring run to y ~40, into the next button. That button is Chat, whose Bot glyph starts at y=40. T1540 chose below-the-icon on purpose (docs/decisions/5gxt28kdhz-rail-b3.md ~23-36) and the tests pin it (crates/fe-workbench/src/tests/rail_b3.rs ~243-254, kiln/crates/kiln-shell/tests/dock_rail_actions.rs ~359).

Design: docs/design/design-system.md ~156 says rail badges sit at the bottom-right corner with a 2 px cut-out ring and never on top of the glyph; ~172 says Changes is "a page with +/-". The asset exists as `Icon::FileDiff` (kiln/crates/kiln-ui/src/file_icons.rs ~104); the rail still uses `Icon::GitBranch` (crates/fe-workbench/src/source_control.rs ~22). docs/design-language.md ~686-688 lists this as an open gap.

Fix:
1. source_control::SIDEBAR uses Icon::FileDiff.
2. Place the badge at the icon's bottom-right corner, inside its own 32 px button: it may overlap the glyph's corner with the cut-out ring (that is what "cut-out ring" is for; "never on top" means never over the glyph's middle), and it must not cross into the next button. Record the reading in the decision file.
3. The same placement serves T1804's agent count on the robot button, so two stacked badges never touch.
4. Update the rail_b3 and dock_rail_actions badge-clearance tests to the new rule, strike the gap in docs/design-language.md, regenerate the screenshot goldens that move.

Regression test in a new crates/fe-workbench/src/tests/rail_badge_placement.rs declared from src/tests/mod.rs, written against base APIs (the drawn badge and icon bounds rail_b3.rs already reads), assert_eq!/assert! without a message.
