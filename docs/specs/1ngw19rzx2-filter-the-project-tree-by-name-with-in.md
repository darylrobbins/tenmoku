---
task_uid: "01m5qphsnb6wgsps1ngw19rzx2"
task: "T320"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-keymap tests/tree_vim_filter_keys.rs: in project_panel with vim on, / resolves to project_panel.filter and shift-f to project_panel.clear_filter on both platforms; with vim off neither resolves; nothing is added in an editor context"
    required: true
    manual: false
  - id: "C2"
    text: "project_panel_filter_tests::live: after /, typed text goes to the filter box (not to the tree's j k bindings) and rows update on each key to the case-insensitive substring matches among listed rows plus their ancestors, expanded; a no-match shows a one-line 'No matches' row"
    required: true
    manual: false
  - id: "C3"
    text: "project_panel_filter_tests::keep_and_clear: Enter leaves the box with the filter kept and j/k moving through matches only; Esc in the box or shift-f clears it and restores the previous expansion and selection"
    required: true
    manual: false
  - id: "C4"
    text: "project_panel_filter_tests::ops: with a filter kept, o/Enter/r/d act on the selected matching row exactly as unfiltered; a rename or delete that removes the last match leaves the filter set with the empty message"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p fe-keymap -p fe-workbench and cargo clippy -p fe-keymap -p fe-workbench --all-targets -- -D warnings pass; no existing test is edited"
    required: true
    manual: false
---

# Filter the project tree by name with / in vim mode: a live filter box that keeps matching rows and their parents, Enter keeps the filter, Esc clears it, as neo-tree's / and nvim-tree's f do (Mac T1128)

## Problem

User 2026-10-24: "Do we offer good vim control of the explorer/outline?" With many files a vim user expects `/` in the tree to narrow it (neo-tree `/` fuzzy filter; nvim-tree `f` live filter, `F` clear). The tree has no filter.

## Current state

project_panel.rs already has a name text field for new/rename (`run_in_field`, `field_focused`, :726-728) and fast-path lists (`only_visible_rows_are_built`, T1071). No filter state or `/` binding (fastedit.toml:830-900). `/` in project_panel is not a vim editor key, so T523's rule is respected.

## Change

- New project_panel_filter.rs (declared from project_panel.rs): `filter: Option<String>`, a view over the existing row list showing only matching rows and ancestors (listed rows only; do not force-list unlisted folders; say so in the empty/No-matches text). Reuse the name field's input for typing rather than a new text widget.
- Commands project_panel.filter and project_panel.clear_filter; keys `/` and `shift-f` in `project_panel && vim` (neo-tree `/`, nvim-tree `F`). Esc inside the box clears.
- Matching is a case-insensitive substring on the file name, as nvim-tree's live filter; fuzzy matching is not in this card.

## Tests

New files only: project_panel_filter_tests.rs (via #[cfg(test)] #[path] from project_panel.rs) and fe-keymap tests/tree_vim_filter_keys.rs.

Lend note (helium lead): spec, scope and criteria are the Mac's full JSON (y57), not truncated.

## Implementation notes

- The filter is `ProjectPanel::filter: Option<filter::Filter>`: the query (and its lower case), the box while it takes
  typing, the expanded folders and selection noted at `/`, the folders it expanded itself, and the shown rows cached by
  `FileTree::version`. Tree row indices stay what selection and the operations use; the list maps its positions to them
  (`list_count`, `list_rows`, `list_pos`, `list_field`), so ops.rs, nav.rs and clipboard.rs are untouched.
- "Listed rows" means every loaded node, collapsed or not: a folder expanded once and collapsed again is searched, and
  its matches' folders are expanded; a folder never listed is not listed to search it (the empty text says "No matches
  in listed folders"). A match's ancestors are queued through `expanding`/`reconcile`, so none does IO on the main
  thread. On each key, folders the filter expanded for a match that's gone collapse again.
- While the box takes typing the panel's key context drops `vim` (as for the name field), and `run_in_filter` mirrors
  `run_in_field`'s routing. Enter, `text_input.submit` or the box losing focus keeps the filter; an empty one is cleared.
- Kept, j/k and gg/G step through matching rows only (ancestor folders are skipped; from a folder selected with `h` or a
  click, j goes on to the next match). Right selects the first child the list shows.
- Clearing (Esc in the box, shift-f) collapses every folder not expanded before `/`, expands again those collapsed since,
  drops folders queued to expand since `/` (so a listing landing later doesn't reopen them), cancels an expand-all under
  way, and restores the selection noted at `/`: none if there was none, the row if it's still there (else the selection
  stays).
- Only rows the list shows are actionable: `selected_index` is `None` for a row the filter hides, so o/Enter/r/d, copy
  path, cut/copy and the rest do nothing to it (review r2t9xx F1). A removed selected row gives the selection to the
  nearest match, or to nothing with no match left. A rename (or reveal) to a name that doesn't match leaves that hidden
  row selected but inert, and the line above the rows names it ("Filter: guide (hides “intro.md”)"); with no match left
  the list says so and the filter stays set (C4).
- Matches the tree gains while the filter is set (a watcher batch, a listing landing, a refresh) have their folders
  expanded (`refilter`, from `apply_fs_changes` and `listed`); a folder collapsed under the filter stays collapsed unless
  a new match appears below it. Unlisted folders still aren't listed to search them (F4).
- `H` reopens the tree with new node ids, so it clears the filter first; any other reopen drops it.
