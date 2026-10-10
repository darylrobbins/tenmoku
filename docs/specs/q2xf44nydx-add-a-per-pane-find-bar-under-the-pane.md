---
task_uid: "01m4fhv8etkkp77tq2xf44nydx"
task: "T265"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::find_bar::show_prefill_and_type: find.show over an editor with foo selected opens the pane's find bar under its toolbar, holding foo, with its input focused; typing extends the query and selects the first match at or after the anchor; the label reads 1 of 3 once counted; a query with no match reads No results and leaves the selection"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::find_bar::next_previous_and_toggles: find.next and find.previous step and wrap from the input and from the editor; each toggle changes the matches as FindQuery says; ( with regex on shows the error in the bar and clears the highlight; with the bar closed, find.next reuses the last query, and with no query it does nothing"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::find_bar::close_tabs_and_panes: find.close hides the bar, clears the editor's find and focuses the editor with the match selected; switching the pane's tab while the bar is open clears the old editor's find and sets the new one's without moving its caret; a second pane's editor is unaffected"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::find_bar::vim_types_in_the_bar: with vim on in normal mode, j and d typed in the bar extend the query (its context carries no vim.mode) and find.next leaves the caret on the match's start"
    required: true
    manual: false
  - id: "C5"
    text: "The seven find.* commands are in fe_workbench::command_catalog(); the default keymap, workbench.rs and everything under crates/fe-vim and crates/fe-gpui are unchanged; every earlier fe-workbench test passes with its files unchanged"
    required: true
    manual: false
---

# Add a per-pane find bar under the pane toolbar: type-ahead, next and previous, case, whole-word and regex toggles, and an n-of-m count

## Problem

Card 3 of 4 for in-file find, asked by the user on 2026-10-09: "In-file find (cmd-F) reusing T213's row
matcher". Cards 1–2 give `FindQuery`/`Pattern::from_query` and `EditorView::{set_find, find_next,
find_previous, find_from, find_count}`. There's no UI to type a query into.

## Where the bar goes (decided)

**A row in the pane, under its toolbar and above the editor, applying to the pane's active editor** (as VS
Code's and Zed's per-editor find bars). Not the status bar: that slot is Vim's single, workbench-wide `/ ? :`
line (T219, `crates/fe-workbench/src/vim_line.rs`, "replaces the status bar while open"), and a find bar stays
open while you edit, keeps per-pane toggles, and must coexist with Vim's line when vim is on.

## Change

New `crates/fe-workbench/src/find_bar.rs`: the bar's state and render, and `registry()` with these commands
(the search panel's `search_panel.rs` is the model for the `TextInput`, the three `fe-ui` `ToggleButton`s and
the registry):

| ID | Title | Context |
|---|---|---|
| `find.show` | Find | `editor` |
| `find.next` | Find Next | `editor \|\| find_bar` |
| `find.previous` | Find Previous | `editor \|\| find_bar` |
| `find.close` | Close Find | `find_bar` |
| `find.toggle_case` | Match Case | `find_bar` |
| `find.toggle_whole_word` | Match Whole Word | `find_bar` |
| `find.toggle_regex` | Use Regular Expression | `find_bar` |

- `crates/fe-workbench/src/lib.rs`: `mod find_bar;` and `find_bar::registry()` in `command_catalog()` (two
  lines).
- `crates/fe-workbench/src/pane.rs`: the pane owns an optional find bar, renders it under `toolbar_row`, and
  handles `RunCommand` for `find.*` on its root (propagating anything else), so `workbench.rs` doesn't change.
- `crates/fe-workbench/Cargo.toml`: `fe-vim.workspace = true`.
- Behaviour:
  - `find.show` opens the bar (or refocuses it) with focus in its input. A non-empty single-row selection
    replaces the query; otherwise the last query stays. The anchor is the selection's start.
  - Each change to the query or a toggle builds `Pattern::from_query`, calls `set_find`, and `find_from(anchor)`.
    An error (`PatternError`'s message) shows in the bar and clears the highlight. An empty query clears it.
  - `find.next`/`find.previous` step and wrap, from the input or the editor, and move the anchor to the match.
    With the bar closed they reuse the pane's last query without opening it; with none they do nothing.
  - A label reads `3 of 12`, `? of 12` when the selection isn't a match, `10000+` when capped, `No results`, or
    nothing while counting (from `find_count`).
  - `find.close` hides the bar, `set_find(None)`, and focuses the editor with the match left selected.
  - When the pane's active tab changes while the bar is open, the old editor's find is cleared and the new
    one's set, without moving its caret. Other panes are unaffected.
  - The bar's key context is `find_bar` (its input adds `text_input`) and never carries `vim.mode`, so with vim
    on its keys only type.

## Tests (new file)

`crates/fe-workbench/src/tests/find_bar.rs`, declared in `crates/fe-workbench/src/tests/mod.rs`; they dispatch
the commands (no keymap change here).

## Size

About 350 non-test lines.

## Out of scope

Key bindings (card 4), Edit menu items, replace, find in selection, history, caret movement inside the input
(`TextInput` can't yet), any `crates/fe-vim` or `crates/fe-gpui` change. Theme: colours come from the `UiTheme`
global the way `search_panel.rs` reads them, so a live-theme sweep (T247) covers the bar like the other panels.
