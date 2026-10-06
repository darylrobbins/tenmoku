---
task_uid: "01m46tzw9zwssfm3xnh84byn6k"
task: "T78"
rev: 6
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::outline_panel::shows_the_active_tabs_outline: with a Rust file open, cmd-shift-o shows the Outline panel whose rows are fe_syntax::outline items in pre-order, indented by depth; activating a Markdown tab shows its headings; focus stays in the editor"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::outline_panel::refreshes_after_edits_off_the_main_thread: 10 edits made without running the executor in between, then running it, start at most 2 outline jobs (test counter), the jobs run on the background executor, and the rows then equal the outline of the final text"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::outline_panel::keeps_the_last_outline_while_reparsing: right after an edit, before the reparse lands, the rows are still the previous outline and not empty"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::outline_panel::clicking_an_item_jumps: clicking a row leaves the editor it came from focused with a caret at that item's name_range.start"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::outline_panel::empty_and_too_large_states: no tab shows No file open; a .txt file shows No outline for plain text; a JSON file shows No outline for json (the language name); a Rust file over the parse limit shows File too large for an outline; an empty Rust file shows No symbols"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::outline_panel::a_capped_outline_builds_only_visible_rows: a Rust file of 20,000 one-line fns shows the Showing the first 10,000 symbols footer and a frame builds no more rows than fit plus overscan"
    required: true
    manual: false
  - id: "C7"
    text: "outline.show (cmd-shift-o) is registered through the ACTION_TASK command registry; rg 'KeyBinding::new|actions!' finds no match in the files this card adds; Cargo.lock changes at most in fe-workbench's own dependency list (fe-syntax), adding no package"
    required: true
    manual: false
---

# Add an outline panel that follows the active editor

# Add an outline panel that follows the active editor

Milestone M3 "Browse", third batch. It builds on:

- `fe_syntax`'s outline API (T64, with T66's fixes): `outline_job(&Syntax) -> Result<OutlineJob,
  NoOutline>`, `OutlineJob::run` (`Send`), and `Outline { items, truncated, version }`. Each item
  carries `name`, `kind`, `range`, `name_range`, `row`, `depth` and `parent`, in pre-order, capped at
  `MAX_OUTLINE_ITEMS` (10,000).
- `EditorView::syntax()`
- the search card's `Dock`
- the select-and-reveal card's `EditorView::select_and_reveal`
- the action-model task's command registry

The plan's Architecture table gives `fe-workbench` the "outline".

## Problem

There's no way to see a file's structure or to jump to a symbol. The outline must keep up with edits,
without running a query over a large file on the UI thread for every keystroke.

## Change

New file `crates/fe-workbench/src/outline_panel.rs`: `OutlinePanel`, added to `Dock` as a third panel.
The only `dock.rs` change is that entry.

- **It follows the active editor.**
  - The workbench tells the panel when the active tab changes. That's a small hunk in `workbench.rs`.
    Any "active editor changed" hook should be one the splits card can keep firing per active pane.
  - The panel observes that `EditorView` entity (`cx.observe`). The view notifies on edits and when a
    background parse lands.
- **Refreshing is off the main thread, and bounded.**
  - On a notify, the panel refreshes when the view's `Syntax` has a reparsed tree (`outline_job`
    returns `Ok`) at a version other than the outline it shows.
  - The refresh builds the `OutlineJob` on the main thread, which clones the tree and the text, and runs
    `OutlineJob::run` on `cx.background_spawn`.
  - At most one job is in flight per panel. Notifies that arrive meanwhile set a dirty flag, and one
    follow-up job runs when the current one lands. Jobs never queue.
  - A result for a tab that's no longer active is dropped.
  - A test-only counter records jobs started.
- **What it shows:**
  - The items, in pre-order, through a `uniform_list`, building only the visible rows.
  - Each row is fe-ui's `TreeRow`, indented by `depth`, showing `name`. There's no chevron and no
    collapsing.
  - Header and empty states:
    - "No file open" when there's no active tab
    - "No outline for plain text" when there's no syntax
    - "No outline for <language>" for `NoOutline::Unsupported`
    - "File too large for an outline" for `NoOutline::TooLarge`
    - "No symbols" when an outline has 0 items
    - a muted footer "Showing the first 10,000 symbols" when `truncated` is set
  - While the language is still loading, or for `NoOutline::NotParsed` after an edit, the panel keeps
    showing the last outline of that tab (empty at first, "Parsing…"). It never flickers to empty
    between keystrokes.
- **Clicking an item jumps to it.** The panel calls `select_and_reveal(name_range.start ..
  name_range.start)` on the editor the outline came from, and focuses that editor. If the buffer
  moved past `Outline::version`, `select_and_reveal`'s clamping covers stale offsets until the next
  refresh lands.
- `crates/fe-workbench/Cargo.toml`: add `fe-syntax.workspace = true` if T63 didn't. That changes only
  `fe-workbench`'s entry in `Cargo.lock`, which needs a person's approval at merge, and adds no new
  crate.
- `crates/fe-workbench/src/lib.rs`: a `mod` line, and one line in `init` to register commands.

### Commands

Register through the action-model task's registry. Don't add `KeyBinding::new` or `actions!`.

| ID | Default binding | Context | Action |
|---|---|---|---|
| `outline.show` | `cmd-shift-o` | `Workbench` | Shows the dock and switches it to Outline. Focus stays in the editor. |

### Tests

New file `crates/fe-workbench/src/tests/outline_panel.rs`, declared in `tests/mod.rs`. It uses headless
GPUI and small Rust, JSON and Markdown files written into a `tempfile` dir. A test-only counter records
rows built.

## Out of scope

- Highlighting the item at the caret (follow-the-caret, using `Outline::item_at`).
- Collapsing items.
- Kind icons.
- Filtering.
- Breadcrumbs.
- A "go to symbol" picker (M4).
- Outlines of injected layers.

## Decided (lead defaults; the user may revisit)

- **The outline is a dock panel** next to Project and Search, not an always-visible split under the
  tree.
- `cmd-shift-o` shows the panel. This is the VS Code "go to symbol" key. M4's symbol picker or M5's
  keymaps may take it back.
- Rows show names only (no kind glyph), fully expanded.
- During a reparse the previous outline stays on screen. The panel doesn't blank out.
