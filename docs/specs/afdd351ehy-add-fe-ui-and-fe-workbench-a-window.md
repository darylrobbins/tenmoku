---
task_uid: "01m46hs5rkxgfrzcafdd351ehy"
task: "T63"
rev: 8
type: "feature"
complexity: "l"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::project_panel::the_root_lists_off_the_main_thread: right after Workbench::new with a root the tree has 0 loaded entries; after the background executor runs, the panel rows are the root entries in tree order"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::project_panel::clicking_a_directory_toggles_it: a click at a directory row laid-out bounds shows its children one level deeper once the listing lands; a second click hides them with no further listing"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::project_panel::only_visible_rows_are_built: with a directory of 100,000 files expanded (fe_project::fixture), a frame builds no more row elements than fit in the panel plus the uniform_list overscan, and scrolling to the end shows the last file"
    required: true
    manual: false
  - id: "C4"
    text: "fe-workbench test tests::project_panel::fs_changes_update_rows: a batch given to ProjectPanel::apply_fs_changes after creating and deleting files on disk adds and removes rows in expanded directories"
    required: true
    manual: false
  - id: "C5"
    text: "fe-workbench test tests::tabs::clicking_a_file_opens_a_focused_tab: a click on a file row opens a tab whose editor model path is that file and whose text equals the file contents once the background read lands; the tab is active, its editor is focused, and the window title names the file"
    required: true
    manual: false
  - id: "C6"
    text: "fe-workbench test tests::tabs::opening_an_open_file_activates_its_tab: after opening two files, clicking the first file again activates its tab and the tab count stays 2"
    required: true
    manual: false
  - id: "C7"
    text: "fe-workbench test tests::tabs::switching_and_closing_tabs: clicking a tab activates it; cmd-shift-] and cmd-shift-[ cycle tabs; cmd-w closes the active clean tab and activates its right neighbour, or the left one when it was last; closing the last tab shows the empty-pane placeholder; cmd-w on a dirty tab leaves it open"
    required: true
    manual: false
  - id: "C8"
    text: "fe-workbench test tests::launch::resolve_launch_picks_root_and_file: a directory argument gives that root and no file; a file or a missing path gives its parent as root and opens the file; no argument gives no root and the welcome tab"
    required: true
    manual: false
  - id: "C9"
    text: "The loader tests moved from crates/fastedit/src/loader.rs pass in crates/fe-workbench/src/loader.rs, the crates/fastedit/src/perf.rs tests still pass, and crates/fastedit/src/main.rs no longer defines a Workbench"
    required: true
    manual: false
  - id: "C10"
    text: "fe-ui depends on gpui and on no workspace crate; scripts/check-no-gpui.sh passes and still prints ok: fe-project; nothing under crates/fe-display changes, and under crates/fe-gpui only crates/fe-gpui/src/keymap_tests.rs and crates/fe-gpui/src/command_tests.rs change, and only to raise their pinned default binding count to include the workbench.* rows and to skip workbench-context bindings in the command-catalog check"
    required: true
    manual: false
  - id: "C11"
    text: "Every fe-ui or fe-workbench file copied or adapted from gpui-kit starts with a header naming the source, commit and Apache-2.0 and has a THIRD_PARTY.md row (and NOTICE names the use); nothing is taken from Zed GPL crates"
    required: true
    manual: false
---

# Add fe-ui and fe-workbench: a window shell with a right-docked file tree and editor tabs

# Add fe-ui and fe-workbench: a window shell with a right-docked file tree and editor tabs

Milestone M3 "Browse", card 3 of the first batch. Depends on card 1 (`fe-project`'s `FileTree`,
`FsChange`, `ProjectWatcher`, and `fixture`). The plan's Architecture table:

- `fe-ui`: "copied gpui-kit components restyled with our theme tokens; the only home of UI primitives".
- `fe-workbench`: "our own lazy file tree on the right dock, tabs and splits, Cmd-P, palette, search
  panel, outline".

This card covers the shell, the right-docked tree and tabs. Splits, Cmd-P, project search and outline
are later cards.

## Problem

`crates/fastedit/src/main.rs` has its own `Workbench` view: one `EditorView` and a hard-coded
"PROJECT" placeholder panel. `crates/fastedit/src/perf.rs` reaches into it with `crate::Workbench` and
`.editor()`. A host app can't reuse any of it, and there is no way to browse a folder or open more
than one file.

## Change

### fe-ui (new crate, GPUI)

Files: `crates/fe-ui/Cargo.toml`, `crates/fe-ui/src/lib.rs`, `crates/fe-ui/src/theme.rs`,
`crates/fe-ui/src/tab_bar.rs`, `crates/fe-ui/src/tree_row.rs`.

- `UiTheme`: tokens for panel background, border, text, muted text, row hover and selected, tab
  active and inactive, and a dirty dot. `UiTheme::default_dark()` matches today's hard-coded colours
  (`0x121418` panel, `0x23262d` border, `0x8b8f98` text) and `EditorTheme::default_dark`'s background.
- `TabBar` and `Tab` (label, dirty dot, close affordance) and `TreeRow` (indent per depth, a chevron for
  directories, label, ignored and error styling) as stateless `RenderOnce` elements. `fe-ui` depends on
  `gpui` only: no `fe-project`, no `fe-workbench`.
- Copying from gpui-kit (Apache-2.0) is allowed per `CLEAN_ROOM.md`:
  - each copied or adapted file starts with the source, commit and licence header
  - each gets a row in `THIRD_PARTY.md`
  - `NOTICE`'s gpui-kit line names the new use

  Never open Zed's GPL crates (`project_panel`, `workspace`, `ui`, `picker`, `fuzzy` …).

### fe-workbench (new crate, GPUI)

Files: `crates/fe-workbench/Cargo.toml`, `crates/fe-workbench/src/lib.rs`, `crates/fe-workbench/src/workbench.rs`,
`crates/fe-workbench/src/project_panel.rs`, `crates/fe-workbench/src/pane.rs`,
`crates/fe-workbench/src/loader.rs` (moved from `crates/fastedit/src/loader.rs` with its tests).

- `Workbench` (moved out of `main.rs` and generalised):
  - `Workbench::new(launch: Launch, theme, cx)`, where `Launch { root: Option<PathBuf>, file: Option<(PathBuf, Option<Buffer>)> }`
    keeps fastedit's existing "read started at the top of `main`, shown on the first frame if
    finished" path.
  - `active_editor() -> Option<&Entity<EditorView>>` replaces `editor()`.
  - The window title comes from the active tab (`name ● — fastedit`, as today).
- `resolve_launch(arg: Option<PathBuf>) -> Launch`:
  - a directory becomes the root with no file
  - a file, or a missing path (a new file, as today), opens in a tab, with its parent directory as the
    root
  - no argument means no root: the panel says "No folder open" and the welcome text is in a tab
- `ProjectPanel` on the right dock, at a fixed 240 px width for now:
  - It holds a `fe_project::FileTree` and renders `row_count()` rows through GPUI's `uniform_list`,
    reading `rows(range)` for the visible range only.
  - Opening the root and every expansion run `FileTree::listing(id)` on `cx.background_spawn`. The
    result goes back to `FileTree::fill` on the main thread, so `Workbench::new` does no filesystem IO
    on the calling thread.
  - Clicking a directory row toggles it. Clicking a file row opens it.
  - A `ProjectWatcher` sends batches through an async channel, and a foreground task applies them with
    `FileTree::apply`. That same entry point, `ProjectPanel::apply_fs_changes`, is what tests call.
  - A test-only counter records how many row elements the last frame built.
- `Pane` holds the tabs: `Vec<Tab { path, editor: Entity<EditorView> }>` and an active index.
  - Opening a file reads it on a background thread via `loader.rs` and then builds
    `EditorView::new(EditorModel::new(buffer, Some(path)), theme, cx)` on the main thread.
    `EditorModel` isn't `Send`.
  - A path that's already open activates its tab instead.
  - The opened editor is focused.
  - Clicking a tab activates it.
  - Commands, registered by `fe_workbench::init` in fe-command's registry (T68) under a `workbench`
    key context, with their default keys as rows in `crates/fe-keymap/keymaps/default-macos.toml`
    (T73); no new `actions!` or `KeyBinding::new`:
    - `workbench.close_tab` (`cmd-w`) closes the active tab and activates its right neighbour, or the left one when it was
      last. Closing the last tab shows an empty-pane placeholder. A **dirty** tab isn't closed: it
      stays open until save prompts exist, and that is logged.
    - `workbench.next_tab` (`cmd-shift-]`) and `workbench.previous_tab` (`cmd-shift-[`) cycle the tabs.
  - A read error shows the error in the tab's place, logged. The workbench doesn't quit.
  - Deleting an open file on disk leaves its tab open.
- Tests in new files: `crates/fe-workbench/src/tests/mod.rs`, `tests/project_panel.rs`, `tests/tabs.rs`,
  `tests/launch.rs`.
  - They run in headless GPUI (`TestAppContext` / `VisualTestContext`, `gpui` with
    `features = ["test-support"]` as a dev-dependency), on the pattern of `crates/fe-gpui/src/tests.rs`.
  - Fixtures come from `fe_project::fixture` in a `tempfile` dir.
  - Clicks go to positions taken from the panel's and tab bar's laid-out row and tab bounds, as
    `fe-gpui`'s `fold_gutter_tests` do.

### fastedit

`crates/fastedit/Cargo.toml`, `crates/fastedit/src/main.rs` and `crates/fastedit/src/perf.rs`:

- `main` calls `fe_workbench::resolve_launch` and opens a `fe_workbench::Workbench`.
- `perf.rs` uses `Workbench::active_editor()`. `--perf` keeps working; no perf numbers are part of this
  card.
- `crates/fastedit/src/loader.rs` is deleted (moved).

### Workspace

- `Cargo.toml`: `fe-project`, `fe-ui` and `fe-workbench` path entries in `[workspace.dependencies]`.
- `scripts/check-no-gpui.sh` already lists `fe-ui` and `fe-workbench` in `UI_CRATES`, so there is no
  script change. `fe-project` must still print `ok`.
- This card touches no files in `crates/fe-gpui` or `crates/fe-display`. T59 is active there.

The merge size cap is 1,000 changed lines, not counting tests or docs. Keep `fe-ui` to the three
primitives above. If the card runs over, split the tab bar into a follow-up rather than growing it.

## Out of scope

Cmd-P palette UI (needs card 2), project search, splits, outline, keyboard navigation in the tree, a
resizable dock, save prompts on close, and theme hot reload (M5).

## Decided (lead defaults; the user may revisit)

- `fastedit path/to/file` roots the tree at the file's parent directory (not the enclosing git root).
- `cmd-w` on a dirty tab leaves it open until save prompts exist.
- No look/screenshot criterion here; a workbench screenshot scene is a possible later card.

## Revised by the lead (2026-10-05)

This task now runs after the command model (T68, T69), settings (T71) and keymap (T72, T73).
Where T69, T71 and T73 wired commands, settings and the keymap into `crates/fastedit/src/main.rs`'s
Workbench, move that wiring into fe-workbench with it, behaviour unchanged.
