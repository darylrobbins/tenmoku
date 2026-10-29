---
task_uid: "01m62s3f62cddcxfv7y397td2j"
task: "T376"
rev: 3
type: "refactor"
complexity: "m"
criteria:
  - id: "C1"
    text: "kiln-ui test file_icon_set::default_is_lucide: with no set installed, file_icon(name) and folder_icon(expanded) return exactly what they return today for every row of the existing file_icons tests, which pass unchanged"
    required: true
    manual: false
  - id: "C2"
    text: "file_icon_set::stub_replaces: after installing a stub IconSet (global, as kiln's other globals), file_icon, folder_icon and the new file_icon_path(name) answer from the stub, and file_icon_ref/folder_icon_ref/the path forms keep a stub's asset paths; resetting restores Lucide; file_icon_set::swapping_redraws: installing, swapping and resetting a set redraws a window already on screen with no other redraw"
    required: true
    manual: false
  - id: "C3"
    text: "fe-workbench test tests::file_icon_set_surfaces::every_surface: with Lucide, then a stub of bundled icons, then a stub of asset paths, then a reset, each installed after the surface has drawn, the project tree row (file and folder), a tab, a breadcrumb file crumb, the recent picker, the toolbar picker (file and folder), the AI changes panel and a Changes panel file row each show the installed set's icon (a probe reads the Icon/path each rendered)"
    required: true
    manual: false
  - id: "C4"
    text: "rg 'file_icon\\(|folder_icon\\(' over crates/ and kiln/ finds calls only inside kiln-ui's seam and its tests plus the gallery pages (and other crates' tests comparing against it); surfaces take file_icon_ref/folder_icon_ref or the path forms, passing the name or expanded flag straight through; no surface keeps its own match on file names"
    required: true
    manual: false
  - id: "C5"
    text: "cargo test -p kiln-ui, -p fe-gpui and -p fe-workbench pass; clippy -D warnings clean in the main workspace and in kiln/ ; no setting and no new icon set is added"
    required: true
    manual: false
---

# Route every file and folder icon in the app through one icon-set seam in kiln-ui, so swapping the set changes the project tree, tabs, breadcrumbs, pickers, AI changes and Changes panel together (Mac T1217)

## Problem
The user: "Keep current icons but let's make sure we can change the set in the future and it apply everywhere". Today kiln-ui's file_icons.rs (T115) exposes free functions `file_icon(name)` and `folder_icon(expanded)` returning the Lucide-subset `Icon`; call sites (verified with rg): fe-gpui pane_item.rs:827 (tabs), fe-workbench project_panel.rs:890-931, ai_changes_panel.rs:327 (uses `.path(file_icon(..).path())`), recent_picker.rs:353, toolbar_picker.rs:567, pane.rs:2509 (breadcrumbs), and the kiln gallery pages (lists.rs, chrome.rs). The Changes panel (git_panel.rs) and the Git Log show no file icons yet.
## Change
In file_icons.rs add an `IconSet` trait (file(name) and folder(expanded) -> an icon reference that can be a bundled Lucide Icon or an asset path) and a global holding `Arc<dyn IconSet>`, defaulting to the current Lucide mapping unchanged. `file_icon`/`folder_icon` read the global; callers that need a path use one new accessor. Give the Changes panel's file rows an icon through the seam. Do not add a setting or another set. Gallery stays on the free functions.

## Implementation notes

- **Seam** (`kiln-ui/src/file_icons.rs`): `IconSet` trait (`file(name)`, `folder(expanded)` → `IconRef`), `IconRef::{Bundled(Icon), Asset(SharedString)}` with `path()`, `bundled()` and `name()` (a bundled icon's name, else the asset path, for debug selectors), `LucideIcons` (the existing mapping, unchanged), `set_icon_set(Arc<dyn IconSet>, cx)`, `reset_icon_set(cx)`, `icon_set()`. Surfaces read `file_icon_ref`/`folder_icon_ref` or `file_icon_path`/`folder_icon_path`. `file_icon`/`folder_icon` stay for the gallery and tests: they return a bundled `Icon` and, for an asset answer, Lucide's.
- **Components carry `IconRef`** (review round 1, F1): `TreeRow::icon`, `Crumb::icon` and `Tab::icon` take `impl Into<IconRef>` (an `Icon` still works, so other callers are unchanged). `PaneItem` gains `tab_icon_ref`, defaulting to `tab_icon`; kiln-shell's pane draws it, `ItemHandle` and fe-workbench's hot-exit wrapper forward it, and `EditorView` answers it from the set (its `tab_icon` is the bundled part). `Tab::icon_kind` stays bundled-only; `Tab::icon_ref` returns the full reference. Crumb icons get a `crumb-icon:{label}:{name}` selector. fe-workbench's test-only `drawn_crumbs` still records a bundled `Icon` so its four existing users don't change; the new test reads the drawn crumb icon by selector.
- **Redraw on swap** (F2): `set_icon_set`/`reset_icon_set` take `&mut App` and call `cx.refresh_windows()`. `swapping_redraws` fails with the refresh removed (checked by hand).
- **Where the set lives:** a thread-local `Option<Arc<dyn IconSet>>`, not a GPUI `Global`, because `file_icon(name)` (C1's tests) and `PaneItem::tab_icon` readers have no `App` to hand. It acts like an app global: an app and all its windows live on its UI thread, and each GPUI test runs its app on its own thread, so one test's stub never reaches another running in parallel. A process-wide static would leak it. `a_set_installed_on_another_thread_is_not_seen` pins this.
- Surfaces that draw `svg()` themselves (recent picker, toolbar picker, AI changes, and the Changes panel's new file icons) use the path forms. Their probes are `recent-icon:`, `toolbar-picker-icon:`, `ai-changes-icon:` and `change-icon:`, each `{file name}:{asset path}`.
- The project tree's new-file field asks `file_icon_ref("")`, a file with no name yet. Symlink rows keep `Icon::FileSymlink`: the trait answers only by file name and folder state, as specced.
- **Respecs:** rev 2 added the `tests/mod.rs` files and reworded C4. Rev 3 widened scope to the components above (kiln-ui tree_row/breadcrumbs/tab_bar, kiln-shell item/lib/pane, fe-workbench hot_exit/project_panel_ops). C2 and C3 now also require asset answers to come through and swaps to redraw. C4 is back to its original reading: no surface calls `file_icon(`/`folder_icon(`.
