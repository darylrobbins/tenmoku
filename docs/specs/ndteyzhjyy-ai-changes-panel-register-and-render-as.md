---
task_uid: "01m5q7z4h809b81cndteyzhjyy"
task: "T243"
rev: 2
type: "feature"
complexity: ""
criteria:
  - id: "C1"
    text: "ai_changes_shows_in_the_right_sidebar_on_changeset: setting a changeset opens AI Changes in the right sidebar beside Problems and focuses it. Headless on Linux"
    required: true
    manual: false
  - id: "C2"
    text: "ai_changes_has_no_rail_entry: there is no activity-rail entry for AI Changes before, or after it has been shown and hidden. Headless on Linux"
    required: true
    manual: false
  - id: "C3"
    text: "The AI Changes commands still open and close the panel"
    required: true
    manual: false
  - id: "C4"
    text: "The existing ai_changes_panel_tests pass; cargo test -p fe-workbench and -p kiln-shell pass and clippy -D warnings is clean; no file outside the scope changes"
    required: true
    manual: false
---

# AI Changes panel: a right-sidebar panel beside Problems that opens itself when a changeset arrives, with no activity-rail entry (follow-up of T235 F10)

## Problem
T235 shipped the AI Changes panel as a right-sidebar panel that joins the activity rail once it has shown (kiln-shell's `DockItem::always_on_rail() == false` means "joins once first shown", `Dock::on_rail`). Review finding F10 (medium, T235) wants it beside Problems with no rail entry. The user decided: it stays in the right sidebar, no bottom-dock or terminal-dock work.

## Change
Keep AI Changes a right-sidebar panel registered in crates/fe-workbench/src/dock.rs beside ProblemsPanel. Setting a changeset shows and focuses it (focus_on_show, as now). It never gets an activity-rail entry: not before it is shown and not after it has been shown and hidden. The rail adds a shown panel in kiln-shell's Dock (`on_rail` / `joined`), so add a small generic opt-out to `DockItem` in kiln/crates/kiln-shell/src/dock.rs (a defaulted method, default behaviour unchanged for every other panel), carry it through `SidebarPanel` in fe-workbench dock.rs, and set it for AI Changes only. Keep the kiln change generic (no fastedit names in kiln). Keep set_changeset, the commands and the row behaviour as T235 left them. No bottom-dock or terminal-dock work.

## Tests
Headless on Linux, in ai_changes_panel_tests.rs: ai_changes_shows_in_the_right_sidebar_on_changeset and ai_changes_has_no_rail_entry; the commands still open and close it; the existing ai_changes_panel_tests pass.
