---
task_uid: "01m50ewgrfsexvs6qab7jkh9ar"
task: "T87"
rev: 1
type: "chore"
complexity: "s"
criteria:
  - id: "C1"
    text: "kiln-shell tests/tab_title_color asserts the colour the tab title is drawn in (through the render path, not PaneItem::tab_title_color or Tab fields): Some(c) draws in c and the default None draws in the theme's tab text colour, with the tab active and inactive; the dirty-dot assertions remain"
    required: true
    manual: false
---

# Assert the drawn tab title colour in kiln-shell's tab_title_color test, active and inactive, including the default (T714 H1/F2 follow-up) (Mac T715)

Filed on argon from Mac T715 (card ~/coordination/argon-cards/T715.json rev 1; note zh). Mac task uid 01m4xe5av53s9sc85vcchhgb89. Base: argon trunk bb63a81 = mac/main 6ac346f synced. Design record: docs/decisions/kiln-app-layer.md. Mac deps already on the trunk: Mac T714. Mac T675 rewrites kiln/crates/kiln/examples/host and tests/host.rs and re-lands soon; don't touch those, the spec cites them only as background. Title the host commit "... (Mac T715)".

Follow-up to T714 (review findings H1/F2, overridden by the user on 2026-10-14 so T714 could land).

T714 tints an open tab's title by its file's git status. Its kiln-shell test `kiln/crates/kiln-shell/tests/tab_title_color.rs` checks the title colour on the `kiln_ui::Tab`s that `PaneState` builds for drawing (`PaneItem::tab_title_color` → `Tab`), and checks the painted dirty dot, but never asserts the colour the title is actually *drawn* in. Codex asked for the rendered colour, active and inactive, including the trait-default (None) case keeping the theme's tab text colours.

Do: assert the drawn title colour through the real render path (e.g. GPUI's test window render/paint of the tab bar: the text run colour of the title, or a rendered-image sample), for an item returning Some(c) and for the default None, with the tab active and inactive. Keep the existing dirty-dot assertions.

Files: kiln/crates/kiln-shell/tests/tab_title_color.rs (new assertions); test support only if needed.
