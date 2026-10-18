---
task_uid: "01m57ya9vehwyw2z9kjd028m3s"
task: "T203"
rev: 2
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "An editor tab shows its file-type icon"
    required: true
    manual: false
  - id: "C2"
    text: "An inactive, unhovered tab has no close button; a dirty dot stays"
    required: true
    manual: false
  - id: "C3"
    text: "tab_icon_close.rs proves both"
    required: true
    manual: false
---

# Show a file-type icon on each tab and the close button only on the active or hovered tab (Mac T986)

## Problem
VS Code Parity Notes (Tabs): fastedit tabs are text only. PaneItem::tab_icon (kiln/crates/kiln-shell/src/item.rs:70) defaults to None and no editor item supplies one; the close button is drawn on every tab.
## Change
Editors return a file-type icon from tab_icon (the project tree's file icons); the pane draws the close button only on the active or hovered tab (dirty dot stays visible).
