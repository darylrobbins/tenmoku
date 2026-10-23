---
task_uid: "01m5gerss154k7r6d2jn31fbqx"
task: "T1076"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "The toolbar from T1075 gains a View as Tree / View as List button (debug selector search-toolbar-view-mode; list-tree icon and tooltip View as Tree while in list view, list icon and tooltip View as List while in tree view); commands search.view_as_tree and search.view_as_list ('View as Tree', 'View as List'); the default is list, kept per panel in memory, and switching keeps the query, results and selection of the same match"
    required: true
    manual: false
  - id: "C2"
    text: "In tree view results are grouped under folder rows by path relative to the search root: files at the root sit at top level, each folder row has the T1075 chevron and its name; single-child folder chains are compacted into one row labelled a/b/c; a folder holding several folders or files is not merged; file rows show the file name; indentation grows one level per depth. Tests tree_groups_files_under_folders, tree_compacts_single_child_folder_chains, tree_does_not_compact_branching_folders"
    required: true
    manual: false
  - id: "C3"
    text: "Folder rows collapse and expand by click, Left/Right and Collapse All / Expand All (the flip rule counts folders and files); collapsing a folder hides its files and lines but they stay in the results for Replace All; a folder row shows the total match count under it; Enter on a folder toggles it. Tests collapsing_a_folder_hides_descendants_but_replace_all_keeps_them, collapse_all_counts_folders, folder_row_shows_total_matches"
    required: true
    manual: false
  - id: "C4"
    text: "Dismiss on a folder row (the existing x) drops every file under it; Replace on a folder row replaces in each file under it; switching back to List restores the flat file rows. Tests dismiss_folder_drops_its_files, replace_in_folder_row, switching_back_to_list"
    required: true
    manual: false
  - id: "C5"
    text: "All GPUI view tests are Mac-only, in the new search_results_tree_tests.rs wired with #[path] mod in search_panel.rs; Up/Down/Left/Right tests from T1075 stay green; no screenshot goldens change"
    required: true
    manual: false
---

# Add View as Tree / View as List to the search results: folder-grouped results with compacted single-child folders, as VS Code's Search view has

User request 2026-10-21: "Let's add some polish to the search in files pane. Follow after VS Code." Approved follow-up: "yes, file the results toolbar card too" (View as Tree / View as List is part of that toolbar; split out of T1075 because the tree needs a new row type).

## Problem
Search results are only a flat list of file groups (search_panel.rs `ResultRow::{File, Line}` :114, rows built in `append` :354 / `dismiss` :688, drawn in `render_rows` :738). VS Code's Search view has View as Tree / View as List (search.action.viewAsTree / viewAsList) in its title toolbar; the tree groups files by folder.

## Change (copy VS Code)
- Add `ResultRow::Folder(id)` and a `view_as_tree: bool` (default false) to SearchPanel; build `rows` for tree view from the file paths relative to the search root: a folder node per directory, compacting a chain of folders that each hold exactly one child folder and no files into one node whose label is the joined path (a/b/c); order folders before files, each by path; file rows show the file name; indent per depth.
- Collapsed state covers folder nodes as well as files (reuse T1075's set, keyed so it survives mode switches or resets with them; reset on new runs). Collapse All / Expand All flip over all collapsible nodes shown.
- Folder row: chevron, name, total match count, and the existing Replace (replace_in_file for every file under it) and x (dismiss every file under it) buttons; click, Left/Right and Enter toggle it.
- Toolbar button and commands search.view_as_tree / search.view_as_list as in the criteria; a mode switch re-lays the same results and keeps the selected match selected.
- Depends on T1074 and T1075. Do not add the search.defaultViewMode setting here.

## Tests
New crates/fe-workbench/src/search_results_tree_tests.rs (Mac-only GPUI view tests), wired with #[cfg(test)] #[path] mod beside the other test mods at the end of search_panel.rs: tree_groups_files_under_folders, tree_compacts_single_child_folder_chains, tree_does_not_compact_branching_folders, collapsing_a_folder_hides_descendants_but_replace_all_keeps_them, collapse_all_counts_folders, folder_row_shows_total_matches, dismiss_folder_drops_its_files, replace_in_folder_row, switching_back_to_list. No screenshot goldens change.

## Implementation notes
- The toolbar's View as Tree / View as List button sits before Collapse All, as in VS Code, so T1075's toolbar tests index Collapse All as the 4th button. Its icons (`list-tree`, `list`) were already bundled; `file_icons.rs` needed no change.
- Folders are keyed by their native paths from the search root (`FileMatches.path` less the root), not the lossy `rel`, so two non-UTF-8 names that read the same stay two folders; only labels are lossy.
- Folder collapse is a separate set of directory paths (`collapsed_folders`), cleared with the files' on a new run, and kept across view switches; the list ignores it. A compacted row is collapsed if any folder in its chain is, so a collapse survives a recompaction (e.g. after a dismissal); expanding the row expands every folder in its chain. Chevrons, visibility, toggling and the toolbar flip all read that resolved state.
- Switching to the tree expands the folders the selected file or match is in, so the same match stays selected there and back in the list. Collapse All in the list collapses files only; in the tree, every folder and file. The flip to Expand All holds when no folder or file row shown is expanded.
- Folder rows are rebuilt with the rows, so a selection is kept across rebuilds by what it shows (a folder by path), falling back to its line's file, then the deepest shown folder holding it, then (a folder, in the list) its first file.
- The tree's rows are rebuilt on each streamed batch, so they move as results come in, as VS Code's sorted tree does; the list still only appends.
- Adding `ShownRow::Folder` touched the exhaustive `ShownRow` matches in the other search test files.
