---
task_uid: "01m57wa3a01eabkpqn6f6e6nt2"
task: "T984"
rev: 7
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The rail has one Explorer button holding Folders and Outline views"
    required: true
    manual: false
  - id: "C2"
    text: "dock.show_project and the outline show command both show the Explorer, the latter with Outline expanded"
    required: true
    manual: false
  - id: "C3"
    text: "Layouts saved before the change load, with Outline in its default state"
    required: true
    manual: false
  - id: "C4"
    text: "explorer_views_tests.rs proves the above"
    required: true
    manual: false
---

# Show fastedit's Folders tree and Outline as two views of one Explorer view container on the Left rail

## Problem
fastedit's sidebar (crates/fe-workbench/src/dock.rs, `SidebarPanel` entries from project_panel.rs, outline_panel.rs, search_panel.rs, git_panel.rs, git_log.rs) gives the file tree and Outline separate rail buttons; VS Code shows them as views of one Explorer container.
## Change
Build the project panel's file tree and the Outline panel as two views (Folders, Outline) of one Explorer `ViewContainer` on the Left rail; the rail has one Explorer button, `dock.show_project` shows it and outline's show command shows it with the Outline view expanded. Layout files saved before this load with Outline collapsed state default. Update tests in a new crates/fe-workbench/src/explorer_views_tests.rs.

## Implementation notes
- The Explorer is kiln-shell's `ViewContainer` (T983), built in `dock.rs`, with
  Folders (the `ProjectPanel`, order 10) over Outline (the `OutlinePanel`,
  order 20, collapsed by default). It is registered under the old project
  panel's id, `project`, so layouts saved before it (`"active": "project"`)
  restore it; `DockPanel::Project` is now `DockPanel::Explorer`, and
  `DockPanel::Outline` is gone, as is the Outline rail button. A saved
  `"active": "outline"` is read as the Explorer.
- The sidebar is the dock on the window's right by default (`dock.move_left`
  moves it), so "the Left rail" is read as the sidebar's rail wherever it is.
- `dock.show_project` shows the Explorer and expands Folders, since it focuses
  the tree; `outline.show` shows it and expands Outline, focus staying where it
  is unless on a panel it hid. A new root's tree replaces the Folders view,
  keeping the views' collapse.
- The views' collapse and weights are saved under the dock's
  `panels.project.views`; the dock notifies as they change so the layout saves.
