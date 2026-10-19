---
task_uid: "01m57w9n5eacx78pjvqjzhhqs4"
task: "T983"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "add_view orders by order and replaces the same id"
    required: true
    manual: false
  - id: "C2"
    text: "DockItem::save_state/restore_state default to no-ops so existing panels and old layouts load unchanged"
    required: true
    manual: false
  - id: "C3"
    text: "A view's collapsed state persists through a Workspace layout save and restore"
    required: true
    manual: false
  - id: "C4"
    text: "framework.md documents ViewContainer and ViewSpec"
    required: true
    manual: false
---

# Add kiln-shell ViewContainer: a DockItem panel that holds views registered by id, with their state in saved layouts

## Problem
A `Dock` panel cannot yet be a view container: nothing lets an app register views into a panel by id, and a saved layout does not hold the views' collapsed state or sizes.
## Change
Add `ViewContainer` in a new kiln/crates/kiln-shell/src/view_container.rs: a `DockItem` (id, title, icon, show command) that owns a `ViewStack` and has `add_view(ViewSpec { id, title, order, default_collapsed, view }, cx)`, ordered by `order`, replacing the same id. Its stack state saves under a "views" key that `DockState::save`/`restore` carry for any panel that implements an optional `DockItem::save_state/restore_state` pair (default no-op, so existing panels and old layouts are unchanged). Document in kiln/docs/framework.md. Tests in kiln/crates/kiln-shell/tests/view_container.rs: views order, collapse persists across save/restore through Workspace layout, unknown view ids ignored.
