---
task_uid: "01m5gercaarph22eh3mqnjy1fv"
task: "T1075"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "SearchPanel draws a toolbar row of IconButtons (tooltips Refresh, Clear Search Results, Collapse All / Expand All; debug selectors search-toolbar-refresh, search-toolbar-clear, search-toolbar-collapse) above the query row, reusing git_panel.rs's IconButton pattern; new kiln icons chevron-down, refresh-cw, list-x, fold-vertical, unfold-vertical are added to file_icons.rs"
    required: true
    manual: false
  - id: "C2"
    text: "Refresh is disabled while the query is empty and reruns the current query (keeping options) otherwise; Clear is disabled when the query, replace, include and exclude are all empty and there are no results; Collapse All is disabled with no results. Tests refresh_disabled_without_query, refresh_reruns_query, clear_disabled_when_pristine in search_results_toolbar_tests.rs"
    required: true
    manual: false
  - id: "C3"
    text: "Clear Search Results cancels a running search, empties query, replace, include, exclude and the results and status, keeps the regex/case/word toggles and the replace/details visibility, and focuses the query. Test clear_resets_fields_results_and_focuses_query"
    required: true
    manual: false
  - id: "C4"
    text: "Each file row has a chevron (ChevronRight when collapsed, chevron-down when expanded); clicking the row toggles its group; collapsed files hide their line rows but stay in the results, so Replace All and the counts status still include them. Tests click_file_row_collapses_and_expands, collapsed_file_still_replaced_by_replace_all"
    required: true
    manual: false
  - id: "C5"
    text: "In the focused results list Up/Down move through file rows and line rows; Left on an expanded file collapses it, Left on a line or a collapsed file selects the parent file; Right on a collapsed file expands it, Right on an expanded file selects its first line; Enter on a file row opens the file at its first match. Commands search.collapse_or_parent and search.expand_or_first_child are bound to left/right in context search_results. Tests left_right_enter_on_file_rows, left_on_line_selects_parent; binding test search_results_collapse_bindings.rs (Linux-OK)"
    required: true
    manual: false
  - id: "C6"
    text: "Collapse All collapses every file group; once every group is collapsed the same button shows Expand All (icon and tooltip flip) and expands them all; a new search, refresh or dismiss leaves new rows expanded. Commands search.collapse_all, search.expand_all, search.refresh, search.clear_results are in the command table (palette titles Collapse All, Expand All, Refresh, Clear Search Results). Tests collapse_all_flips_to_expand_all, new_search_resets_collapse"
    required: true
    manual: false
  - id: "C7"
    text: "GPUI view tests in search_results_toolbar_tests.rs are Mac-only; the keymap binding test is Linux-OK; existing search_panel_filter_tests.rs and search_replace_tests.rs stay green (edit in place any assertion that selection skips file rows); no screenshot goldens change"
    required: true
    manual: false
---

# Add the search results toolbar (Refresh, Clear Search Results, Collapse All / Expand All) and collapsible file groups, as VS Code's Search view has

User request 2026-10-21: "Let's add some polish to the search in files pane. Follow after VS Code." Approved follow-up: "yes, file the results toolbar card too".

## Problem
The Search view's results have no toolbar and file groups cannot be collapsed, only dismissed. crates/fe-workbench/src/search_panel.rs: render (~:1287) draws only the query row, replace, include/exclude and the results list; `dismiss` (:688) is the only way to remove a file group; `ResultRow` (:114) is `File | Line` with no collapsed state; `select_next` (~:715) and `OpenSelected` (:470) only deal with Line rows; fastedit.toml:653-681 binds only up/down/enter (and j/k in vim) in `search_results`. The sidebar has no title-action slot (the dock title row in kiln-shell dock.rs is the name only), so the toolbar lives inside the panel.

## Change (copy VS Code's Search view, searchView.ts / searchActions.ts)
- Toolbar row inside SearchPanel::render, above the query row, right-aligned, IconButtons as git_panel.rs:1005-1040 draws them (tooltip, `.disabled(..)`). Add kiln icons chevron-down, refresh-cw, list-x (Clear), fold-vertical (Collapse All), unfold-vertical (Expand All) to kiln/crates/kiln-ui (svg + `icons!` entry in file_icons.rs). Commands search.refresh ("Refresh"), search.clear_results ("Clear Search Results"), search.collapse_all ("Collapse All"), search.expand_all ("Expand All"), registered in the table near :1470 (context `search`, new SearchOp variants).
- Refresh: reruns the current query with current options and filters; disabled while the query is empty.
- Clear Search Results: cancels a running search; empties query, replace, include and exclude, results and status; keeps the regex/case/word toggles and whether replace/details are shown; focuses the query input. Disabled when query, replace, include, exclude are all empty and there are no results. If in doubt about an enablement rule, check VS Code's SearchView.updateActions.
- Collapse All / Expand All: one button. It shows Collapse All while any file group is expanded; once every group is collapsed it flips to Expand All (icon and tooltip), which expands all. Disabled with no results.
- File groups: add collapsed state (a set of file indices reindexed on dismiss, reset on every new run or refresh so fresh results are expanded); rebuild `rows` through one helper used by `append`, `restart`, `dismiss` and collapse changes. File row gets a leading chevron (ChevronRight collapsed, chevron-down expanded). Clicking the file row toggles it. Collapsing hides its line rows only: the file stays in `files`, so counts, status text and Replace All / per-file Replace still include it. If the selected line row is hidden by a collapse, select its file row.
- Keyboard in the focused results list: Up/Down now also stop on file rows (as VS Code's tree); Left collapses an expanded file, else selects the parent file from a line; Right expands a collapsed file, else selects its first line; Enter on a file row opens the file at its first match, Enter on a line as today. New commands search.collapse_or_parent and search.expand_or_first_child bound to left/right in context `search_results` in crates/fe-keymap/keymaps/fastedit.toml (not in vim-only; j/k stay as they are, h/l not added).
- Depends on T1074 (same files; the toolbar sits above its query row and the "..." button).

## Tests
- New crates/fe-workbench/src/search_results_toolbar_tests.rs, wired with `#[cfg(test)] #[path = "search_results_toolbar_tests.rs"] mod results_toolbar_tests;` beside the filter_tests mod at the end of search_panel.rs. Mac-only (GPUI view tests). Functions: refresh_disabled_without_query, refresh_reruns_query, clear_disabled_when_pristine, clear_resets_fields_results_and_focuses_query, click_file_row_collapses_and_expands, collapsed_file_still_replaced_by_replace_all, left_right_enter_on_file_rows, left_on_line_selects_parent, collapse_all_flips_to_expand_all, new_search_resets_collapse.
- New crates/fe-keymap/tests/search_results_collapse_bindings.rs (Linux-OK): left/right resolve to the two commands in search_results and nowhere else.
- Edit in place any search_panel_filter_tests.rs / search_replace_tests.rs assertion that selection skips file rows. No screenshot goldens change (no scene shows the search panel).
