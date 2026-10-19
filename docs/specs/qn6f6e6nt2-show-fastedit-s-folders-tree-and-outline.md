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
