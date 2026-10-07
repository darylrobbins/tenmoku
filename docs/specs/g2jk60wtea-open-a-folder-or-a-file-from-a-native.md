---
task_uid: "01m492vn0e16k01bg2jk60wtea"
task: "T123"
rev: 1
type: "feature"
complexity: "m"
criteria:
  - id: "C1"
    text: "fe-workbench test tests::open_folder::open_folder_re_roots_in_place: with root A open and a.rs in a tab, cmd-o prompts for a directory only (did_prompt_for_paths; PathPromptOptions directories true, files false, multiple false); answering root B lists B's entries in the tree after the background executor runs, cmd-p finds B's files and none of A's, a search finds matches only in B, a.rs's tab stays open (dirty state kept), and RootChanged(B) is emitted once"
    required: true
    manual: false
  - id: "C2"
    text: "fe-workbench test tests::open_folder::the_old_root_is_released: after re-rooting, the old ProjectPanel and quick-open index entities are dropped (their WeakEntity no longer upgrades), a search running over A is cancelled and its results cleared while its query and options stay, and the dock keeps its visibility and active panel"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::open_folder::cancel_and_open_file: cancelling the folder prompt changes nothing; workbench.open_file prompts for files only, and answering a file outside the root opens it in the active pane's new active tab with the root unchanged; answering a file already open activates its tab"
    required: true
    manual: false
  - id: "C4"
    text: "workbench.open_folder (cmd-o, workbench) and workbench.open_file (no binding) are registered through the command registry and known to fe_workbench::command_catalog(); the menu table's File menu starts with Open Folder… and Open File… then a separator, and fastedit's tests/menus.rs::default_menus_know_every_command still passes; command_tests and keymap_tests pass with only their pinned counts updated; rg 'KeyBinding::new|actions!' finds no match in the files this card adds"
    required: true
    manual: false
---

# Open a folder or a file from a native open panel, re-rooting the workbench in place

# Open a folder or a file from a native open panel, re-rooting the workbench in place

A follow-up to the menu-bar card (T117), whose table has no File > Open… because no command existed. It
builds on:

- T63's `Workbench`, `ProjectPanel` and `Pane::open`
- T76's `QuickOpen` and the finder index, T77's `SearchPanel` and `Dock`, and T78's outline
- T79's active pane
- the menu table (`crates/fe-keymap/menus/default-macos.toml`)
- gpui-pre 0.3.7's `App::prompt_for_paths(PathPromptOptions { files, directories, multiple, prompt })`.
  It returns a oneshot of `Result<Option<Vec<PathBuf>>>`, with `None` on cancel. In tests,
  `TestAppContext::simulate_path_prompt_response` answers it and `did_prompt_for_paths` reports it.

## Problem

A project root can only be chosen on the command line at launch. There's no way to open another folder,
or a file outside the tree, from the running app.

## Change

### Commands

New file `crates/fe-workbench/src/open.rs`. Its commands are registered through the command registry,
as `dock.rs` registers its own: a registry whose handlers only name an op, which the workbench then
does. `crates/fe-workbench/src/lib.rs` gets a `mod` line, one line in `init`, and the registry in
`command_catalog()`'s chain. Don't add `KeyBinding::new` or `actions!`.

| ID | Title | Default binding | Context |
|---|---|---|---|
| `workbench.open_folder` | Open Folder… | `cmd-o` | `workbench` |
| `workbench.open_file` | Open File… | none | `workbench` |

- **`workbench.open_folder`** shows the panel with `directories: true, files: false, multiple: false`,
  prompting "Open". A chosen folder re-roots the workbench (below). Cancelling does nothing.
- **`workbench.open_file`** shows it with `files: true, directories: false, multiple: false`. The chosen
  file opens in the active pane through `Pane::open`, as a tree click does. The root doesn't change,
  even when the file lies outside it.
- An error from the panel is logged, and nothing changes.

### Re-rooting

`Workbench::set_root(root: PathBuf, window, cx)` in `crates/fe-workbench/src/workbench.rs` is the one
entry point. It replaces the project root in the current window:

- **The tree.** A new `ProjectPanel` for `root` lists in the background, as at launch. The old panel,
  and with it its tree and its `ProjectWatcher`, is dropped. Batches from the old root are ignored.
- **Quick open.** The finder index is rebuilt for `root`, and the old index's walk is cancelled or its
  results dropped.
- **Search.** The search panel searches `root`. A running search is cancelled, its results are cleared,
  and the query and options are kept.
- **The dock.** It keeps its visibility and active panel. `crates/fe-workbench/src/dock.rs` gets a
  setter for the replaced panels if `set_root` builds new ones.
- **Open tabs stay open,** dirty ones included. They're just files, so nothing unsaved is lost. New
  opens go to the active pane as before.
- **An event for later cards.** The workbench emits `RootChanged(PathBuf)`, which later cards that index
  the root hook into. M4's navigation index (T107) is one.
- The window title follows the active tab, as before.

Rebuilding the per-root entities through their constructors is preferred over adding re-root methods
to each. Re-subscribe to the new ones, and resubscribe the open-file and fs-change events.

### Menus and keys

- `crates/fe-keymap/menus/default-macos.toml`: File gets Open Folder… (`workbench.open_folder`) and Open
  File… (`workbench.open_file`) at its top, then a separator.
- `crates/fe-keymap/keymaps/default-macos.toml`: `cmd-o` binds to `workbench.open_folder` in
  `workbench`. Update the pinned counts and contexts in `crates/fe-gpui/src/command_tests.rs` and
  `crates/fe-gpui/src/keymap_tests.rs`.

### Tests

New file `crates/fe-workbench/src/tests/open_folder.rs`, declared in `tests/mod.rs`. It uses headless
GPUI, with two `tempfile` fixture roots, and answers the panel with `simulate_path_prompt_response`.

## Out of scope

- Opening in a new window, and multi-root workspaces.
- Recent folders, and File > Open Recent.
- Reloading the project's `.fastedit/settings.toml` for the new root. The settings watcher keeps the
  launch root's project file, so it's noted as a limit in the code.
- Prompting about dirty tabs: they stay open.
- Opening several files at once.
- Drag-and-drop of folders onto the window.

## Decided (lead defaults; the user may revisit)

- `cmd-o` opens a folder, the most common case. Open File… has no default key.
- Re-rooting happens in place, in the current window. Tabs stay open.
- A file outside the root opens in a tab without changing the root.
