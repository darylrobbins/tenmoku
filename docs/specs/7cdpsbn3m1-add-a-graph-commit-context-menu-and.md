---
task_uid: "01m67vc764s1k0ka7cdpsbn3m1"
task: "T263"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test git_log_menu_tests::commit_menu: right click on a commit row offers Copy Commit Hash, Copy Subject, and nothing that rewrites history; each puts the right text on the clipboard"
    required: true
    manual: false
  - id: "C2"
    text: "git_log_menu_tests::file_menu: right click on a file row offers Open Changes, Open File at This Revision (a read-only editor on text_at(id, path), tab '\u003cname\u003e @ \u003cshort\u003e') and Compare with Working Tree (a DiffView of text_at against the file's working side)"
    required: true
    manual: false
  - id: "C3"
    text: "git_log_menu_tests::edges: a deleted file's Compare with Working Tree compares against nothing; a file absent from the working tree shows the right side empty; the menu items are in the command catalog and palette"
    required: true
    manual: false
  - id: "C4"
    text: "cargo test -p fe-workbench passes, incl. command_catalog_tests; clippy clean"
    required: true
    manual: false
---

# Add a Graph commit context menu and palette commands: copy sha, open a file at that revision, and compare a file with the working tree (Mac T1226)

## Problem
My recommendation: VS Code and GitLens offer copy hash, open file at revision and compare with working tree from the graph. git_commands.rs holds the git command registry; the DiffView opener is git_diff.rs. New code goes in git_log_menu.rs; git_log.rs only adds the right-click hook.

## Implementation notes

- The menu and the `git_log.*` typed commands (context `git_log_panel`) live in `git_log_menu.rs`; `git_log.rs` adds the right-click hook on commit and file rows, Escape to close, and attaches the panel's `ViewCommands`. From the palette the commands act on the selected row.
- Compare with Working Tree is `Workbench::compare_revision` in `git_diff.rs` (scope expansion): the commit's text read-only on the left, titled with the short id, against the file's open buffer, else the file from disk, else an empty read-only side. Asking again shows the tab open.
- Open File at This Revision needs a tab title that isn't the file's name, so fe-gpui's `EditorView` gained `set_tab_title(title, icon_by)` (scope expansion). The editor is pathless and clean, so it isn't saved with the layout.
- Carried T262 F1: `git_log_panel` joins the text-input caret/forward-delete predicates in `fastedit.toml`. The three fe-gpui tests that list that context string are updated, and `git_log_filter_tests::box_caret_keys_edit_inside_the_query` covers it.
- Review round 1: a split of an `EditorView` keeps its read-only state, tab title and chosen language (fe-gpui). Revision opens carry a per-workbench request number and the pane's active tab, so a read that lands late doesn't take focus. Revision comparisons are matched by the full commit id (`Compared::revision`). One whose right side was empty is rebuilt in its tab once the file is back. The disk read's error matters only when no buffer of the file is open.
- Review round 2: a revision open is shown through the pane's tab activation, which counts as a navigation, and the active-tab check is gone. A superseded open's quiet push doesn't block the latest one, in either order, in an empty or a populated pane. Revision comparisons match the full commit id and the exact committed path, so two committed paths that resolve to one working file stay apart.
