---
task_uid: "01m62t2f3pd8wge7j7jmz7ntyw"
task: "T1232"
rev: 3
type: "bugfix"
complexity: "xs"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::pane_toolbar_no_path_item::settings_tab_has_no_untitled_crumb: with the Settings tab active, pane.drawn_crumbs() contains no 'untitled' crumb (at the base it has one, from kiln_shell::crumbs::path_crumbs(None, ..))"
    required: true
    manual: false
  - id: "C2"
    text: "tests::pane_toolbar_no_path_item::keyboard_shortcuts_tab_has_none_either: same for Open Keyboard Shortcuts"
    required: true
    manual: false
  - id: "C3"
    text: "tests::pane_toolbar_no_path_item::untitled_buffer_still_says_untitled: a new empty editor buffer still shows 'untitled'; existing tests::pane_toolbar tests pass unchanged"
    required: true
    manual: false
  - id: "C4"
    text: "no file outside the scope changes"
    required: true
    manual: false
---

# Stop the pane toolbar showing the crumb 'untitled' for a Settings (or Keyboard Shortcuts) tab, which has no file

## Problem
The toolbar's crumbs come from pane.rs (~line 2478-2497): `pane_toolbar::path_crumbs(path, root)`. For an item with no path (Settings editor, Shortcuts editor) `kiln_shell::crumbs::path_crumbs(None, ..)` returns the single crumb 'untitled', which is right for an untitled editor buffer and wrong for every other pathless item. The one existing special case is the DiffView (`diff_title`, named by its tab title).
## Change
Generalise `diff_title`: only an item that is an EditorView with no path says 'untitled'; any other pathless item (Settings, Shortcuts, ...) shows its `tab_title(cx)` as a non-file crumb (or no path crumb at all, then only the item's own `breadcrumbs`). Prefer tab title: 'Settings'. Do not change kiln-shell crumbs.rs. Depends on T1217, which also edits pane.rs.
