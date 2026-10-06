---
task_uid: "01m46tzw83e83eahbjhqsvdctp"
task: "T77"
rev: 6
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::search_panel::the_dock_switches_panels: cmd-shift-f shows Search with its input focused; cmd-shift-e shows Project again with its rows unchanged and FileTree stats().listings not raised; cmd-b hides the dock and cmd-b shows it again"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::search_panel::results_stream_grouped_by_file: with threads 1, enter on a needle present in 3 files (twice on one line of one) gives one header row per file with its match count, one row per matching line with its line number and highlighted ranges equal to the LineMatch ranges in the preview, and the status 4 results in 3 files"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::search_panel::options_toggle_and_rerun: clicking the regex toggle reruns fo+[0-9] and finds foo1; alt-cmd-c makes FOO stop matching foo; alt-cmd-w skips foobar; an invalid regex shows its error in the status line and no result rows"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::search_panel::a_new_search_cancels_the_old: submitting a second query before the executor runs sets the first run's cancel flag, and after it runs every result row comes from the second query"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::search_panel::capped_results_say_so: with max_matches set to 10 by the test hook over 20 matches, 10 match rows show and the status reads Showing the first 10 results (the cap value)"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::search_panel::clicking_a_match_selects_it: clicking a match row opens the file in a focused tab with the selection equal to the match range in the buffer; the same holds for a CRLF file and for a file already open (its tab activates, tab count unchanged)"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test tests::search_panel::only_visible_rows_are_built: with 1,000 matching lines, a frame builds no more result rows than fit in the panel plus the uniform_list overscan"
    required: true
    manual: false
  - id: "C8"
    text: "search.show (cmd-shift-f), dock.show_project (cmd-shift-e), dock.toggle (cmd-b) and search.toggle_regex/toggle_case/toggle_whole_word are registered through the ACTION_TASK command registry; rg 'KeyBinding::new|actions!' finds no match in the files this card adds; Cargo.toml and Cargo.lock are unchanged"
    required: true
    manual: false
---

# Add the project search panel to the right dock

# Add the project search panel to the right dock

Milestone M3 "Browse", third batch. It builds on:

- T65's `fe_project::search`: `SearchQuery`, `SearchOptions`, the streamed `FileMatches`/`LineMatch`
  batches, `SearchSummary` with `truncated` and `cancelled`, and the cancel flag
- T63's workbench shell
- the quick-open card's `TextInput` and `HighlightedLabel`
- the select-and-reveal card's `EditorView::select_and_reveal`
- the action-model task's command registry

The plan's Architecture table gives `fe-workbench` the "search panel".

## Problem

T65 can search the project, but there's no UI for it, and no way to get from a match to the file.

## Change

### The right dock

New file `crates/fe-workbench/src/dock.rs`: `Dock`, which owns the right dock's panels. T63 renders
`ProjectPanel` there directly; this card moves that rendering into `Dock`.

- Exactly one panel shows at a time: Project, or Search (the outline card adds Outline).
- A header strip of panel names switches between them. It uses fe-ui's `TabBar`.
- Switching keeps each panel's state. The tree isn't re-listed, and search results stay.
- The dock can be hidden. Its width stays T63's 240 px.
- Changes to `workbench.rs` are limited to replacing the panel render with the `Dock` and wiring the
  commands.

### The search panel

New file `crates/fe-workbench/src/search_panel.rs`: `SearchPanel`.

- **Query and options:**
  - a `TextInput` for the query
  - three toggles: regex (`.*`), match case (`Aa`) and whole word (`ab`). New
    `crates/fe-ui/src/toggle_button.rs`: `ToggleButton`, a `RenderOnce` with on and off styles from
    `UiTheme`.
  - Defaults are T65's: literal, case-insensitive, not whole word.
- **Running a search:**
  - Enter in the input runs it. Flipping a toggle reruns it when the query isn't empty.
  - Each run gets a new generation and a new `cancel` flag. Starting a run sets the previous run's
    flag. Batches carrying an old generation are dropped.
  - `fe_project::search` runs on `cx.background_spawn`, with its `sink` sending batches over the
    channel T63 uses for watcher batches. Don't add a new crate. A foreground task appends the
    batches to the panel's results.
  - Closing the workbench, or dropping the panel, cancels the run.
- **Results:**
  - They're flattened into rows for a `uniform_list`, with only the visible rows built:
    - one header row per file: the relative path and its match count
    - one row per `LineMatch`: the 1-based line number and the preview in a `HighlightedLabel`, with
      the match ranges shifted by `preview_start` and clipped to the preview
  - Files appear in arrival order and aren't re-sorted, so rows don't jump while results stream in.
- **Status line:**
  - "Searching…" while the search runs
  - "N results in M files" when it's done
  - "No results"
  - "Showing the first N results" (N = `max_matches`, 10,000 by default) when `truncated` is set
  - the `InvalidPattern` message, in the error colour, with no result rows
- **Opening a match:** clicking a match row opens the file in the active pane, through the same entry
  point as a tree click.
  - Once the editor exists, the match is selected with `select_and_reveal` and the editor is focused.
    For a file that's already open, this happens immediately.
  - The target is computed from `line_number - 1` and the first range in the line, against the
    buffer's text: the start of that row plus the in-line byte range. It is not computed from
    `line_offset`, so files with CRLF line endings land correctly. `select_and_reveal` clamps
    anything stale.
  - If `pane.rs` needs a "reveal this range once the tab's editor exists" hook for files still
    loading, add it there, keeping it small.
- `crates/fe-workbench/src/lib.rs`: `mod` lines, and one line in `init` to register commands.
  `crates/fe-ui/src/lib.rs`: `mod` and `pub use` lines.

### Commands

Register every new command through the action-model task's registry. Don't add `KeyBinding::new` or
`actions!`. Proposed IDs:

| ID | Default binding | Context |
|---|---|---|
| `search.show` | `cmd-shift-f` | `Workbench`; shows the dock, switches to Search and focuses the input |
| `dock.show_project` | `cmd-shift-e` | `Workbench` |
| `dock.toggle` | `cmd-b` | `Workbench` |
| `search.toggle_regex` | `alt-cmd-r` | `SearchPanel` |
| `search.toggle_case` | `alt-cmd-c` | `SearchPanel` |
| `search.toggle_whole_word` | `alt-cmd-w` | `SearchPanel` |

`search.run` uses the `TextInput`'s submit, so it needs no binding of its own.

### Tests

New file `crates/fe-workbench/src/tests/search_panel.rs`, declared in `tests/mod.rs`. It uses headless
GPUI. Fixtures are written into a `tempfile` dir.

- A test-only hook sets `SearchOptions` (`threads: 1` for deterministic order, and a low
  `max_matches`).
- A test-only counter records rows built in the last frame.
- Clicks go to laid-out row bounds, as in T63's tests.

## Out of scope

- Replace.
- Include and exclude globs.
- Search-as-you-type.
- Collapsing file groups.
- Highlighting every match inside the editor.
- Searching unsaved buffers.
- Sorting results.
- A resizable dock.
- Moving between results with keys (F4 and similar).

## Decided (lead defaults; the user may revisit)

- **Search lives in the right dock**, switching with the tree and outline (one panel at a time), as
  VS Code does in its sidebar. It's not a centre tab like Zed's.
- **A search runs on Enter**, not as you type, so typing never starts walks over a large repo.
  Toggles rerun the search.
- **Clicking a match selects it** (the caret is at its end), rather than placing a bare caret at its
  start.
- Results appear in arrival order. File groups are always expanded.
- Dock bindings follow VS Code: `cmd-shift-e`, `cmd-shift-f`, `cmd-b`.

## Implementation notes

- **Enter in the search input** is `text_input.submit`, which calls `TextInput::submit`; the panel
  runs the query on its `Submitted` event, so there is no `search.run` command. It's bound to `enter`
  in `search_panel && text_input` only: a plain `text_input` binding would be deeper than the
  palettes' `picker.confirm` and shadow it.
- **Contexts** follow the existing lowercase names: the spec's `Workbench` is `workbench`, and
  `SearchPanel` is `search_panel`.
- **Registry:** the dock and search commands are one `fe-command` registry in `dock.rs`
  (`dock::init`, called from `fe_workbench::init`). Besides the `mod` lines and that `init` line,
  `lib.rs` re-exports `Dock`, `DockPanel` and `SearchPanel` and chains the registry into
  `command_catalog()`, without which `KeymapStore` drops the new bindings. The seven new keymap
  rows also move the binding counts that fe-gpui's `command_tests.rs` and `keymap_tests.rs` pin
  (61 → 68; 14 → 21 for fe-workbench's contexts).
- **Opening a match** goes through `Pane::open_at(path, Some(Reveal { row, columns }))`, which
  `Pane::open` now calls with `None`. For a file being read, the reveal is kept with its path and
  applied in `file_read` only if that file is still the one opened last; any later open clears it.
- **Dock header:** fe-ui's `Tab` always draws its close mark; the dock's tabs have no close handler,
  so clicking it switches to that panel like the rest of the tab.
