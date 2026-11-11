---
task_uid: "01m6vvv8m4tpnjxz4fdpg60nxr"
task: "T1548"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Tests in tests::bottom_panel_b3 (cargo nextest run -p fe-workbench) pass: the rail's Problems, Git log and Terminal open in the bottom panel; its header is a 34 px tab strip with one tab per open tool window and the pane controls at its end; default height 250 px; the Git log is no longer a view under Changes"
    required: true
    manual: false
  - id: "C2"
    text: "A layout saved before this change (Problems in the side dock, terminal bottom) still loads (tests::bottom_panel_b3::old_layouts_load)"
    required: true
    manual: false
  - id: "C3"
    text: "cargo nextest run -p fe-workbench --test l10n passes (ui_text_is_in_the_catalogue, keys_used_here_exist) with this card's files in l10n-allow.toml's migrated list"
    required: true
    manual: false
  - id: "C4"
    text: "On the Mac: cargo run -p fe-workbench --example screenshots matches every golden; the ones this card claims were regenerated with UPDATE_SCREENSHOTS=1 in this change and looked at (workbench.png if it changes)"
    required: true
    manual: false
---

# Bottom panel: Problems, Terminal and Git log as bottom tool windows with the panel's own 34 px tab strip

# Bottom panel: Problems, Terminal and Git log as bottom tool windows

Spec: `docs/design/design-system.md#workbench-anatomy` (Bottom panel:
Problems, Runs, Terminal, Git log, own header, own tabs; Measure: 250 px,
resizable). Audit finding GL2 (the held card P20).

Today the bottom dock holds only the terminal (`DockState::new(Bottom).without_rail()`,
`crates/fe-workbench/src/workbench.rs`), and Problems and the Git log are side
views.

## Change

- The bottom dock holds Problems, Terminal and Git log (Runs when it exists),
  opened from the rail's bottom group; its header is a 34 px panel tab strip
  (matching the editor's) with the pane controls at its right. Default height
  250 px, resizable as now.
- The Git log leaves the Source Control container and becomes its own tool
  window (`crates/fe-workbench/src/dock.rs`, `git_log.rs` placement only).
- Saved layouts that name the old places still load (`crates/fe-workbench/src/dock_layout.rs`,
  `workbench_chrome.rs`); `kiln/crates/kiln-shell/src/dock.rs` bottom variant
  gets tabs.
- Tests that pin the bottom dock follow (`crates/fe-workbench/src/layout_tests.rs`,
  `dock_position_tests.rs`, `kiln/crates/kiln-shell/tests/dock_position.rs`).
- If `workbench.png` changes, regenerate it on the Mac.

## i18n

Text in `workbench.rs` and `dock.rs` this card adds goes in as keys (`dock.ftl`, which the rail card created; `workbench.rs` itself is migrated by the last fe-workbench card).

Platform: **Mac-only** (workbench chrome).
