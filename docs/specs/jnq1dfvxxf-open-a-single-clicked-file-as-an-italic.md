---
task_uid: "01m57yaa59k68t8yjnq1dfvxxf"
task: "T204"
rev: 4
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "Single click opens an italic preview tab; the next single click replaces it"
    required: true
    manual: false
  - id: "C2"
    text: "Editing, double-click or pinning makes it permanent"
    required: true
    manual: false
  - id: "C3"
    text: "preview_tab.rs proves these"
    required: true
    manual: false
---

# Open a single-clicked file as an italic preview tab that the next single click replaces (Mac T987)

## Problem
VS Code Parity Notes (Tabs): no preview tabs. kiln-shell/src/pane.rs has no preview state.
## Change
A single click on a project tree file opens an italic preview tab that the next single click replaces; editing, double-click or pinning makes it permanent. Setting workbench.preview_tabs, on by default.

## Implementation notes

- kiln-shell's `PaneState` holds at most one preview item. `PaneView::add_preview`
  puts a new one in the old one's place, closing it without asking; a dirty
  preview is never replaced. Each item's observer clears the preview once it
  is dirty, so an edit keeps it for good, even if it is saved or undone later.
  `PaneView::pin_item` keeps it, and a double-click on its tab does that.
  kiln-ui's `Tab::preview` draws the label and description in italics.
- In the project panel, the first click on a file emits `PanelEvent::PreviewFile`
  and the second click of a double-click emits `OpenFile`. Enter and the other
  keys still open a file to stay. An `open` that isn't a preview (a double-click,
  go to definition, quick open) keeps a preview tab of that file. Each pending
  read in the fe-workbench pane records why it was opened (`Intent`). A "keep"
  intent is permanent, so a later single click can't turn it back into a
  preview. That holds even when another pane's buffer is shared. A read the host
  starts with `Pane::opening` is a "keep". The intent also carries through a wait
  behind a Save As.
- A preview read lands only if it is still the file opened last, or the pane
  shows nothing else. When a later open or a shown tab has superseded it, the
  read is dropped, so an older preview never replaces a newer one or shows
  without focus.
- "Keep Editor" (`workbench.keep_editor`, palette only, with no key) pins the
  active tab.
- `workbench.preview_tabs` (default on) is read from the global settings when a
  workbench is made and from each settings reload. The shared test window in
  `fe-workbench/src/tests` turns it off, because the older tests click rows
  expecting tabs that stay open.
